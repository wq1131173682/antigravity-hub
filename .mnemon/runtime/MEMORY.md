Antigravity Hub is a multi-platform API Key rotation proxy desktop app (Tauri v2 + React 19 + Rust). GitHub: wq1131173682/antigravity-hub. Local proxy on 127.0.0.1:8045. Single author: 王千. License: CC BY-NC-SA 4.0. Current version: 5.3.26. Single branch: master.
§
Backend modules: proxy.rs (Axum, SSE streaming, key rotation), quota_window.rs (sliding-window 5h/daily/monthly), keystore.rs (API key CRUD), platform_manager.rs, model_manager.rs, key_model_map.rs (key-model associations), token_stats.rs (persistent token stats), codex_translator.rs (archived), scheduler.rs (30s cleanup), feature_flags.rs (CODEX_ENABLED=false).
§
Frontend: React 19 + TypeScript + Ant Design + Tailwind CSS + daisyUI + Zustand + i18next (12 languages). Pages: Dashboard, Accounts, Settings. Stores: useAccountStore, useConfigStore, usePlatformStore, useViewStore, useDebugConsole. Vite port 1420, proxy /api/ → 127.0.0.1:8045.
§
Commit convention: Conventional Commits type(scope): description. feat/fix/refactor/chore/docs/style/i18n. Version bump: chore(release): bump app version to X.Y.Z. CI: ci.yml (frontend type-check+build → Rust check/clippy/fmt → cross-platform build). Release: release.yml (tag-triggered, minisign signing, GitHub release, updater.json).
§
v5.3.10 milestone: Codex integration archived (CODEX_ENABLED=false), responses API translation stopped. Code retained, UI greyed. Core proxy now only does OpenAI Chat Completions pass-through + key rotation. Previous versions: v5.3.0 (multi-turn tool call fixes), v5.2.23 (Responses API relay), v5.2.10 (token stats), v5.2.0 (model sync).
§
Proxy max_tokens injection changed from 4096 to 65536 default. Removed old logic that forced max_tokens; now only injects when missing. Reasoning models (DeepSeek, Qwen, etc.) no longer truncated by undersized max_tokens.
§
Antigravity Hub proxy: Compatible key rotation implemented (v5.3.27). On 429/5xx, retry same key N times (exponential backoff + jitter) before rotating. N = number of available keys. Prevents account-level rate limits from "blitzing" all keys. Default max_tokens injection: 65536 (was 4096). Model struct has max_output_tokens field.
§
此机器直连 github.com 常失败（git push 超时）；需先设置 git 代理 http://127.0.0.1:7890（测试 7890 端口连通）再推送。用户明确指示：网络不通先走代理，代理不通则去掉代理并停止。github.com:443 的 Test-NetConnection 可能返回 True 但 git push 仍失败，以实际 push 结果为准。
§
发版前必须先更新 tauri.conf.json 和 Cargo.toml 的版本号，再提交推送和打 tag。版本号必须与 tag 一致（如 v5.3.29 → 5.3.29），否则 Release workflow 的版本检查会失败。
§
此机器直连 github.com 常失败（git push 超时）；需先设置 git 代理 http://127.0.0.1:7890 再推送。测试 7890 端口连通后，用 git config --global http.proxy http://127.0.0.1:7890 和 https.proxy 设置代理。如果代理不行则去掉代理（git config --global --unset http.proxy）并停止。
§
check_for_updates uses event-driven architecture: backend emits `update:check_result` event (status: available/up_to_date/error with version, current_version, rid); frontend Settings.tsx listens with `listen('update:check_result', ...)` and shows status + install button. Install button calls `download_and_install` via `tauriInvoke` with a Channel.
§
Proxy models endpoint behavior: `/v1/models` at platform level is intercepted by handle_models_request (returns locally configured models). `/models` (without v1) at platform level passes through to upstream API — clients like DSH use this to discover upstream models. Global-level (no prefix) `/v1/models` and `/models` both return ALL platforms' local models.
§
Release workflow: just bump version → push master → tag → push tag. CI auto-runs Release workflow and publishes. No need to wait for completion. If version doesn't match tag, CI fails fast at the "Verify tag version matches Cargo.toml" step.
