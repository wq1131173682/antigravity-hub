---
id: "55d6d91f-adf4-418c-bf40-048a6a087138"
title: "Antigravity Hub — Project Knowledge Base"
description: "Project knowledge base update: v5.3.28 release, compatible key rotation, check-for-updates, workflow fixes"
status: "active"
created_at: "2026-08-19T10:17:34.590Z"
updated_at: "2026-08-21T09:18:42.202Z"
content_hash: "93a0da40f186011280ecc7e4eb318f0988ffc1291220e34dae5d88a780ff9388"
source_paths:
  - "README.md"
  - "README_EN.md"
  - "CHANGELOG.md"
  - "CONTRIBUTING.md"
  - "package.json"
  - "src-tauri/Cargo.toml"
  - "src-tauri/tauri.conf.json"
  - "src-tauri/src/lib.rs"
  - "src-tauri/src/main.rs"
  - "src-tauri/src/modules/mod.rs"
  - "src-tauri/src/models/mod.rs"
  - "src-tauri/src/commands/mod.rs"
  - "src-tauri/src/modules/feature_flags.rs"
  - "src-tauri/src/modules/proxy.rs"
  - "src-tauri/src/modules/quota_window.rs"
  - "src-tauri/src/modules/keystore.rs"
  - "src-tauri/src/modules/scheduler.rs"
  - "src-tauri/src/modules/platform_manager.rs"
  - "src-tauri/src/modules/token_stats.rs"
  - "src-tauri/src/modules/config.rs"
  - "src-tauri/src/models/apikey.rs"
  - "src-tauri/src/models/platform.rs"
  - "src-tauri/src/models/model.rs"
  - "src-tauri/src/models/config.rs"
  - "src/App.tsx"
  - "src/main.tsx"
  - "vite.config.ts"
  - ".gitignore"
  - ".github/workflows/ci.yml"
  - ".github/workflows/release.yml"
session_ids:
  - "session-0317c75f-a5c3-4037-9a2d-b56ff875cd2f"
  - "d42d30a0-9a61-42a7-9314-525370e2ba22"
  - "22017bd2-718c-4ea3-9c45-14f2bb1dec9b"
  - "faa99c5b-3bdd-42bb-a869-c7e86ba2365b"
  - "0cfe61f2-cce4-4d90-a568-df678c3d40b0"
  - "30a7a32b-efc3-4470-905b-06275dd0c630"
  - "604c5c81-8426-401d-8501-61ef91b99d4b"
memory_body_ids:
  []
---

# Antigravity Hub — Project Knowledge Base

## Project Identity

- **Name**: Antigravity Hub
- **Package name**: `antigravity-tools`
- **Cargo crate**: `antigravity_tools`
- **Identifier**: `com.lbjlaq.antigravity-tools`
- **GitHub**: https://github.com/wq1131173682/antigravity-hub
- **License**: CC BY-NC-SA 4.0
- **Derived from**: [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) by lbjlaq
- **Authors**: 王千 (wq1131173682)

## Purpose

A locally running **API Key rotation proxy** desktop application. It manages multiple API keys across multiple platforms (Sensenova, Agnes, OpenAI-compatible gateways, etc.), starts a local OpenAI-compatible proxy, and automatically handles:

- **Load balancing** — requests distributed across available keys
- **Failover** — 429/5xx auto-rotate with exponential backoff
- **Quota management** — per (model × key) sliding-window (5h/daily/monthly) tracking
- **OpenAI protocol pass-through** — Chat Completions forwarded upstream as-is with local key rotation

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | React 19, TypeScript, Ant Design v5, Tailwind CSS, daisyUI, Zustand, i18next (12 languages), Recharts, framer-motion |
| Backend | Rust, Tauri v2, Axum (local proxy), reqwest, tokio, serde, chrono, parking_lot |
| Storage | JSON files (`gui_config.json`, `api_keys.json`, `quota_windows.json`, `token_stats.json`) in `~/.antigravity_tools/` |
| Proxy Protocol | OpenAI Chat Completions (pass-through), Responses API (archived since v5.3.10) |
| CI/CD | GitHub Actions (ci.yml + release.yml), Tauri updater, minisign signing |

## Version History (Git)

- **Repository**: Single branch `master` (origin/master)
- **Current version**: 5.3.28
- **Version range in git**: 5.0.0 → 5.3.28
- **Commit convention**: `type(scope): description` (Conventional Commits)
- **Common types**: `feat`, `fix`, `refactor`, `chore`, `docs`, `style`, `i18n`
- **Version bump pattern**: `chore(release): bump app version to X.Y.Z`
- **CRITICAL RELEASE RULE**: before committing/pushing code, MUST bump `src-tauri/Cargo.toml` AND `src-tauri/tauri.conf.json` to the target version. The git tag and both files must match (e.g. tag `v5.3.29` ↔ version `5.3.29`). The Release workflow now fails fast if they mismatch. `package.json` version is the frontend's and does NOT affect Tauri bundle naming (may be left unsynced).

### Major Milestones

| Version | Date | Significance |
|---|---|---|
| v5.3.28 | 2026-08-21 | ⭐ **Release pipeline finally green**: updater.json signature/URL issues resolved (artifact download pattern `bundle-*-r<run_id>`, version verification, pre-build tag↔version check). **Compatible key rotation** (retry same key N times before rotating). **Check-for-updates button** in Settings + `check_for_updates`/`get_app_version` Tauri commands. |
| v5.3.27 | 2026-08-20 | ⭐ **max_output_tokens support**: Added to Model struct, upstream extraction, `/models` endpoint, and UI. Default `max_tokens` injection changed from 4096 to 65536. Sensenova `context_length`/`max_output_length` fields now correctly parsed. |
| v5.3.26 | 2026-08-20 | Removed reasoning-to-content mirroring in SSE normalization |
| v5.3.10 | 2026-08-13 | ⭐ Milestone: Codex integration archived; core stability fixes (SSE keepalive, 429 cooling, Connection: close) |
| v5.3.0 | 2026-08-11 | Responses API multi-turn tool call fixes; session context caching; `/v1/models` endpoint |
| v5.2.23 | 2026-08-11 | Responses API relay service (independent binary) |
| v5.2.10 | 2026-08-04 | Codex model rewrite + persistent per-platform token stats |
| v5.2.0 | 2026-07-28 | Upstream model sync with multi-URL fallback; model testing |
| v5.0.0 | 2026-07-26 | Version alignment with Cargo.toml |

## Architecture

### Rust Backend (`src-tauri/src/`)

#### Modules (`modules/`)

| Module | Purpose |
|---|---|
| `proxy.rs` | Local Axum HTTP proxy server (~2500 lines). SSE streaming, key rotation (compatible rotation: retry-same-key-then-rotate), request forwarding |
| `quota_window.rs` | Sliding-window quota tracking (5h/daily/monthly). 429/500 error recording with exponential backoff (max 30s cooldown) |
| `keystore.rs` | API key CRUD + persistence. Round-robin rotation index per platform |
| `platform_manager.rs` | Platform CRUD (add/update/delete/reorder) |
| `model_manager.rs` | Model CRUD + upstream model sync |
| `key_model_map.rs` | Key-Model associations (many-to-many) |
| `token_stats.rs` | Token usage statistics (persistent, per-platform breakdown) |
| `codex_translator.rs` | Responses API ↔ Chat Completions SSE translation (archived) |
| `codex_integration.rs` | Codex CLI config apply/restore/auth-clear (archived) |
| `responses_bridge.rs` | Independent relay binary for Responses API bidirectional conversion |
| `diagnostics.rs` | A/B-class session-stop diagnosis module + CLI |
| `feature_flags.rs` | `CODEX_ENABLED = false` — toggle for archived Codex features |
| `scheduler.rs` | Background cleanup every 30 seconds (expired quotas, disabled keys) |
| `config.rs` | App configuration load/save |
| `i18n.rs` | Tray menu i18n (Chinese/English) |
| `logger.rs` / `log_bridge.rs` | Logging + debug console bridge |

#### Commands (`commands/mod.rs`)

~610 lines of Tauri IPC commands organized by domain:
- Platform management (5 commands)
- Model management (8 commands)
- Key-Model associations (4 commands)
- API Key management (7 commands)
- Quota window tracking (9 commands)
- Token stats (3 commands)
- Proxy control (5 commands)
- Config (5 commands)
- Window controls (3 commands)
- Utilities (7 commands + `get_lan_ip`)
- **Update checks** (`check_for_updates`, `get_app_version`) — added v5.3.28
- Codex integration (5 commands, archived)
- Debug console (5 commands)

#### Models (`models/`)

- **`Platform`**: `id`, `name`, `base_url`, `path_prefix`, `notes`, `sort_order`, `created_at`, `base_url_overrides: Vec<PathOverride>`, `default_model: Option<String>`
- **`PathOverride`**: `path_prefix`, `base_url` — allows a single platform to serve endpoints at different API roots
- **`ApiKey`**: `id`, `platform_id`, `key_value`, `name`, `status: KeyStatus`, `disabled_reason`, `disabled_until`, `sort_order`, `created_at`
- **`KeyStatus`**: `Active` / `Disabled`
- **`Model`**: `id`, `platform_id`, `model_name`, `display_name`, `per_5hour` (default 3000), `per_day` (default 10000), `per_month` (default 100000), `sort_order`, `created_at`, `max_input_tokens: Option<u64>`, `max_output_tokens: Option<u64>` — added v5.3.27
- **`AppConfig`**: `language`, `theme`, `proxy_port` (default 8080), `proxy_host` (default `127.0.0.1`), `auto_switch` (default true), `upstream_proxy_url: Option<String>`, `platforms: Vec<Platform>`

### Frontend (`src/`)

#### Pages
- **Dashboard** (`Dashboard.tsx`): Quota overview, model cards, usage charts
- **Accounts** (`Accounts.tsx`): Platform management, key management, model-key mapping, model quota editing (including max_output_tokens)
- **Settings** (`Settings.tsx`): App config, proxy settings, language/theme, Codex integration, **Check for Updates button** (v5.3.28)

#### Stores (Zustand, single domain per store)
- `useAccountStore.ts`: Account management
- `useConfigStore.ts`: App configuration
- `usePlatformStore.ts`: Platform state (includes addModel/updateModelLimits with maxOutputTokens)
- `useViewStore.ts`: UI view state
- `useDebugConsole.ts`: Debug console state

#### Configuration
- `vite.config.ts`: Port 1420, proxy `/api/` → `127.0.0.1:8045`
- `tailwind.config.js`: Tailwind + daisyUI
- `tsconfig.json`: Strict TypeScript, `noUnusedLocals`, `noUnusedParameters`

## Key Design Decisions

### Proxy Architecture
- **HTTP client**: `reqwest` with 3600s total timeout, 20s connect timeout, 60s TCP keepalive, `pool_max_idle_per_host(0)` (no idle connection reuse to avoid stale socket hangs)
- **Compatible key rotation** (v5.3.28): on 429/5xx, retry the SAME key N times (exponential backoff `2^errors` capped 32s + jitter 0–50%) before rotating. Rotation threshold: `current_key_errors >= keys_to_try.len()`. Counter reset on success/key-switch. Rationale: immediate rotation "blitzes" all keys in ms, triggering account-level rate limits (observed with SenseNova).
- **SSE streaming**: Multi-line SSE fragment reassembly; `SseKeepaliveStream` injects `: ping\n\n` every 25s on idle; `Connection: close` header at stream end
- **Stream termination**: Only on EOF (not on `finish_reason`/`[DONE]`); 30s post-termination window for multi-segment tool calls
- **Key disabling**: manual disable only (keystore `set_key_status`); proxy NEVER auto-disables keys on 429/5xx (comment: "keys are a precious resource"). Scheduler re-enables keys after quota backoff expires.

### Reasoning Effort (`reasoning_effort`) Handling
- **Passthrough for Chat Completions**: The `reasoning_effort` field is forwarded to upstream unchanged for non-Codex requests.
- **Codex Responses API translation** (`codex_translator.rs:640-656`): When `CODEX_ENABLED=true`, converts `{"reasoning": {"effort": "medium"}}` → `"reasoning_effort": "medium"`.
- **Model-level sanitization** (`codex_translator.rs:953-976`): `sanitize_reasoning_effort_for_model()` strips the field entirely for Mistral family and Gemini.

### max_tokens Handling (v5.3.27+)
- `parse_and_prepare_body`: injects `max_tokens=65536` when the client omits it (was 4096 before v5.3.27; the 4096 cap truncated reasoning-model thinking output, returning `finish_reason: "length"`).
- Upstream model sync now extracts `context_length` (sensenova) + `max_output_length` / `max_output_tokens` into `max_input_tokens` / `max_output_tokens`.

### Update Mechanism
- **Tauri updater plugin** (`tauri_plugin_updater`): checks `https://github.com/wq1131173682/antigravity-hub/releases/latest/download/updater.json` on startup (`dialog: true` shows native dialog). `check_for_updates` command (v5.3.28) lets the Settings button trigger the same check.
- **updater.json** is generated by the Release workflow's `updater` job via `scripts/sign_updater.py` (minisign-ed25519, key from `TAURI_SIGNING_PRIVATE_KEY` secret). Only the GitHub Actions job (or someone holding the secret) can produce valid signatures.

### Release Workflow (`.github/workflows/release.yml`) — v5.3.28 hardening
- Artifacts named `bundle-<target>-r<run_id>` (unique per run); download pattern MUST be `bundle-*-r${{ github.run_id }}` (the `r` prefix is critical — a mismatch silently downloads nothing).
- "Verify artifacts were downloaded" step fails loudly on empty `bundles/`.
- "Verify bundle versions match tag" step: every `.msi/.exe/.AppImage/.deb` filename must contain the tag version; catches stale artifacts before signing.
- Pre-build "Verify tag version matches Cargo.toml" step (both build and updater jobs) — fails fast on tag↔code version mismatch.
- Publish: delete existing release (idempotent) → create with updater.json → upload only installers with `--clobber` (per-file WARN, never aborts) → verify all updater.json URLs return HTTP 200.

## Operational Notes

- **Local git push requires proxy**: this machine's direct connection to github.com frequently times out. Before `git push`: test `127.0.0.1:7890`, then set `git config --global http.proxy http://127.0.0.1:7890` and `https.proxy`. If the proxy is down, unset (`git config --global --unset http.proxy`) and stop rather than pushing unverified commits.
- **Windows CI build failures**: `Invoke-WebRequest https://github.com/webview/webview2/...` intermittently fails — a transient GitHub Actions network issue unrelated to code. Rerun with `gh run rerun <run-id> --job <windows-job-id>`.
- **Auto-update relies on `releases/latest`**: the current latest release's `updater.json` must be valid for in-app updates to work.
- **Settings update i18n**: `settings.update.*` keys were added to `src/locales/zh.json` only; other locale files still lack them (fall back to key names).
