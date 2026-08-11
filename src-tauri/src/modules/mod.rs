pub mod config;
pub mod platform_manager;
pub mod model_manager;
pub mod key_model_map;
pub mod keystore;
pub mod proxy;
pub mod quota_window;
pub mod scheduler;
pub mod logger;
pub mod log_bridge;
pub mod i18n;
pub mod token_stats;
pub mod codex_integration;
pub mod codex_translator;
/// Responses API ↔ Chat Completions 双向转换器。
/// 当前仅由独立二进制 `responses_relay` 使用（`#[path]` 内联），
/// 主应用 lib 中不可达，故允许 dead_code 以避免误报。
#[allow(dead_code)]
pub mod responses_bridge;
