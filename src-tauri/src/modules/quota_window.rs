use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::fs;

use super::platform_manager;

const QUOTA_WINDOW_FILE: &str = "quota_windows.json";

/// A usage window tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageWindow {
    pub count: u32,
    pub window_start: i64,
}

impl UsageWindow {
    pub fn new(window_start: i64) -> Self {
        Self { count: 0, window_start }
    }

    pub fn is_expired(&self, duration_secs: i64, now: i64) -> bool {
        now - self.window_start >= duration_secs
    }

    pub fn reset(&mut self, now: i64) {
        self.count = 0;
        self.window_start = now;
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
        let now = chrono::Utc::now().timestamp();
        Self {
            key_id,
            model_id,
            platform_id,
            five_hour: UsageWindow::new(now),
            day: UsageWindow::new(now),
            month: UsageWindow::new(now),
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

    /// Record a successful API call
    pub fn record_call(&mut self) {
        let now = chrono::Utc::now().timestamp();
        let windows: Vec<(&mut UsageWindow, i64)> = vec![
            (&mut self.five_hour, 5 * 3600),
            (&mut self.day, 86400),
            (&mut self.month, 2592000),
        ];
        for (window, duration) in windows {
            if window.is_expired(duration, now) {
                window.reset(now);
            }
            window.count += 1;
        }
    }

    /// Get which windows are exceeded (based on model limits)
    pub fn get_exceeded_windows(&self) -> Vec<String> {
        let limits = self.get_limits().unwrap_or(UsageLimits {
            max_per_5hrs: u32::MAX,
            max_per_day: u32::MAX,
            max_per_month: u32::MAX,
        });
        let mut exceeded = Vec::new();
        if self.five_hour.count > limits.max_per_5hrs { exceeded.push("5hour".into()); }
        if self.day.count > limits.max_per_day { exceeded.push("day".into()); }
        if self.month.count > limits.max_per_month { exceeded.push("month".into()); }
        exceeded
    }

    /// Check if ANY window is exceeded
    pub fn is_any_window_exceeded(&self) -> bool {
        !self.get_exceeded_windows().is_empty()
    }

    /// Record a 429 error
    pub fn record_429(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.consecutive_429 += 1;
        self.last_429_time = now;
        let backoff = (std::cmp::min(120, 1 << self.consecutive_429) * 60) as i64;
        self.disabled_until = Some(now + backoff);
        self.disabled_reason = Some(format!("Rate limited (429 x{}) - retry after {}s", self.consecutive_429, backoff));
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
    /// Get the best available key for a specific model
    pub fn get_best_available_key_for_model(&self, model_id: &str) -> Option<&ModelKeyTracker> {
        let available: Vec<&ModelKeyTracker> = self.trackers.iter()
            .filter(|t| t.model_id == model_id && t.is_available())
            .collect();
        if available.is_empty() {
            return None;
        }
        available.into_iter().min_by_key(|t| {
            t.five_hour.count + t.day.count + t.month.count
        })
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

static QUOTA_STATE: Lazy<Mutex<QuotaWindowState>> = Lazy::new(|| {
    Mutex::new(load_quota_state().unwrap_or_default())
});

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

fn save_quota_state(state: &QuotaWindowState) -> Result<(), String> {
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
    save_quota_state(&state)
}

pub fn record_429_error(key_id: &str, model_id: &str, platform_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let tracker = state.ensure_tracker(key_id, model_id, platform_id);
    tracker.record_429();
    save_quota_state(&state)
}

pub fn record_500_error(key_id: &str, model_id: &str, platform_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let tracker = state.ensure_tracker(key_id, model_id, platform_id);
    tracker.record_500();
    save_quota_state(&state)
}

pub fn get_best_available_key_for_model(model_id: &str) -> Result<Option<String>, String> {
    let state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    Ok(state.get_best_available_key_for_model(model_id).map(|t| t.key_id.clone()))
}

pub fn get_all_window_status() -> Result<Vec<serde_json::Value>, String> {
    let state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    let statuses: Vec<serde_json::Value> = state.trackers.iter().map(|t| {
        let limits = t.get_limits().ok().unwrap_or(UsageLimits {
            max_per_5hrs: 0, max_per_day: 0, max_per_month: 0,
        });
        serde_json::json!({
            "key_id": t.key_id,
            "model_id": t.model_id,
            "platform_id": t.platform_id,
            "five_hour": { "count": t.five_hour.count, "max": limits.max_per_5hrs, "window_start": t.five_hour.window_start },
            "day": { "count": t.day.count, "max": limits.max_per_day, "window_start": t.day.window_start },
            "month": { "count": t.month.count, "max": limits.max_per_month, "window_start": t.month.window_start },
            "consecutive_429": t.consecutive_429,
            "consecutive_500": t.consecutive_500,
            "disabled_until": t.disabled_until,
            "disabled_reason": t.disabled_reason,
            "is_available": t.is_available(),
        })
    }).collect();
    Ok(statuses)
}

pub fn get_key_usage(key_id: &str, model_id: &str) -> Result<serde_json::Value, String> {
    let state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    if let Some(t) = state.trackers.iter().find(|t| t.key_id == key_id && t.model_id == model_id) {
        let limits = t.get_limits().ok().unwrap_or(UsageLimits {
            max_per_5hrs: 0, max_per_day: 0, max_per_month: 0,
        });
        Ok(serde_json::json!({
            "key_id": t.key_id,
            "model_id": t.model_id,
            "key_name": t.key_id,
            "disabled": !t.is_available(),
            "disabled_reason": t.disabled_reason,
            "five_hour": { "count": t.five_hour.count, "max": limits.max_per_5hrs, "window_start": t.five_hour.window_start },
            "day": { "count": t.day.count, "max": limits.max_per_day, "window_start": t.day.window_start },
            "month": { "count": t.month.count, "max": limits.max_per_month, "window_start": t.month.window_start },
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
    let state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
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
                "five_hour": { "count": t.five_hour.count, "max": limits.max_per_5hrs, "window_start": t.five_hour.window_start },
                "day": { "count": t.day.count, "max": limits.max_per_day, "window_start": t.day.window_start },
                "month": { "count": t.month.count, "max": limits.max_per_month, "window_start": t.month.window_start },
                "is_available": t.is_available(),
            })
        })
        .collect();
    Ok(usages)
}

pub fn remove_key_tracker(key_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.remove_key_trackers(key_id);
    save_quota_state(&state)
}

pub fn remove_model_trackers(model_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.remove_model_trackers(model_id);
    save_quota_state(&state)
}

pub fn remove_platform_trackers(platform_id: &str) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.remove_platform_trackers(platform_id);
    save_quota_state(&state)
}

pub fn set_auto_switch(enabled: bool) -> Result<(), String> {
    let mut state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    state.auto_switch_enabled = enabled;
    save_quota_state(&state)
}

pub fn get_auto_switch_enabled() -> Result<bool, String> {
    let state = QUOTA_STATE.lock().map_err(|e| e.to_string())?;
    Ok(state.auto_switch_enabled)
}

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
        save_quota_state(&state)?;
    }
    Ok(reenabled)
}

pub fn initialize() {
    let state = load_quota_state().unwrap_or_default();
    if let Ok(mut s) = QUOTA_STATE.lock() {
        *s = state;
    }
}
