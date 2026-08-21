---
id: "55d6d91f-adf4-418c-bf40-048a6a087138"
title: "Antigravity Hub — Project Knowledge Base"
description: "Complete project knowledge extracted from codebase and git history: purpose, architecture, conventions, data models, and historical decisions."
status: "active"
created_at: "2026-08-19T10:17:34.590Z"
updated_at: "2026-08-21T05:58:16.308Z"
content_hash: "7c2a2fb87f9d128f30554030333326871d94b2b9a714e70f02e76bb97e834f47"
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
- **Current version**: 5.3.27
- **Version range in git**: 5.0.0 → 5.3.27
- **Commit convention**: `type(scope): description` (Conventional Commits)
- **Common types**: `feat`, `fix`, `refactor`, `chore`, `docs`, `style`, `i18n`
- **Version bump pattern**: `chore(release): bump app version to X.Y.Z`

### Major Milestones

| Version | Date | Significance |
|---|---|---|
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
| `proxy.rs` | Local Axum HTTP proxy server (~2480 lines). SSE streaming, key rotation, request forwarding, HTTP client with `reqwest` |
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

~592 lines of Tauri IPC commands organized by domain:
- Platform management (5 commands)
- Model management (8 commands)
- Key-Model associations (4 commands)
- API Key management (7 commands)
- Quota window tracking (9 commands)
- Token stats (3 commands)
- Proxy control (5 commands)
- Config (5 commands)
- Window controls (3 commands)
- Utilities (7 commands)
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
- **Settings** (`Settings.tsx`): App config, proxy settings, language/theme, Codex integration

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
- **Key rotation**: Round-robin with weighted selection; 429/500 triggers exponential backoff (max 30s cooldown)
- **SSE streaming**: Multi-line SSE fragment reassembly; `SseKeepaliveStream` injects `: ping\n\n` every 25s on idle; `Connection: close` header at stream end
- **Stream termination**: Only on EOF (not on `finish_reason`/`[DONE]`); 30s post-termination window for multi-segment tool calls

### Reasoning Effort (`reasoning_effort`) Handling
- **Passthrough for Chat Completions**: The `reasoning_effort` field is forwarded to upstream unchanged for non-Codex requests. The proxy does not intercept or modify it.
- **Codex Responses API translation** (`codex_translator.rs:640-656`): When `CODEX_ENABLED=true`, the proxy converts OpenAI Responses API `{"reasoning": {"effort": "medium"}}` to Chat Completions format `"reasoning_effort": "medium"` — the format expected by DeepSeek, Qwen, Kimi, etc.
- **Model-level sanitization** (`codex_translator.rs:953-976`): `sanitize_reasoning_effort_for_model()` strips the field entirely for Mistral family (codestral, mistral-*, pixtral-*) and Google Gemini models, which reject ANY value with HTTP 400 (code 3051). Other models pass through untouched.
- **Response-side extraction** (`codex_translator.rs:2027+`): `extract_reasoning_content()` normalizes multiple upstream field names (`reasoning_content`, `reasoning`, `thinking`, `thinking_content`, `reasoning_text`, `thought`, `thoughts`) into a unified `reasoning` output item for Codex Responses API clients.

### `max_tokens` Handling (v5.3.27)
- **Default injection**: When the client omits `max_tokens` in `/chat/completions` or `/completions` requests, the proxy injects `max_tokens=65536` (changed from 4096 in v5.3.27). This avoids truncating reasoning models whose `reasoning_content` would otherwise be capped by an undersized limit.
- **Passthrough**: When the client provides `max_tokens` (any value 1–65536), it is forwarded unchanged.
- **Rationale**: Modern reasoning models (DeepSeek-R1, Qwen3-Thinking, etc.) typically support 32K–131K output tokens. Sensenova upstream returns `max_output_length` values of 65536 (most models) and 131072 (glm-5.2). The 65536 default covers the majority.

### `max_output_tokens` Support (v5.3.27)
- **Model struct**: Added `max_output_tokens: Option<u64>` field to `Model` (model.rs)
- **Upstream extraction** (`proxy.rs:fetch_upstream_model_list`): Now extracts `max_output_tokens` / `max_output_length` from upstream `/v1/models` responses. Also fixed `context_length` extraction (sensenova uses `context_length` as the field name, which was previously missing from the lookup chain).
- **`/v1/models` endpoint**: Returns `max_output_tokens` when available, alongside existing `max_input_tokens`.
- **Frontend UI**: AddModelDialog and EditModelQuotaDialog both display a "输出上限 / Output (max_output_tokens)" field with default 65536.
- **Only sensenova upstream** returns `max_output_length` in `/v1/models`. Agnes and Nvidia return minimal OpenAI-compatible format without this field.

### App Config Storage
- **Location**: `C:\Users\<user>\.antigravity_tools\`
- **Config file**: `gui_config.json` (platform list, proxy settings, theme)
- **Other data files**: `api_keys.json`, `models.json`, `key_model_map.json`, `quota_windows.json`, `token_stats.json`, `proxy_logs.db`, `security.db`, `user_tokens.db`, `token_stats.db`
- **Persistence**: Tauri filesystem plugin writes to the app data directory; no SQLite for core data (JSON files except for logs/security which use SQLite)

## Key Design Decisions

### Proxy Architecture
- **HTTP client**: `reqwest` with 3600s total timeout, 20s connect timeout, 60s TCP keepalive, `pool_max_idle_per_host(0)` (no idle connection reuse to avoid stale socket hangs)
- **Key rotation**: Round-robin with weighted selection; 429/500 triggers exponential backoff (max 30s cooldown)
- **SSE streaming**: Multi-line SSE fragment reassembly; `SseKeepaliveStream` injects `: ping\n\n` every 25s on idle; `Connection: close` header at stream end
- **Stream termination**: Only on EOF (not on `finish_reason`/`[DONE]`); 30s post-termination window for multi-segment tool calls

### Reasoning Effort (`reasoning_effort`) Handling
- **Passthrough for Chat Completions**: The `reasoning_effort` field is forwarded to upstream unchanged for non-Codex requests. The proxy does not intercept or modify it.
- **Codex Responses API translation** (`codex_translator.rs:640-656`): When `CODEX_ENABLED=true`, converts OpenAI Responses API `{"reasoning": {"effort": "medium"}}` to Chat Completions format `"reasoning_effort": "medium"`.
- **Model-level sanitization** (`codex_translator.rs:953-976`): Strips `reasoning_effort` for Mistral family and Google Gemini models (HTTP 400 rejection).
- **Response-side extraction** (`codex_translator.rs:2027+`): Normalizes multiple upstream field names (`reasoning_content`, `reasoning`, `thinking`, `thinking_content`, `reasoning_text`, `thought`, `thoughts`) into unified `reasoning` output item.

### `max_tokens` Handling (v5.3.27)
- **Default injection**: When client omits `max_tokens` in `/chat/completions` or `/completions`, injects `max_tokens=65536` (changed from 4096 in v5.3.27). This avoids truncating reasoning models whose `reasoning_content` would otherwise be capped by an undersized limit.
- **Passthrough**: When client provides `max_tokens` (1–65536), forwarded unchanged.
- **Rationale**: Modern reasoning models typically support 32K–131K output tokens. Sensenova upstream reports `max_output_length` values of 65536 (most models) and 131072 (glm-5.2).

### `max_output_tokens` Support (v5.3.27)
- **Model struct**: Added `max_output_tokens: Option<u64>` field.
- **Upstream extraction**: `fetch_upstream_model_list` now extracts `context_length` (sensenova field name) and `max_output_length`/`max_output_tokens`.
- **`/v1/models` endpoint**: Returns `max_output_tokens` when available.
- **Frontend UI**: AddModelDialog and EditModelQuotaDialog both display a max_output_tokens field with default 65536.
- **Only sensenova upstream** returns `max_output_length` in `/v1/models`. Agnes and Nvidia return minimal OpenAI-compatible format without this field.

### App Config Storage
- **Location**: `C:\Users\<user>\.antigravity_tools\`
- **Config file**: `gui_config.json` (platform list, proxy settings, theme)
- **Other data files**: `api_keys.json`, `models.json`, `key_model_map.json`, `quota_windows.json`, `token_stats.json`, `proxy_logs.db`, `security.db`, `user_tokens.db`, `token_stats.db`
- **Persistence**: Tauri filesystem plugin writes to the app data directory; JSON files except for logs/security which use SQLite

### Upstream `/v1/models` Response Formats
- **Sensenova** (`token.sensenova.cn`): Rich metadata — `context_length`, `max_output_length`, `supported_features`, `input_modalities`, `output_modalities`, `pricing`, `datacenters`, `businesses`
- **Agnes** (`apihub.agnes-ai.cn` / `api.agnes
