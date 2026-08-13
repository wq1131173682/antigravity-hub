//! 功能开关（运行时常量）。
//!
//! 设计意图：被封存的功能**代码不删除**，仅通过此开关禁用。修改 `CODEX_ENABLED`
//! 即可在「启用 / 停用」之间切换，无需改动业务代码。
//!
//! 当 `CODEX_ENABLED = false`（默认）时：
//! - 代理不再把 `/v1/responses` 翻译为 `/v1/chat/completions`；
//! - 不再对请求 / 响应体做 Responses ↔ Chat Completions 协议转换；
//! - 与之相关的 IPC 命令（Codex 集成）直接返回「功能已停用」。
//!
//! 应用现在只做一件事：OpenAI 兼容协议的**直接穿透** + 我们的 API Key
//! 轮转 / 配额逻辑。

/// 是否启用 Codex CLI 集成与 Responses API 协议转换。
///
/// 设为 `true` 可重新启用被封存的 Codex 相关功能（仅用于向后兼容 / 调试）。
pub const CODEX_ENABLED: bool = false;
