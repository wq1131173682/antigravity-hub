---
id: "7b9ac838-cdba-484c-aee7-15455f96ef49"
title: "Antigravity Hub — Release & Proxy Rotation Handoff (v5.3.28 & v5.3.29)"
description: "v5.3.28–v5.3.29 release engineering: compatible key rotation, Release workflow fixes (artifact pattern, shell:bash on Windows runners), real event-driven check-for-updates fix, updater.json signing pipeline."
status: "active"
created_at: "2026-08-21T09:11:49.288Z"
updated_at: "2026-08-24T06:07:26.279Z"
content_hash: "88e70a537f07c08d7fafc8f12a96d305a4ca8b43e35cd9e1bbb35f8dec3211b6"
source_paths:
  - ".github/workflows/release.yml"
  - "scripts/sign_updater.py"
  - "src-tauri/src/modules/proxy.rs"
  - "src-tauri/src/modules/quota_window.rs"
  - "src-tauri/src/commands/mod.rs"
  - "src-tauri/src/lib.rs"
  - "src/pages/Settings.tsx"
  - "src/services/platformService.ts"
  - "src/locales/zh.json"
  - "src-tauri/tauri.conf.json"
  - "src-tauri/Cargo.toml"
session_ids:
  - "ca2969f8-2e05-4b7e-bfcd-29649103fcd3"
  - "14d27755-33d7-45f3-adf8-f68b8b94e10e"
memory_body_ids:
  []
---

# Antigravity Hub — Release & Proxy Rotation Handoff (v5.3.28 & v5.3.29)

Companion to the project knowledge base (covers v5.3.27 and earlier). This document records the v5.3.28 release engineering work (compatible key rotation, Release workflow artifact fix, updater.json signing pipeline) and the v5.3.29 auto-update fix that made the Settings check button actually work.

## v5.3.28 Release (2026-08-21)

Published successfully with **correct updater.json** (fresh signatures, URLs point to `Antigravity.Hub_5.3.28_*` assets). The updater.json signature problem from v5.3.27 was finally resolved — root causes and fixes below.

## v5.3.29 Release — Auto-Update Fix (2026-08-24)

**Root cause of "app is 5.3.26 but won't update"**: two defects stacked up.

1. **Backend stub**: `check_for_updates()` in `commands/mod.rs` only returned `{"status": "checking"}` — it never invoked `app.updater().check()`. The real check ran only once at startup in `lib.rs setup()` inside `tauri::async_runtime::spawn`, and any network/signature error was logged and silently swallowed — the user never saw that the check had failed.
2. **Frontend fake success**: `Settings.tsx` treated the stub reply as success and unconditionally showed "检测完成" after a 2s `setTimeout`, so the button *looked* functional while doing nothing.

**Fix (v5.3.29)**:
- `check_for_updates(app: tauri::AppHandle)` now calls `app.updater().check()` in an async spawn and **emits** a `update:check_result` event with `status: available | up_to_date | error` (payload carries `version` / `current_version` / `message`). Uses `tauri_plugin_updater::UpdaterExt` and `tauri::Emitter`.
- `Settings.tsx` subscribes with `listen('update:check_result', ...)` from `@tauri-apps/api/event`, renders the real state (new-version line, up-to-date, or error), and resets the button spinner on the event. Version fields are null-safe with `?? '?'`. The listener is cleaned up via an `unlistenRef` on unmount.
- `src/services/platformService.ts` `checkForUpdates()` return type changed to `Promise<void>` (result arrives via event).
- i18n: `settings.update.*` (title/check/checking/check_completed/check_failed/new_version/current/auto_desc) completed in **all 12 locale files** (was zh.json only in v5.3.28); `new_version` and `current` keys added to zh.json.

**Second CI failure fixed on the fly**: the first v5.3.29 Release run failed on `build (windows-latest, ...)` because GitHub Actions Windows runners default `run:` steps to PowerShell — the bash-style `if [ ... ]; then` in "Verify tag version matches Cargo.toml" threw `ParserError: Missing '(' after 'if'`. Fix: added `shell: bash` to every bash-syntax step in the `build` job (verify-tag and verify-bundle-version steps; `updater` job runs on Ubuntu so it was unaffected). Second run was fully green: linux build + windows build + updater jobs all passed, release published 2026-08-24T06:01:20Z, tag `v5.3.29`.

**Lesson for future releases**: any `run:` step containing bash syntax (`if [ ]`, `[[ ]]`, `find ... | while read`, heredocs) in the matrix `build` job MUST declare `shell: bash`, or it will fail only on Windows runners.

## Compatible Key Rotation (proxy.rs)

**Change**: on 429/5xx, the proxy no longer immediately rotates to the next key. It now retries the *same* key N times before rotating.

- Added `current_key_errors` counter in `forward_with_retry` (reset on success or on key switch).
- **Rotation threshold**: `current_key_errors >= number_of_available_keys` (`keys_to_try.len()`).
- **Backoff**: exponential `2^errors` seconds (cap 32s) + random jitter 0–50% of base (`rand::random::<u64>() % base`).
- Rationale: immediate rotation "blitzes" every key in milliseconds, which is exactly what triggers account-level rate limits (observed with SenseNova). Waiting out the same key's transient cooldown increases success and avoids 502 "All keys exhausted".
- Log lines: `429 from key[0]=… (consecutive errors on this key: N)`, `retrying same key in Xs (error #N, attempt M)`, `rotating to next key after N consecutive errors`.
- Unrelated warning (benign): `current_key_errors` / `last_error` "never read" clippy warnings remain.

## Release Workflow Artifact Fix (.github/workflows/release.yml)

The recurring "updater.json points at wrong version / no bundles found" failures had these root causes and fixes:

1. **Artifact name pattern mismatch (THE bug)**: upload names artifacts `bundle-<target>-r<run_id>` (with `r` prefix, added in commit 7ccbe59 to prevent stale-artifact reuse), but the download `pattern` was `bundle-*-<run_id>` (missing `r`), matching nothing → empty `bundles/` → `sign_updater.py` failed with "ERROR: no bundles found under bundles". Fix: pattern is now `bundle-*-r${{ github.run_id }}`.
2. **Never silently continue past a failed download**: the filtered download has `continue-on-error: true` (fallback download logic was removed as it made the job fail even when the fallback succeeded). A new "Verify artifacts were downloaded" step exits 1 with a clear message if `bundles/` is empty.
3. **Version verification step**: after download, every `.msi/.exe/.AppImage/.deb` filename must contain the tag version (`${GITHUB_REF_NAME#v}`); otherwise exit 1. This catches stale artifacts before signing.
4. **Tauri version sources**: the built artifact filename uses the version from **`src-tauri/Cargo.toml`** (and `tauri.conf.json`) — NOT the git tag. A `v5.3.28` tag with Cargo.toml still at `5.3.27` builds `Antigravity.Hub_5.3.27_*` files, which the version check correctly rejects. **Both `tauri.conf.json` and `Cargo.toml` must be bumped before tagging.** (This is why earlier v5.3.27 attempts produced 5.3.26-named assets that no longer happen with the version check.)
5. `package.json` version is the frontend's and does not affect Tauri bundle naming (it may be left unsynced).
6. **Windows runner shell (v5.3.29)**: bash-syntax `run:` steps in the matrix build job need `shell: bash`; see the v5.3.29 section above.

## updater.json Signing Pipeline

- `scripts/sign_updater.py` discovers bundles under `--bundle-dir`, signs each with `TAURI_SIGNING_PRIVATE_KEY` (GitHub secret), writes `updater.json`; `--verify` self-checks before writing.
- Signing runs in the `updater` job (Ubuntu) after downloading build artifacts; requires the `r<run_id>` artifact names (unique per run).
- Release publish: delete existing release for tag (idempotent) → `gh release create <tag> updater.json` → upload only `.msi/.exe/.AppImage/.deb` with `--clobber` (a failing file logs WARN, never aborts).
- After publish, the workflow verifies all updater.json URLs resolve HTTP 200.
- If signatures are missing/mismatched in an already-published release, regeneration requires the signing key — only the GitHub Actions updater job (or someone with the secret) can produce valid signatures.

## Check for Updates Button (Settings page) — final v5.3.29 behavior

- Backend commands `check_for_updates` and `get_app_version` (in `commands/mod.rs`, registered in `lib.rs`). Pre-v5.3.29 `check_for_updates` was a stub returning `{"status":"checking"}`; from v5.3.29 it really calls `app.updater().check()` and emits `update:check_result` (see v5.3.29 section above).
- Updater plugin config: `tauri.conf.json` `plugins.updater` → endpoints `https://github.com/wq1131173682/antigravity-hub/releases/latest/download/updater.json`, `dialog: true` (native install prompt appears automatically when a newer version is found), minisign `pubkey`.
- Frontend: `src/services/platformService.ts` exports `checkForUpdates()` (→ `Promise<void>`); `src/pages/Settings.tsx` shows the update card with check button, listens for `update:check_result`, and uses i18n keys `settings.update.*` (present in all 12 locale files).
- Startup auto-check already exists in `lib.rs setup()` via `handle.updater().check()`.

## Operational Notes

- The auto-update flow relies on `https://github.com/wq1131173682/antigravity-hub/releases/latest/download/updater.json` (tauri.conf.json `plugins.updater.endpoints`). Keep that release "latest" so `releases/latest` resolves.
- After this fix lands in a release, users on older builds (e.g. 5.3.26) can genuinely trigger the updater dialog from Settings → 检测更新, which checks for and offer-installs the newest version.
- Local git pushes to GitHub on this machine require the proxy `http://127.0.0.1:7890` (direct connection frequently times out). Set with `git config --global http.proxy http://127.0.0.1:7890` (and https.proxy) before `git push`.
- Testing branch builds: Windows CI build frequently fails on WebView2 download (`Invoke-WebRequest https://github.com/webview/webview2/...`) — a transient GitHub Actions network issue unrelated to code.
