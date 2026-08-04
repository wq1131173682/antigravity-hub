use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use once_cell::sync::Lazy;

/// Persisted token usage file (in the app data directory).
const STATS_FILE: &str = "token_stats.json";
/// Minimum interval between disk saves (seconds). Throttles file I/O on the
/// proxy hot path — counters still update in memory every request, but the
/// file is only written at most this often.
const SAVE_THROTTLE_SECS: i64 = 5;

/// Aggregate token usage counters for the proxy.
///
/// Updated by `record_usage_for_platform()` / `record_streaming_for_platform()`
/// from proxy.rs:
/// - non-streaming upstream responses that include a `usage` block
///   (OpenAI / Anthropic compatible shape: `{ "usage": { "prompt_tokens": N,
///   "completion_tokens": N } }`) are counted with real token numbers;
/// - streaming responses (where we cannot cheaply parse token usage) are
///   counted as requests only.
///
/// Counters are persisted to `token_stats.json` so usage survives app
/// restarts, and are additionally broken down per platform.
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct TokenStats {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
    pub streaming_request_count: u64,
    pub first_request_at: i64,
    pub last_updated: i64,
}

impl TokenStats {
    fn add(&mut self, prompt: u64, completion: u64, now: i64) {
        if self.request_count == 0 {
            self.first_request_at = now;
        }
        self.prompt_tokens = self.prompt_tokens.saturating_add(prompt);
        self.completion_tokens = self.completion_tokens.saturating_add(completion);
        self.total_tokens = self.total_tokens.saturating_add(prompt + completion);
        self.request_count = self.request_count.saturating_add(1);
        self.last_updated = now;
    }

    fn add_streaming(&mut self, now: i64) {
        if self.request_count == 0 {
            self.first_request_at = now;
        }
        self.streaming_request_count = self.streaming_request_count.saturating_add(1);
        self.request_count = self.request_count.saturating_add(1);
        self.last_updated = now;
    }
}

/// Persisted store: global aggregate + per-platform breakdown.
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct TokenStatsStore {
    aggregate: TokenStats,
    per_platform: HashMap<String, TokenStats>,
}

/// In-memory store, seeded from disk on first access.
static TOKEN_STATS: Lazy<Mutex<TokenStatsStore>> = Lazy::new(|| {
    Mutex::new(load_from_disk().unwrap_or_default())
});

/// Timestamp of the last disk save (Unix seconds). 0 = never saved.
static LAST_SAVED: Lazy<Mutex<i64>> = Lazy::new(|| Mutex::new(0));

fn stats_file_path() -> Option<std::path::PathBuf> {
    crate::modules::platform_manager::get_data_dir()
        .ok()
        .map(|dir| dir.join(STATS_FILE))
}

fn load_from_disk() -> Option<TokenStatsStore> {
    let path = stats_file_path()?;
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save the store to disk atomically (temp file + rename, matching config.rs).
fn save_to_disk(store: &TokenStatsStore) {
    let Some(path) = stats_file_path() else { return };
    if let Ok(json) = serde_json::to_string_pretty(store) {
        // Write to a temp file then rename for crash safety.
        let temp_path = path.with_extension("json.tmp");
        if std::fs::write(&temp_path, &json).is_ok() {
            let _ = std::fs::rename(&temp_path, &path);
        }
    }
}

/// Persist now if the save throttle window has elapsed.
/// Called with the stats lock already held. After a reset(), LAST_SAVED is 0
/// so the next record always triggers an immediate save.
fn maybe_save(store: &TokenStatsStore, now: i64) {
    let mut last = LAST_SAVED.lock().unwrap_or_else(|e| e.into_inner());
    if now - *last >= SAVE_THROTTLE_SECS {
        save_to_disk(store);
        *last = now;
    }
}

/// Record token usage attributed to a specific platform (aggregate + platform).
pub fn record_usage_for_platform(platform_id: Option<&str>, prompt: u64, completion: u64) {
    let now = chrono::Utc::now().timestamp();
    if let Ok(mut store) = TOKEN_STATS.lock() {
        store.aggregate.add(prompt, completion, now);
        if let Some(pid) = platform_id {
            let entry = store.per_platform.entry(pid.to_string()).or_default();
            entry.add(prompt, completion, now);
        }
        maybe_save(&store, now);
    }
}

/// Record a streaming request attributed to a specific platform.
pub fn record_streaming_for_platform(platform_id: Option<&str>) {
    let now = chrono::Utc::now().timestamp();
    if let Ok(mut store) = TOKEN_STATS.lock() {
        store.aggregate.add_streaming(now);
        if let Some(pid) = platform_id {
            let entry = store.per_platform.entry(pid.to_string()).or_default();
            entry.add_streaming(now);
        }
        maybe_save(&store, now);
    }
}

/// Return a snapshot of the current aggregate counters.
pub fn get_summary() -> TokenStats {
    TOKEN_STATS.lock().map(|s| s.aggregate.clone()).unwrap_or_default()
}

/// Return per-platform counters keyed by platform_id.
pub fn get_platform_summary() -> HashMap<String, TokenStats> {
    TOKEN_STATS.lock().map(|s| s.per_platform.clone()).unwrap_or_default()
}

/// Reset all counters and delete the persisted file.
pub fn reset() {
    if let Ok(mut store) = TOKEN_STATS.lock() {
        *store = TokenStatsStore::default();
        if let Some(path) = stats_file_path() {
            let _ = std::fs::remove_file(&path);
        }
        if let Ok(mut last) = LAST_SAVED.lock() {
            *last = 0;
        }
    }
}
