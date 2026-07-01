use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::fs;

use super::platform_manager;

const QUOTA_WINDOW_FILE: &str = "quota_windows.json";

// ── Sliding-window tracker ──

/// A sliding usage window tracker using individual call timestamps.
/// Timestamps are pushed on record and expired ones cleaned on mutation;
/// read-side methods (len, is_empty) are O(1) and may count a few stale
/// entries between API calls, which is conservative and safe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageWindow {
    pub timestamps: VecDeque<i64>,
}

impl UsageWindow {
    pub fn new() -> Self {
        Self { timestamps: VecDeque::new() }
    }

    /// Record a call at `now` and evict entries older than `now - duration_secs`.
    pub fn record(&mut self, now: i64, duration_secs: i64) {
        let cutoff = now - duration_secs;
        while let Some(&t) = self.timestamps.front() {
            if t < cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
        self.timestamps.push_back(now);
    }

    /// Remove all timestamps older than `now - duration_secs`.
    pub fn clean_expired(&mut self, now: i64, duration_secs: i64) {
        let cutoff = now - duration_secs;
        while let Some(&t) = self.timestamps.front() {
            if t < cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    /// Remove expired AND return the count of remaining (mutable helper).
    pub fn count_active(&mut self, now: i64, duration_secs: i64) -> u32 {
        self.clean_expired(now, duration_secs);
        self.timestamps.len() as u32
    }

    /// Number of entries in the buffer (may include stale entries between records).
    pub fn len(&self) -> u32 {
        self.timestamps.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    /// Earliest timestamp in the buffer (for display purposes).
    pub fn window_start(&self) -> i64 {
        self.timestamps.front().copied().unwrap_or(0)
    }
}

/// Limits fetched from Model config
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UsageLimits {
    pub max_per_5hrs: u32,
    pub max_per_day: u32,
    pub max_per_month: u32,
}

/// Tracker for a single (model_id, key_id) pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelKeyTracker {
    pub key_id: String,
    pub model_id: String,
    pub platform_id: String,
    pub five_hour: UsageWindow,
    pub day: UsageWindow,
    pub month: UsageWindow,
    pub consecutive_429: u32,
    pub consecutive_500: u32,
    pub last_429_time: i64,
    pub last_500_time: i64,
    pub disabled_until: Option<i64>,
    pub disabled_reason: Option<String>,
}

impl ModelKeyTracker {
    pub fn new(key_id: String, model_id: String, platform_id: String) -> Self {
        Self {
            key_id,
            model_id,
            platform_id,
            five_hour: UsageWindow::new(),
            day: UsageWindow::new(),
            month: UsageWindow::new(),
            consecutive_429: 0,
            consecutive_500: 0,
            last_429_time: 0,
            last_500_time: 0,
            disabled_until: None,
            disabled_reason: None,
        }
    }

    /// Get the limits for this model from the model config
    pub fn get_limits(&self) -> Result<UsageLimits, String> {
        let models = super::model_manager::list_models(&self.platform_id)?;
        if let Some(model) = models.iter().find(|m| m.id == self.model_id) {
            Ok(UsageLimits {
                max_per_5hrs: model.per_5hour,
                max_per_day: model.per_day,
                max_per_month: model.per_month,
            })
        } else {
            // Model deleted - return no limits
            Ok(UsageLimits {
                max_per_5hrs: u32::MAX,
                max_per_day: u32::MAX,
                max_per_month: u32::MAX,
            })
        }
    }

    /// Record a successful API call — sliding-window timestamps.
    pub fn record_call(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.five_hour.record(now, 5 * 3600);
        self.day.record(now, 86400);
        self.month.record(now, 2_592_000);
    }

    /// Clean expired entries in all windows using the current time.
    pub fn clean_all_windows(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.five_hour.clean_expired(now, 5 * 3600);
        self.day.clean_expired(now, 86400);
        self.month.clean_expired(now, 2_592_000);
    }

    /// Get which windows are exceeded (based on model limits).
    /// Uses len() without cleaning — stale entries gradually decay on
    /// subsequent record_calls, so this is conservative.
    /// NOTE: A limit of 0 means "unlimited" (matching frontend's ∞ display).
    pub fn get_exceeded_windows(&self) -> Vec<String> {
        let limits = self.get_limits().unwrap_or(UsageLimits {
            max_per_5hrs: u32::MAX,
            max_per_day: u32::MAX,
            max_per_month: u32::MAX,
        });
        let mut exceeded = Vec::new();
        // limit == 0 means unlimited — skip the check (frontend shows as ∞)
        if limits.max_per_5hrs > 0 && self.five_hour.len() > limits.max_per_5hrs {
            exceeded.push("5hour".into());
        }
        if limits.max_per_day > 0 && self.day.len() > limits.max_per_day {
            exceeded.push("day".into());
        }
        if limits.max_per_month > 0 && self.month.len() > limits.max_per_month {
            exceeded.push("month".into());
        }
        exceeded
    }

    /// Check if ANY window is exceeded
    pub fn is_any_window_exceeded(&self) -> bool {
        !self.get_exceeded_windows().is_empty()
    }

    /// Record a 429 error
    /// NOTE: 429 does NOT disable the key — it only records the count.
    /// The proxy should wait and retry the same key, not switch.
    pub fn record_429(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.consecutive_429 += 1;
        self.last_429_time = now;
        // Do NOT set disabled_until — 429 is a rate limit, not an error.
        // The key remains available for retry after a short wait.
    }

    /// Record a 500 error
    pub fn record_500(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.consecutive_500 += 1;
        self.last_500_time = now;
        let backoff = std::cmp::min(60, 5 * self.consecutive_500) as i64;
        self.disabled_until = Some(now + backoff);
        self.disabled_reason = Some(format!("Server error (500 x{}) - retry after {}s", self.consecutive_500, backoff));
    }

    /// Check if this tracker is currently available
    pub fn is_available(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        if let Some(until) = self.disabled_until {
            if now < until {
                return false;
            }
        }
        !self.is_any_window_exceeded()
    }

    /// Record success (reset consecutive errors)
    pub fn record_success(&mut self) {
        self.consecutive_429 = 0;
        self.consecutive_500 = 0;
    }
}

/// Global quota window state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWindowState {
    pub trackers: Vec<ModelKeyTracker>,
    pub auto_switch_enabled: bool,
}

impl Default for QuotaWindowState {
    fn default() -> Self {
        Self {
            trackers: Vec::new(),
            auto_switch_enabled: true,
        }
    }
}

impl QuotaWindowState {
    /// Get the best available key for a specific model (lowest total usage).
    pub fn get_best_available_key_for_model(&self, model_id: &str) -> Option<&ModelKeyTracker> {
        self.trackers.iter()
            .filter(|t| t.model_id == model_id && t.is_available())
            .min_by_key(|t| t.five_hour.len() + t.day.len() + t.month.len())
    }

    pub fn get_tracker_mut(&mut self, key_id: &str, model_id: &str) -> Option<&mut ModelKeyTracker> {
        self.trackers.iter_mut().find(|t| t.key_id == key_id && t.model_id == model_id)
    }

    pub fn ensure_tracker(&mut self, key_id: &str, model_id: &str, platform_id: &str) -> &mut ModelKeyTracker {
        if !self.trackers.iter().any(|t| t.key_id == key_id && t.model_id == model_id) {
            self.trackers.push(ModelKeyTracker::new(
                key_id.to_string(),
                model_id.to_string(),
                platform_id.to_string(),
            ));
        }
        self.get_tracker_mut(key_id, model_id).unwrap()
    }

    pub fn remove_tracker(&mut self, key_id: &str, model_id: &str) {
        self.trackers.retain(|t| !(t.key_id == key_id && t.model_id == model_id));
    }

    pub fn remove_key_trackers(&mut self, key_id: &str) {
        self.trackers.retain(|t| t.key_id != key_id);
    }

    pub fn remove_model_trackers(&mut self, model_id: &str) {
        self.trackers.retain(|t| t.model_id != model_id);
    }

    pub fn remove_platform_trackers(&mut self, platform_id: &str) {
        self.trackers.retain(|t| t.platform_id != platform_id);
    }
}

// ── Global state + dirty-flag debounced persistence ──

static QUOTA_STATE: Lazy<Mutex<QuotaWindowState>> = Lazy::new(|| {
    Mutex::new(load_quota_state().unwrap_or_default())
});

/// Dirty flag: state has changed since last save.
/// Set on every mutation, cleared after a successful save.
static QUOTA_DIRTY: AtomicBool = AtomicBool::new(false);

/// Last flush timestamp — avoid writing more often than every N seconds
/// even when the dirty flag is set.
static LAST_SAVE_TS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

fn mark_dirty() {
    QUOTA_DIRTY.store(true, Ordering::Relaxed);
}

fn should_flush() -> bool {
    if !QUOTA_DIRTY.load(Ordering::Relaxed) {
        return false;
    }
    let now = chrono::Utc::now().timestamp();
    let last = LAST_SAVE_TS.load(Ordering::Relaxed);
    now - last >= 30
}

fn clear_dirty() {
    QUOTA_DIRTY.store(false, Ordering::Relaxed);
    LAST_SAVE_TS.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
}

/// Attempt to persist state to disk, but only if dirty AND at least 30s
/// have passed since the last flush.  Returns true when a write actually
/// happened.
fn try_flush(state: &QuotaWindowState) -> bool {
    if !should_flush() {
        return false;
    }
    if save_quota_state_inner(state).is_ok() {
        clear_dirty();
        true
    } else {
        false
    }
}

/// Force a flush regardless of the debounce timer.  Used before reads
/// that need up-to-date on-disk state, or on explicit user request.
fn force_flush(state: &QuotaWindowState) {
    if save_quota_state_inner(state).is_ok() {
        clear_dirty();
    }
}

// ── Serialization helpers ──

fn load_quota_state() -> Result<QuotaWindowState, String> {
    let path = get_data_dir()?.join(QUOTA_WINDOW_FILE);
    if !path.exists() {
        return Ok(QuotaWindowState::default());
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("read quota windows failed: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("parse quota windows failed: {}", e))
}

pub fn load_quota_state_internal() -> Result<QuotaWindowState, String> {
    load_quota_state()
}

fn save_quota_state_inner(state: &QuotaWindowState) -> Result<(), String> {
    let path = get_data_dir()?.join(QUOTA_WINDOW_FILE);
    let content = serde_json::to_string_pretty(state).map_err(|e| format!("serialize quota windows failed: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("write quota windows failed: {}", e))
}

fn get_data_dir() -> Result<PathBuf, String> {
    platform_manager::get_data_dir()
}

// ── Public API ──

pub fn record_api_call(key_id: &str, model_id: &str, platform_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let tracker = state.ensure_tracker(key_id, model_id, platform_id);
    tracker.record_call();
    tracker.record_success();
    mark_dirty();
    // Best-effort flush: non-blocking write at most every 30s
    let _ = try_flush(&state);
    Ok(())
}

pub fn record_429_error(key_id: &str, model_id: &str, platform_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let tracker = state.ensure_tracker(key_id, model_id, platform_id);
    tracker.record_429();
    mark_dirty();
    let _ = try_flush(&state);
    Ok(())
}

pub fn record_500_error(key_id: &str, model_id: &str, platform_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let tracker = state.ensure_tracker(key_id, model_id, platform_id);
    tracker.record_500();
    mark_dirty();
    let _ = try_flush(&state);
    Ok(())
}

/// Filter candidate key_ids down to those whose (key_id, model_id) tracker is
/// currently available — i.e. not disabled (within backoff) and not over any
/// configured quota window (5h / day / month).
///
/// Keys with no tracker yet (never used on this model) are treated as available,
/// so freshly added keys remain selectable.
///
/// Fail-open on lock poisoning: if the quota state mutex is poisoned, return
/// the candidate list unchanged rather than blocking the proxy.
pub fn filter_available_keys(
    candidates: &[String],
    model_id: &str,
    _platform_id: &str,
) -> Vec<String> {
    let mut state = match QUOTA_STATE.lock() {
        Ok(s) => s,
        Err(_) => return candidates.to_vec(),
    };
    let now = chrono::Utc::now().timestamp();
    let result: Vec<String> = candidates
        .iter()
        .filter(|key_id| {
            match state
                .trackers
                .iter_mut()
                .find(|t| t.key_id == **key_id && t.model_id == model_id)
            {
                None => true,
                Some(t) => {
                    // Refresh the in-memory view of expired timestamps so we
                    // don't reject a key whose window has just rolled over.
                    t.five_hour.clean_expired(now, 5 * 3600);
                    t.day.clean_expired(now, 86400);
                    t.month.clean_expired(now, 2_592_000);
                    t.is_available()
                }
            }
        })
        .cloned()
        .collect();
    // Mark dirty whenever the filter touched any tracker — clean_expired may have
    // evicted entries, and the next scheduler tick (within 30s) will flush the
    // cleaned state to disk. Without this, in-memory evictions are lost on
    // restart if no record_call happens in the meantime.
    mark_dirty();
    result
}

pub fn get_best_available_key_for_model(model_id: &str) -> Result<Option<String>, String> {
    let state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    Ok(state.get_best_available_key_for_model(model_id).map(|t| t.key_id.clone()))
}

fn build_window_json(window: &UsageWindow, max: u32) -> serde_json::Value {
    serde_json::json!({
        "count": window.len(),
        "max": max,
        "window_start": window.window_start(),
    })
}

pub fn get_all_window_status() -> Result<Vec<serde_json::Value>, String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    // Clean all windows before returning for accurate display
    let now = chrono::Utc::now().timestamp();
    for t in &mut state.trackers {
        t.five_hour.clean_expired(now, 5 * 3600);
        t.day.clean_expired(now, 86400);
        t.month.clean_expired(now, 2_592_000);
    }
    let statuses: Vec<serde_json::Value> = state.trackers.iter().map(|t| {
        let limits = t.get_limits().ok().unwrap_or(UsageLimits {
            max_per_5hrs: 0, max_per_day: 0, max_per_month: 0,
        });
        serde_json::json!({
            "key_id": t.key_id,
            "model_id": t.model_id,
            "platform_id": t.platform_id,
            "five_hour": build_window_json(&t.five_hour, limits.max_per_5hrs),
            "day": build_window_json(&t.day, limits.max_per_day),
            "month": build_window_json(&t.month, limits.max_per_month),
            "consecutive_429": t.consecutive_429,
            "consecutive_500": t.consecutive_500,
            "disabled_until": t.disabled_until,
            "disabled_reason": t.disabled_reason,
            "is_available": t.is_available(),
        })
    }).collect();
    // Flush after clean if dirty
    if QUOTA_DIRTY.load(Ordering::Relaxed) {
        force_flush(&state);
    }
    Ok(statuses)
}

pub fn get_key_usage(key_id: &str, model_id: &str) -> Result<serde_json::Value, String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    if let Some(t) = state.trackers.iter_mut().find(|t| t.key_id == key_id && t.model_id == model_id) {
        let now = chrono::Utc::now().timestamp();
        t.five_hour.clean_expired(now, 5 * 3600);
        t.day.clean_expired(now, 86400);
        t.month.clean_expired(now, 2_592_000);
        let limits = t.get_limits().ok().unwrap_or(UsageLimits {
            max_per_5hrs: 0, max_per_day: 0, max_per_month: 0,
        });
        Ok(serde_json::json!({
            "key_id": t.key_id,
            "model_id": t.model_id,
            "key_name": t.key_id,
            "disabled": !t.is_available(),
            "disabled_reason": t.disabled_reason,
            "five_hour": build_window_json(&t.five_hour, limits.max_per_5hrs),
            "day": build_window_json(&t.day, limits.max_per_day),
            "month": build_window_json(&t.month, limits.max_per_month),
            "is_available": t.is_available(),
        }))
    } else {
        Ok(serde_json::json!({
            "key_id": key_id,
            "model_id": model_id,
            "key_name": "",
            "disabled": false,
            "disabled_reason": null,
            "five_hour": { "count": 0, "max": 0, "window_start": 0 },
            "day": { "count": 0, "max": 0, "window_start": 0 },
            "month": { "count": 0, "max": 0, "window_start": 0 },
            "is_available": true,
        }))
    }
}

/// Get all usage for a specific model across all keys
pub fn get_model_usage(model_id: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    // Clean all trackers for fresh count
    let now = chrono::Utc::now().timestamp();
    for t in &mut state.trackers.iter_mut().filter(|t| t.model_id == model_id) {
        t.five_hour.clean_expired(now, 5 * 3600);
        t.day.clean_expired(now, 86400);
        t.month.clean_expired(now, 2_592_000);
    }
    let usages: Vec<serde_json::Value> = state.trackers.iter()
        .filter(|t| t.model_id == model_id)
        .map(|t| {
            let limits = t.get_limits().ok().unwrap_or(UsageLimits {
                max_per_5hrs: 0, max_per_day: 0, max_per_month: 0,
            });
            serde_json::json!({
                "key_id": t.key_id,
                "model_id": t.model_id,
                "key_name": t.key_id,
                "disabled": !t.is_available(),
                "disabled_reason": t.disabled_reason,
                "five_hour": build_window_json(&t.five_hour, limits.max_per_5hrs),
                "day": build_window_json(&t.day, limits.max_per_day),
                "month": build_window_json(&t.month, limits.max_per_month),
                "is_available": t.is_available(),
            })
        })
        .collect();
    Ok(usages)
}

pub fn remove_key_tracker(key_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.remove_key_trackers(key_id);
    mark_dirty();
    force_flush(&state);
    Ok(())
}

pub fn remove_model_trackers(model_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.remove_model_trackers(model_id);
    mark_dirty();
    force_flush(&state);
    Ok(())
}

pub fn remove_platform_trackers(platform_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.remove_platform_trackers(platform_id);
    mark_dirty();
    force_flush(&state);
    Ok(())
}

pub fn set_auto_switch(enabled: bool) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.auto_switch_enabled = enabled;
    mark_dirty();
    force_flush(&state);
    Ok(())
}

pub fn get_auto_switch_enabled() -> Result<bool, String> {
    let state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    Ok(state.auto_switch_enabled)
}

/// Re-enable trackers whose backoff period has expired.
pub fn clean_expired_disabled() -> Result<Vec<String>, String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let mut reenabled = Vec::new();
    for tracker in state.trackers.iter_mut() {
        if let Some(until) = tracker.disabled_until {
            if now >= until {
                tracker.disabled_until = None;
                tracker.disabled_reason = None;
                reenabled.push(tracker.key_id.clone());
            }
        }
    }
    if !reenabled.is_empty() {
        mark_dirty();
        force_flush(&state);
    } else if QUOTA_DIRTY.load(Ordering::Relaxed) {
        // Still flush if there are pending dirty changes
        force_flush(&state);
    }
    Ok(reenabled)
}

/// Clean expired window entries in all trackers (called periodically by scheduler).
pub fn clean_expired_windows() -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    for t in &mut state.trackers {
        t.five_hour.clean_expired(now, 5 * 3600);
        t.day.clean_expired(now, 86400);
        t.month.clean_expired(now, 2_592_000);
    }
    // Flush dirty state if enough time has passed
    if QUOTA_DIRTY.load(Ordering::Relaxed) {
        force_flush(&state);
    }
    Ok(())
}

pub fn initialize() {
    let state = load_quota_state().unwrap_or_default();
    if let Ok(mut s) = QUOTA_STATE.lock() {
        *s = state;
    }
}
