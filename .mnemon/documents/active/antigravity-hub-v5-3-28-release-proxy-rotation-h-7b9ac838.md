---
id: "7b9ac838-cdba-484c-aee7-15455f96ef49"
title: "Antigravity Hub — v5.3.28 Release & Proxy Rotation Handoff"
description: "v5.3.28 release engineering: compatible key rotation (per-key retry budget + jitter), Release workflow artifact pattern fix, check-for-updates button, and updater.json signing pipeline notes."
status: "active"
created_at: "2026-08-21T09:11:49.288Z"
updated_at: "2026-08-21T09:11:49.288Z"
content_hash: "1e32a2fd7585de64c10a88a4aacd5a4de9d968f5fbd26bed49743ca129d80c63"
source_paths:
  - ".github/workflows/release.yml"
  - "scripts/sign_updater.py"
  - "src-tauri/src/modules/proxy.rs"
  - "src-tauri/src/modules/quota_window.rs"
  - "src-tauri/src/commands/mod.rs"
  - "src-tauri/src/lib.rs"
  - "src/pages/Settings.tsx"
  - "src/services/platformService.ts"
  - "src-tauri/tauri.conf.json"
  - "src-tauri/Cargo.toml"
session_ids:
  - "ca2969f8-2e05-4b7e-bfcd-29649103fcd3"
memory_body_ids:
  []
---

# Antigravity Hub — v5.3.28 Release & Proxy Rotation Handoff

Companion to the project knowledge base (covers v5.3.27 and earlier). This document records the v5.3.28 release engineering work: compatible key rotation, the Release workflow artifact fix, the check-for-updates feature, and the updater.json signing pipeline.

## v5.3.28 Release (2026-08-21)

Published successfully with **correct updater.json** (fresh signatures, URLs point to `Antigravity.Hub_5.3.28_*` assets). The updater.json signature problem from v5.3.27 was finally resolved — root causes and fixes below.

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
5. `package.json` version is the frontend's and does not affect Tauri bundle naming (it was left at 5.3.26; does not need syncing for releases).

## updater.json Signing Pipeline

- `scripts/sign_updater.py` discovers bundles under `--bundle-dir`, signs each with `TAURI_SIGNING_PRIVATE_KEY` (GitHub secret), writes `updater.json`; `--verify` self-checks before writing.
- Signing runs in the `updater` job (Ubuntu) after downloading build artifacts; requires the `r<run_id>` artifact names (unique per run).
- Release publish: delete existing release for tag (idempotent) → `gh release create <tag> updater.json` → upload only `.msi/.exe/.AppImage/.deb` with `--clobber` (a failing file logs WARN, never aborts).
- After publish, the workflow verifies all updater.json URLs resolve HTTP 200.
- If signatures are missing/mismatched in an already-published release, regeneration requires the signing key — only the GitHub Actions updater job (or someone with the secret) can produce valid signatures.

## Check for Updates Button (Settings page)

- Backend Tauri commands added (`commands/mod.rs`, registered in `lib.rs`): `check_for_updates` (returns `{"status":"checking"}`; actual check is the tauri-plugin-updater, configured `dialog: true`, which shows the native update dialog when a new version exists) and `get_app_version` (`env!("CARGO_PKG_VERSION")`).
- Frontend: `src/services/platformService.ts` exports `checkForUpdates()`; `src/pages/Settings.tsx` gained an "更新 / Update" card with a check button, loading/status text, and i18n keys `settings.update.*` (added to `src/locales/zh.json`; other locales not yet updated).
- Startup auto-check already existed in `lib.rs setup()` via `handle.updater().check()`.

## Operational Notes

- The auto-update flow relies on `https://github.com/wq1131173682/antigravity-hub/releases/latest/download/updater.json` (tauri.conf.json `plugins.updater.endpoints`). Keep that release "latest" so `releases/latest` resolves.
- Local git pushes to GitHub on this machine require the proxy `http://127.0.0.1:7890` (direct connection frequently times out). Set with `git config --global http.proxy/http://127.0.0.1:7890` (and https.proxy) before `git push`.
- Testing branch builds: Windows CI build frequently fails on WebView2 download (`Invoke-WebRequest https://github.com/webview/webview2/...`) — a transient GitHub Actions network issue unrelated to code.
