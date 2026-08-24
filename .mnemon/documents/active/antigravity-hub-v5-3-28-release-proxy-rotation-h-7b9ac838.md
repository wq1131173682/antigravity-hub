---
id: "7b9ac838-cdba-484c-aee7-15455f96ef49"
title: "Antigravity Hub — Release Engineering Handoff (v5.3.28–v5.3.32)"
description: "Release engineering: compatible key rotation, Release workflow fixes (artifact pattern, shell:bash), event-driven check-for-updates, install button, version display fix, /models pass-through behavior."
status: "active"
created_at: "2026-08-21T09:11:49.288Z"
updated_at: "2026-08-24T07:42:21.228Z"
content_hash: "812c19909e7d9c20fbd62c6cf53e377ca26648945deaaa11e710d1d498ee9b72"
source_paths:
  - ".github/workflows/release.yml"
  - "scripts/sign_updater.py"
  - "src-tauri/src/modules/proxy.rs"
  - "src-tauri/src/commands/mod.rs"
  - "src-tauri/src/lib.rs"
  - "src/pages/Settings.tsx"
  - "src/services/platformService.ts"
  - "src/locales"
  - "src-tauri/tauri.conf.json"
  - "src-tauri/Cargo.toml"
  - "package.json"
  - "updater.json"
session_ids:
  - "ca2969f8-2e05-4b7e-bfcd-29649103fcd3"
  - "14d27755-33d7-45f3-adf8-f68b8b94e10e"
  - "e4f35a12-8ed5-45cf-8b58-4fb294211c29"
memory_body_ids:
  []
---

# Antigravity Hub — Release Engineering Handoff (v5.3.28–v5.3.32)

Companion to the project knowledge base. Records the v5.3.28–v5.3.32 release engineering work: compatible key rotation, Release workflow artifacts/shell fixes, the event-driven auto-update flow, the Settings install button, the About-version display fix, and the proxy `/models` pass-through behavior.

## Releases summary

| Version | Date | What |
|---|---|---|
| v5.3.28 | 2026-08-21 | Compatible key rotation; Release workflow artifact pattern fix (`bundle-*-r<run_id>`); updater.json signing pipeline green |
| v5.3.29 | 2026-08-24 | Real event-driven check-for-updates; `shell: bash` on Windows runners; i18n for all 12 locales |
| v5.3.30 | 2026-08-24 | About version now from backend `get_app_version()` (was stale package.json); package.json synced |
| v5.3.31 | 2026-08-24 | Install button in Settings that triggers native updater dialog; RID emitted in check_result |
| v5.3.32 | 2026-08-24 | Platform-level `/v1/models` intercept only; `/models` (no v1) passes through to upstream |

## v5.3.29 – Auto-Update Fix

**Root cause of "app is 5.3.26 but won't update"**: two stacked defects.

1. **Backend stub**: `check_for_updates()` in `commands/mod.rs` only returned `{"status": "checking"}` — it never invoked `app.updater().check()`. The real check ran only at startup in `lib.rs setup()` inside `tauri::async_runtime::spawn`; network/signature errors were logged and swallowed — the user never saw the check failed.
2. **Frontend fake success**: `Settings.tsx` treated the stub as success and unconditionally showed "检测完成" after a 2s `setTimeout`.

**Fix (event-driven, keeps working in v5.3.31/32)**:
- `check_for_updates(app: tauri::AppHandle)` calls `app.updater().check()` in an async spawn and emits `update:check_result` with `status: available | up_to_date | error`, payload `version` / `current_version` / `message` / `rid`.
- The `check_result` listener stores the update RID (`updateRid` state) for the install step; the update object is kept in the app resource table via `app.resources_table().add(update)` before emitting.
- `Settings.tsx` subscribes via `listen('update:check_result', ...)` (`@tauri-apps/api/event`), renders new-version / up-to-date / error, resets spinner on event, null-safe with `?? '?'`, cleanup via `unlistenRef`.
- `src/services/platformService.ts` `checkForUpdates(): Promise<void>` (result arrives via event).
- i18n `settings.update.*` completed in **all 12 locale files**; `new_version` and `current` keys added to zh.json. Note: writing locale files with PowerShell `Set-Content` corrupted non-ASCII glyphs (`→` → `鈫?`) — validate JSON Unicode after any locale batch edit.

**CI failure fixed on the fly**: first v5.3.29 Release failed on Windows because Actions Windows runners default `run:` to PowerShell, and bash `if [ ]` threw `ParserError`. Fix: add `shell: bash` to every bash-syntax step in the matrix `build` job (verify-tag, verify-bundle-version; `updater` job runs on Ubuntu so unaffected). **Lesson**: any `run:` step with bash syntax (`if`, `[[ ]]`, `find | while read`, heredoc `<<'PY'`) in the matrix build job MUST declare `shell: bash`.

## v5.3.30 – About-version display

**Root cause**: Settings About imported `version` from `package.json`, which lagged behind Cargo.toml/tauri.conf.json (stuck at 5.3.26), so the About page showed the wrong version even on the latest build.
**Fix**: `Settings.tsx` now calls backend `get_app_version()` (reads `CARGO_PKG_VERSION`) into `appVersion` state; falls back to `?` on failure. Removed the `package.json` version import. `package.json` bumped to match (does not affect Tauri bundle naming).

## v5.3.31 – Install button (native dialog)

**Problem**: update check only notified; clicking did nothing because `dialog: true` in tauri.conf.json only auto-shows the native dialog for the STARTUP check, not the manual check.
**Fix**: when `check_result` says `available`, Settings shows a green install button; `handleInstallUpdate` calls the updater plugin's built-in command `download_and_install` via `tauriInvoke('download_and_install', { rid: updateRid, onEvent: channel })` (Channel from `@tauri-apps/api/core`). The plugin then shows its native download/install dialog. Rust side emits the RID so the frontend can install. (The `Update` struct's own `download_and_install` method exists but has no dialog; the IPC command with `dialog` path is the right one.)

## v5.3.32 – /models pass-through

**Story**: an earlier change made the platform-level interceptor catch both `/v1/models` AND `/models`, which blocked clients (e.g. DSH configuring a custom provider like `acme-gateway` with base URL `<ip>:5343/openrouter/v1`) from discovering upstream models — the interceptor returned the local (possibly empty) model list instead.
**Final behavior (a08d92e reverted by 033f645)**:
- `handle_models_request(Some(platform_prefix))` fires ONLY for `/v1/models` (and trailing slash) at platform level → returns locally configured models (OpenAI `{object:list, data:[{id,...}]}` format), global list cached with `MODELS_CACHE` + TTL.
- `/models` (no v1) at platform level passes through to upstream → clients like DSH see the real upstream model list.
- Global (no-prefix) `/v1/models` and `/models` both return ALL platforms' local models.

## Compatible Key Rotation (proxy.rs, v5.3.28)

On 429/5xx, retry the SAME key N times before rotating.
- `current_key_errors` counter in `forward_with_retry` (reset on success/key switch).
- **Rotation threshold**: `current_key_errors >= keys_to_try.len()`.
- **Backoff**: exponential `2^errors` capped 32s + jitter 0–50%. Rationale: immediate rotation "blitzes" keys in ms, triggering account-level rate limits (observed with SenseNova).
- Log lines: `429 from key[0]=… (consecutive errors …)`, `retrying same key in Xs …`, `rotating to next key after N consecutive errors`.
- Benign clippy warnings (`last_error`, `current_key_errors` never read) remain.

## Release Workflow (release.yml)

1. Bump version in BOTH `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (and `updater.json`) BEFORE tagging; tag must match exactly (e.g. `v5.3.32` ↔ `5.3.32`) or the pre-build "Verify tag version matches Cargo.toml" fails fast. Also run `cargo update -p antigravity_tools` to sync Cargo.lock.
2. Artifacts named `bundle-<target>-r<run_id>`; download pattern MUST be `bundle-*-r${{ github.run_id }}` (the `r` is critical).
3. Windows runners: bash-syntax steps need `shell: bash`.
4. Publish: delete existing release (idempotent) → `gh release create <tag> updater.json` → upload only `.msi/.exe/.AppImage/.deb` with `--clobber` (per-file WARN never aborts) → verify updater.json URLs HTTP 200.
5. User preference: no need to watch CI — push commit + tag and let the workflow finish on its own.
6. `package.json` version does not affect bundle naming (may be unsynced, but keep it matching for the About fix).

## updater.json Signing Pipeline

`scripts/sign_updater.py` signs bundles with `TAURI_SIGNING_PRIVATE_KEY` (GitHub secret), writes `updater.json`, `--verify` self-checks; only the Actions updater job (or key holder) can produce valid signatures. Auto-update relies on `releases/latest/download/updater.json`.
