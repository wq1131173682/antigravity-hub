<div align="center">

# Antigravity Hub

**Multi-platform API Key rotation proxy · Desktop app (Tauri v2)**

A lightweight rebuild of [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager), focused on **multi-account management and API key rotation proxying**.

<br/>

<img src="public/icon.png" alt="Logo" width="80" height="80" style="border-radius: 16px;">

<br/><br/>

<img src="https://img.shields.io/badge/Tauri-v2-orange?style=flat-square" alt="Tauri">
<img src="https://img.shields.io/badge/Rust-1.75%2B-blue?style=flat-square" alt="Rust">
<img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square" alt="React">
<img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform">
<img src="https://img.shields.io/badge/license-CC%20BY--NC--SA%204.0-green?style=flat-square" alt="License">

<br/>

[中文](README.md) · [Changelog](CHANGELOG.md) · [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

</div>

---

## What is this

A locally running **API key rotation proxy**. Add API keys from multiple platforms (Sensenova, Agnes, OpenAI-compatible gateways, etc.) and it starts an **OpenAI-compatible proxy** on your machine that automatically handles:

- **Load balancing** — requests are evenly distributed across available keys
- **Failover** — automatically rotates to the next usable key on 429 / 5xx (with exponential backoff)
- **Quota management** — per (model × key) sliding-window tracking for 5-hour / daily / monthly usage, auto-switching when limits are exceeded
- **OpenAI protocol pass-through + key rotation** — OpenAI Chat Completions requests are forwarded upstream as-is, with keys managed and rotated locally by quota/failure policy

One entry point, many keys, smart quota-aware scheduling.

## Features

### 🚀 Core Proxy
- Local OpenAI-compatible proxy (default `127.0.0.1:8045`) for any OpenAI-compatible SDK / client
- Automatic key rotation & load balancing with intelligent 429/5xx backoff (single key supported too)
- ⚠️ **Codex integration & Responses API translation archived in v5.3.10** — code retained, UI greyed/disabled; the proxy now targets OpenAI Chat Completions pass-through
- Broad upstream compatibility: multiple reasoning field names (`reasoning_content` / `thinking` / …), inline thinking blocks (`>think` / `<think>` / `[think]` markers), tool calls, streaming & non-streaming

### 💳 Multi-platform Account Management
- Unlimited platforms and accounts, each with its own `base_url` / `path_prefix` / API key
- Per (model × key) mapping — fine-grained control over which key may call which model
- Upstream model list auto-sync & auto-creation, configurable context window (`max_input_tokens`)

### 📊 Quota & Statistics
- **Sliding-window quota tracking** (5-hour / daily / monthly) per (model × key)
- 429/500 error counting with automatic backoff and expiry recovery
- Global + per-platform **token usage statistics** (persisted across restarts)

### 🤖 Codex CLI Integration (archived · v5.3.10)
> As of v5.3.10, Codex integration and Responses API translation are **archived/disabled** (code retained, Settings greyed out and non-interactive). The following capabilities are unavailable in the current release.
- One-click generation & application of Codex `config.toml` (`model_providers.custom` + API key auth)
- Config backup / one-click restore, clear residual OAuth data
- Environment variable conflict detection (`OPENAI_API_KEY` / `OPENAI_BASE_URL` / …)
- Optional model catalog generation (`model_catalog_json`, Codex Desktop only)

### 🎨 Other
- 12 languages i18n (follows system language)
- System tray: quick account switching, quota refresh
- Auto-update (Tauri updater)

## Quick Start

### Prerequisites

- Node.js >= 18
- Rust >= 1.75
- Tauri v2 system dependencies (see the [official docs](https://v2.tauri.app/start/prerequisites/))

### Development

```bash
git clone git@github.com:wq1131173682/antigravity-hub.git
cd antigravity-hub
npm install

# Development mode (with hot reload)
npm run tauri dev

# Build release bundles
npm run tauri build
```

### Usage

1. Launch the app and add platforms & API keys under "Account Management"
2. Start the local proxy (default port `8045`)
3. Point any OpenAI-compatible client at `http://127.0.0.1:8045/{platform-prefix}/v1`
4. (Optional) Codex CLI integration is archived/disabled in v5.3.10; watch for it in a future release

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | React 19 · TypeScript · Ant Design · Tailwind CSS · daisyUI · Zustand · i18next |
| Backend | Rust · Tauri v2 · Axum (local proxy) |
| Storage | JSON files (`api_keys.json` / `quota_windows.json` / `token_stats.json` / `config.json` etc., in the OS data directory) |
| Proxy protocol | OpenAI Chat Completions (pass-through) / Responses API (archived in v5.3.10) |

## Project Structure

```
├── src/                  # Frontend (React 19 + TypeScript)
│   ├── components/       # UI components (accounts / dashboard / settings / layout)
│   ├── pages/            # Top-level pages (Accounts / Dashboard / Settings)
│   ├── services/         # Backend IPC wrappers
│   ├── stores/           # Zustand stores (accounts / config / platforms)
│   ├── locales/          # 12 languages
│   └── utils/            # Utilities
├── src-tauri/            # Backend (Rust + Tauri v2)
│   ├── src/commands/     # Tauri IPC commands
│   ├── src/modules/      # Core logic (proxy / codex / quota / keystore)
│   └── src/models/       # Data models
└── .github/workflows/    # CI / release pipelines
```

## Changelog

<!-- Note: keep the version entries in the "- **vX.Y.Z** title + indented items" format to stay consistent with release.yml note extraction -->

- **v5.3.10** Key release · stability fixes (⭐ milestone)
  - Archived Codex integration & Responses API translation (`CODEX_ENABLED=false`; code retained, UI greyed/disabled)
  - OpenAI protocol pass-through with local key rotation (429/5xx auto-failover to next key)
  - Fix: proxied chat/tool-call drops (Connection: close after SSE), mid-frame idle cutoff (keepalive at event boundary only), 429 rate-limit exhaustion (cooldown backoff retry)
- **v5.2.11** Compatibility enhancements & fixes
  - Compatibility: broad cross-platform Responses API translation (`reasoning` → `reasoning_effort`, `tool_choice` conversion, thinking markers `>think`/`<think>`/`[think]`, more reasoning field names)
  - Fix: empty upstream streams now surface as errors instead of silent empty completions; only 2xx counts toward quota (fixes 4xx inflating usage and dropping keys); model rewriting scoped to Responses API only
  - Fix: Codex `model_catalog_json` is now opt-in (off by default to avoid CLI init failure)
- **v5.2.10** Codex model rewrite + persistent per-platform token stats
- **v5.2.9** Codex config merge, upstream request debug log, multi-reasoning-field & top-level chunk reasoning support
- **v5.2.3** Codex CLI integration optimizations (7 issues fixed)
- **v5.2.0** Upstream model sync with multi-URL fallback & auto-creation, model testing
- Full history in [CHANGELOG.md](CHANGELOG.md)

## Acknowledgements

Thanks to [lbjlaq](https://github.com/lbjlaq) for the original [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager). This project is a targeted rebuild and simplification of it. For the full-featured version, please also check out the original project.

## License

This project is distributed under **CC BY-NC-SA 4.0** ([Creative Commons Attribution-NonCommercial-ShareAlike 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/)), inherited from the original project.

> ⚠️ **Note**: This is a **non-commercial** license that requires **share-alike**. Please ensure compliance (or obtain authorization) before commercial use.
