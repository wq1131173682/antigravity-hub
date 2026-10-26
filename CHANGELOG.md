# Changelog

All notable changes to Antigravity Hub are documented here.

This project is a derivative work of
[`lbjlaq/Antigravity-Manager`](https://github.com/lbjlaq/Antigravity-Manager)
(CC BY-NC-SA 4.0) rebuilt as a key-rotation proxy. Versions are tagged `vX.Y.Z`;
pushing a tag runs the Release workflow, which builds Windows and Linux bundles,
signs them, and publishes a GitHub release with `updater.json`.

## [5.3.42] - 2026-10-26

One URL for every model of every platform, plus the consistency fixes that
change required.

### Added

- **Proxy**: a **unified entry** at `/v1/...`. Its platform is not taken from the
  path but from the request body's `model`, which is namespaced
  `"<platform_prefix>:<model_name>"`. `GET /v1/models` is now prefix-agnostic and
  lists every platform's models, so a single base URL reaches all of them.
- **Proxy**: `GET /v1/v1/models` is also answered (some clients, DSH among them,
  double the version prefix).
- **UI**: the dashboard shows the unified entry URL next to the per-platform
  paths, and each model card shows the exact `<prefix>:<model>` id to send.

### Changed

- **Proxy**: `/v1/models` ids are `"<platform_prefix>:<model_name>"` instead of
  the bare model name. Two platforms offering the same model name are now
  distinguishable. The colon is deliberate — model names routinely contain
  hyphens (`deepseek-v4-flash`), so a hyphen separator could not be split
  reliably.
- **Proxy**: each `/v1/models` entry keeps the raw name in a new `model` field,
  and `owned_by` now carries the platform prefix instead of the platform UUID
  (which no client could use).
- **Proxy**: `model` in a forwarded body has its namespace stripped, so upstreams
  only ever see the bare id they know.
- **Platform**: `path_prefix` is validated. Version-like segments (`v1`, `v2`,
  `v1beta`, ...) are refused because `/<version>/...` is the unified API root;
  `/`, `:`, whitespace and duplicates are refused too. The frontend input and
  its error message follow the same rule.
- **Proxy**: a single proxied request now reads `gui_config.json` once instead of
  up to three times (platform lookup, namespace resolution, base-URL overrides
  each used to load it independently).

### Fixed

- **Proxy**: the `/v1/models` cache was not invalidated when the advertised set
  changed. Adding, renaming, deleting or importing a platform/model left clients
  (and the new namespaced ids) stale for up to the 60s TTL. All mutation paths
  now invalidate it.
- **Proxy**: the "ambiguous bare model name" error now names the concrete ids to
  send (`a:model` or `b:model`) instead of a template.
- **Config**: the default proxy port is now `5343` (was `8045`), consistently
  across `models/config.rs`, `proxy.rs`, `usePlatformStore` and `Settings`. This
  is the default only — an existing install keeps whatever `proxy_port` is
  already persisted in `gui_config.json`.
- **Dev**: removed the dead `/api/` proxy from `vite.config.ts`. Nothing in
  `src/` ever fetched `/api/` (all backend calls are Tauri IPC, and the Rust side
  exposes only the `/*path` LLM proxy), so the entry forwarded to a port that had
  already drifted out of sync with the real one.

### Backwards compatibility

`/{platform_prefix}/v1/...` and `/{platform_prefix}/v1/models` are unchanged.
A namespaced id on a prefixed path is accepted when it agrees with the prefix and
rejected with `400` when it contradicts it — it is never silently rerouted into
another platform's key pool.

### Tests

23 new backend tests. Beyond unit coverage of the helpers, several drive the real
`proxy_handler`, and one stands up the production router on a real listener and
exercises it over HTTP, so routing is verified end to end rather than by
inspection. Suite: 58 passing.

## [5.3.41] - 2026-09-23

Correct error semantics and retry behaviour when the upstream rate-limits every
key at once, plus consistency fixes found while investigating that path.

### Fixed

- **Proxy**: a key pool that was merely cooling down from upstream 429s was
  reported as `502 Proxy error: No active API keys available`, which was wrong on
  both counts — the keys existed, and the upstream had answered. The retry loop
  now records the pool's recovery time and the handler returns **`503` with a
  `Retry-After` header** and an OpenAI-compatible JSON error body
  (`error.type = "rate_limit_error"`, `error.code = "upstream_rate_limited"`).
  `502` is now reserved for genuine proxy/transport faults.
- **Proxy**: the retry budget was a hard 60s, so a request could give up while
  the soonest cooldown was still ~30s away. It is now a *floor* that is extended
  to cover a known cooldown expiry, capped at 180s measured from request start
  (anchored to the start, not to "now", so repeated extensions cannot push it
  forward forever).
- **Proxy**: the per-cooldown sleep was truncated by `clamp(1, 30)`, which could
  wake before the key became available and burn an attempt. It now waits out the
  full cooldown plus a 2s grace.
- **Proxy**: a pool excluded for a non-cooldown reason (quota window exceeded) is
  no longer described as having no keys; it names the quota cause and the
  candidate count instead.
- **Proxy**: repaired GBK/UTF-8 double-decoded comments (`—` and `─` had become
  CJK mojibake in `proxy.rs` and `keystore.rs`); unrecoverable Chinese comments
  were rewritten in English.
- **Config**: the default proxy port was `8080` while the docs, the Vite dev
  proxy and the UI placeholder all said `8045`. The default is now `8045`
  consistently (`models/config.rs`, `proxy.rs`, `usePlatformStore`, `Settings`).
  Existing installs are unaffected — the persisted port wins.
- **Frontend**: `utils/request.ts` carried a `COMMAND_MAPPING` table and a
  `fetch()` fallback for a "web mode" that has no backend (the Rust side
  registers a single `/*path` proxy route, so every one of those paths 404s).
  The dead branch is removed; calling IPC outside Tauri now fails loudly.

### Tests

- Added regression coverage for the new failure classification: 503 semantics
  and message wording, the already-expired cooldown clamp, the bounded budget
  extension, and the OpenAI-shaped JSON error body. 35 tests pass.

## [5.3.40] - 2026-09-17

Repository hygiene, dead-code removal, and UI correctness fixes. No behaviour of
the proxy itself changes apart from the two fixes noted below.

### Fixed

- **Proxy**: resolve the target model id once per request instead of on every
  attempt, and refresh quotas for all loaded platforms after model limits are
  updated.
- **UI**: repair style classes that compiled to nothing and therefore had no
  effect:
  - `animate-in fade-in zoom-in-95` never produced CSS (the `tailwindcss-animate`
    plugin is not a dependency) — replaced with a local `animate-enter` keyframe.
  - `dark:bg-red-900/8` and `dark:bg-amber-900/8` used an opacity step that does
    not exist in Tailwind — corrected to `/20`.
  - `MODEL_CONFIG` lookup used a UUID instead of the model name, so per-model
    styling never matched.
- **UI**: modal z-index was lower than daisyUI's `.modal` (999), so the open
  modal was painted underneath. Added an explicit z-index scale and moved
  overlays onto it.
- **Accessibility**: platform tree rows in Accounts are operable by keyboard
  (`role="button"`, `tabIndex`, Enter/Space), icon buttons have `aria-label`s,
  and empty states offer a next action.
- **Settings**: the language selector exposed only Chinese and English; it now
  offers all 12 shipped locales.
- **CI**: the Windows job downloaded a hardcoded WebView2 release asset that
  began returning 404, which made every `master` build fail. The step now
  detects an existing runtime and falls back to the Microsoft Evergreen
  bootstrapper.

### Changed

- Quota bars animate with `transform: scaleX()` instead of `width`, keeping the
  animation off the layout path.
- Removed decorative purple/fuchsia gradients and blurred glow circles from the
  Dashboard in favour of the documented canvas token.
- A global `prefers-reduced-motion` block now disables the app's transitions and
  animations.

### Removed

- Dead code unreachable from the app entry (`src/main.tsx`): 27 source files and
  6 unreferenced static assets, verified with the TypeScript compiler API.
- 11 unused dependencies, 114 orphan command mappings, 5 orphan IPC commands and
  the no-op log bridge.
- 89% of i18n keys that no code path referenced (12,579 → 828 leaves).

### Repository

- `.mnemon/` (agent memory: SQLite database, notes, user preferences) is no
  longer tracked and is gitignored. The files remain on disk.
- `.gitignore` de-duplicated and extended with SQLite sidecar rules.
- `updater.json` is no longer tracked: it is a build product of the Release
  workflow and the committed copy was stale.
- Added `DESIGN.md`, documenting the colour/z-index/motion/typography/layout
  rules, the upstream lineage, and the audit kit used to verify UI changes.

## [5.3.39] - 2026-09-08

- Pure pass-through proxy; Codex integration and Responses API translation fully
  removed (~10k lines). Version bumped to 5.3.39.

## [5.3.38] - 2026-09-07

- Mini View rewritten on the current data model (proxy state + live counters).
- `ar.json` common block restored.

## [5.3.37] - 2026-08-28

- Proxy: normalize the `developer` role to `system` for upstreams that reject it.
