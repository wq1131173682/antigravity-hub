/**
 * 前端功能开关。
 *
 * 与 Rust 端 `src-tauri/src/modules/feature_flags.rs` 的 `CODEX_ENABLED`
 * 保持同步：Codex CLI 集成与 Responses API 协议转换已封存停用（代码不删除，
 * 仅通过开关禁用）。修改时务必同步两端，避免前端可交互但后端已拒绝请求。
 */
export const CODEX_ENABLED = false;
