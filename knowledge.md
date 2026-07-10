# Project knowledge

This file gives Freebuff context about your project: goals, commands, conventions, and gotchas.

## What is this?
**Antigravity Hub** — a lightweight local **API Key rotation proxy** tool. Feed it multiple API keys and it automatically load-balances and fails over between them, exposing a single OpenAI-compatible endpoint.

Forked from [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) v4.2.1, simplified for the "multi-key rotation" use case.

## Key directories
| Path | Purpose |
|------|---------|
| `src/` | Frontend React 19 app |
| `src/components/` | UI components: accounts, common, dashboard, layout, navbar |
| `src/pages/` | Dashboard, Accounts, Settings |
| `src/services/` | Tauri invoke wrappers |
| `src/stores/` | Zustand state stores |
| `src/locales/` | i18n translations (12 languages: ar, en, es, ja, ko, my, pt, ru, tr, vi, zh, zh-TW) |
| `src/types/` | TypeScript types |
| `src/utils/` | Utility functions |
| `src-tauri/` | Rust backend (Tauri v2) |
| `src-tauri/src/commands/` | Tauri IPC commands |
| `src-tauri/src/models/` | Data models (apikey, config, model, platform) |
| `src-tauri/src/modules/` | Core modules: config, proxy, model_manager, etc. |
| `docker/` | Docker deployment configs |
| `docs/` | Documentation |
| `.github/workflows/` | CI / deploy / release workflows |

## Commands

```bash
# Frontend dev server only (browser, no Tauri APIs)
npm run dev

# TypeScript check + Vite build
npm run build

# Full Tauri development (native window + hot reload)
npm run tauri dev

# Build distributable installer (MSI/NSIS/DMG/AppImage)
npm run tauri build
```

### CI checks (run before committing)
```bash
# Frontend
npx tsc --noEmit
npm run build

# Rust (run from src-tauri/)
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check
```

## Tech stack
- **Frontend**: React 19 + TypeScript + Ant Design 5 + Tailwind CSS (daisyui themes) + Zustand + i18next + Recharts + Vite 7
- **Backend**: Rust + Tauri v2 + Axum (HTTP proxy server, port 8045)
- **Storage**: Per-account JSON files (UUID-keyed), SQLite for token stats/logs
- **Styling**: Tailwind CSS with daisyui (light/dark themes, toggled via `darkMode: 'class'`)

## Architecture data flow
```
External client (Claude Code, OpenCode, OpenRouter, etc.)
  → OpenAI/Anthropic/Gemini API call
  → Local Axum proxy (port 8045)
    → Model router (ID mapping)
    → Dispatcher (round-robin, weighted, quota-aware)
    → Request mapper (protocol translation)
    → Upstream Google/Anthropic API
  → Response mapped back → Client
```

## Conventions
- **TypeScript**: strict mode, `noUnusedLocals`, `noUnusedParameters` — no dead code allowed
- **Rust**: `cargo fmt` + `clippy -D warnings` enforced in CI
- **npm install**: always use `--legacy-peer-deps` (avoid React 19 peer dep conflicts)
- **i18n**: all UI text goes through `react-i18next` `t()`; add keys to all 12 locale files
- **Dark mode**: Tailwind `class` strategy + daisyui `dark` theme; toggle with `darkMode` class on `<html>`

## Notable gotchas
- **Windows**: requires VS 2022 Build Tools with VC++ workload for Tauri native compilation
- **Linux**: requires `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`, and other system deps (see CI workflow)
- **Rust min**: 1.75+ (Tauri v2 requirement)
- **Window close**: hides to tray instead of quitting (handled in `lib.rs` RunEvent)
- **Vite proxy**: `/api/` → `http://127.0.0.1:8045` for dev
- **Tauri v2**: uses `@tauri-apps/api` v2 plugins (opener, dialog, fs, process, updater)
- **Port 8045**: default proxy service port (configurable in settings)
- **Account data**: each account stored as a separate JSON file; index file aggregates them
- **Quota window tracking**: per-model+per-key rate limit tracking with 429/500 error recording
