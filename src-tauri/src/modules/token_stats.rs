use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use once_cell::sync::Lazy;

/// Aggregate token usage counters for the current proxy session.
///
/// Updated by `record_usage()` from proxy.rs whenever a non-streaming
/// upstream response includes a `usage` block (OpenAI / Anthropic compatible
/// shape: `{ "usage": { "prompt_tokens": N, "completion_tokens": N } }`).
///
/// Counters are in-memory only and reset on app restart — this is a "live
/// session counter" used for the dashboard summary, not a durable ledger.
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

static TOKEN_STATS: Lazy<Mutex<TokenStats>> = Lazy::new(|| Mutex::new(TokenStats::default()));

/// Record token usage from a non-streaming upstream response.
/// `prompt` and `completion` are taken straight from `usage.prompt_tokens` /
/// `usage.completion_tokens`; the caller has already parsed the body.
pub fn record_usage(prompt: u64, completion: u64) {
    let now = chrono::Utc::now().timestamp();
    if let Ok(mut stats) = TOKEN_STATS.lock() {
        stats.add(prompt, completion, now);
    }
}

/// Record a streaming request that we could not extract tokens from
/// (keeps the request counter honest — `request_count` reflects every
/// successful upstream call, not just the ones we could parse).
pub fn record_streaming_request() {
    let now = chrono::Utc::now().timestamp();
    if let Ok(mut stats) = TOKEN_STATS.lock() {
        stats.add_streaming(now);
    }
}

/// Return a snapshot of the current aggregate counters.
pub fn get_summary() -> TokenStats {
    TOKEN_STATS.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Reset all counters (e.g. when the user clicks a "Reset" button).
pub fn reset() {
    if let Ok(mut stats) = TOKEN_STATS.lock() {
        *stats = TokenStats::default();
    }
}
