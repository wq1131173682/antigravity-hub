# Changelog

All notable changes to Antigravity Hub are documented here.

This project is a derivative work of
[`lbjlaq/Antigravity-Manager`](https://github.com/lbjlaq/Antigravity-Manager)
(CC BY-NC-SA 4.0) rebuilt as a key-rotation proxy. Versions are tagged `vX.Y.Z`;
pushing a tag runs the Release workflow, which builds Windows and Linux bundles,
signs them, and publishes a GitHub release with `updater.json`.

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
