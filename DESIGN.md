# Antigravity Hub — Design System

Evidence-based reference for the app's UI. Every rule below was verified against
the built bundle (`dist/assets/*.css`), not inferred from intent. Where the code
and this document disagree, the code is wrong.

Stack: React 19 + TypeScript, Tailwind CSS 3.4, daisyUI 5.5, lucide-react icons.

---

## 1. Colour

### Canvas

One variable owns the app background:

```css
:root      { --app-canvas: #FAFBFC; }   /* light */
html.dark  { --app-canvas: #1d232a; }   /* dark  */
```

Exposed to Tailwind as `bg-canvas`. It is consumed in exactly three places:
`Layout`, `Navbar`, and the Dashboard sticky bar. `ThemeManager` reads the same
variable via `getComputedStyle` to paint the native Tauri window, so the OS
window and the DOM can never drift apart.

**Never** write the hex inline. The previous build repeated `#FAFBFC` across
`App.css` (×4), `Layout`, `Navbar`, `Dashboard`, and `ThemeManager` — five
independent copies of one value.

> `index.html` still hardcodes `#1a1f2e` as a pre-hydration splash colour. That
> is intentional (it paints before any CSS variable exists) and is the only
> sanctioned exception.

### Semantic status

`src/utils/status.ts` is the single source of truth for quota/health colour.
Status is always derived through `quotaStatus(used, limit, over)`:

| Level | Meaning | Hue | Text | Bar |
|---|---|---|---|---|
| `ok` | < 80% of limit | emerald | `STATUS_TEXT.ok` | `STATUS_BAR_*` |
| `warn` | ≥ 80%, not over | amber | `STATUS_TEXT.warn` | `STATUS_BAR_*` |
| `danger` | over the limit | rose | `STATUS_TEXT.danger` | `STATUS_BAR_*` |

All class strings are complete literals so Tailwind's JIT can see them. Never
build them with `text-${x}` interpolation.

### Accent budget

**One accent per view.** Primary actions are `blue-500`/`blue-600`. Do not
introduce a second accent hue for decoration.

Purple, violet, fuchsia, indigo and multi-hue gradients were removed from the
reachable UI. They carried no meaning — the 5h-quota hero was an
indigo→violet→purple gradient with two blurred glow blobs, which competed with
every neighbouring card and made its own status colour unreadable. Prominence
now comes from scale and typography.

Two hues that remain are deliberate:

- **Gradient quota fills** (`STATUS_BAR_GRADIENT`). A gradient between two steps
  of the *same* hue is a texture, not a colour claim. Use `variant="solid"` when
  the bar sits on a tinted or coloured surface.
- **`green` vs `emerald`.** `green` marks proxy lifecycle state (running dot,
  Live chip, start button). `emerald` marks quota health. Keep them separate —
  do not "tidy" one into the other.

### Theming

daisyUI 5 reads its options from a CSS `@plugin "daisyui" { ... }` block, **not**
from a `daisyui` key in `tailwind.config.js`. A hex theme previously sat in that
config file and was silently ignored for the whole life of the project: the
bundle always shipped daisyUI's stock oklch palette. That dead config has been
removed rather than left as a trap.

`theme` is `'light' | 'dark' | 'system'`. `ThemeManager` resolves `system` via
`prefers-color-scheme` — the app must follow the OS when set to system.

---

## 2. z-index

Fixed scale in `tailwind.config.js`. Arbitrary `z-[...]` values are not allowed.

| Token | Value | Use |
|---|---|---|
| `z-base` | 0 | normal flow |
| `z-raised` | 10 | popovers, dropdown menus |
| `z-sticky` | 20 | sticky toolbars |
| `z-overlay` | 1000 | modals, dialogs |
| `z-toast` | 1100 | toasts, modal drag region |
| `z-debug` | 1200 | debug console |

**The scale starts at 1000 because daisyUI's `.modal` is `z-index: 999`.**
Adding `z-[100]` to a `.modal-open` element wins the same-specificity cascade and
*pulls the dialog down* to 100 — which is what the previous build did to every
confirmation dialog. Anything layered above a modal must exceed 999.

`ModalDialog` and `AccountDetailsDialog` use daisyUI's `.modal` / `.modal-box` /
`.modal-backdrop` structure. Since `.modal-backdrop` is `z-index: -1`, it only
stays behind the box while `.modal` establishes a stacking context — keep that
wrapper intact.

---

## 3. Motion

### Entrance

`.animate-enter` (defined in `App.css`) is the only entrance animation: 150ms
`ease-out`, `opacity` + `scale` only.

It exists because the code previously used `animate-in fade-in zoom-in-95` from
`tailwindcss-animate`, which **is not a dependency of this project**. Those
classes compiled to nothing, so every dropdown and dialog appeared instantly.
Before adding any `animate-*`/`fade-*`/`zoom-*` utility, confirm it exists in
`dist/assets/*.css`.

### Rules

- Animate **compositor properties only**: `transform`, `opacity`.
- **Never** transition `width`, `height`, `top`, `left`, `margin` or `padding`.
  Progress bars scale a full-width fill with `transform: scaleX()` and
  `origin-left` instead of animating `width`. The old `width` transitions
  reflowed on every step, and the token bar re-reflowed every 3s from its poll.
- Interaction feedback ≤ 200ms. The theme-switch View Transition was 500ms and
  read as lag; it is now 240ms.
- Prefer `transition-colors` / `transition-transform` over `transition-all`.
  `transition-all` also animates layout properties you did not intend.
- **No `backdrop-blur` on large or scrolling surfaces.** The sticky Dashboard
  bar blurred a full-width element on every scroll frame for no visual gain.
- No animation without a reason. Decorative glow (`blur-2xl` blobs) is not one.
- Playwright-style looping animations should pause off-screen; `animate-ping`
  is confined to small status dots.

### Reduced motion

`App.css` neutralises `animation-duration` and `transition-duration` under
`prefers-reduced-motion: reduce`, and `.animate-enter` is disabled outright.
`Navbar` additionally skips the View Transition entirely on that preference
(and on Linux, where it can crash). Any new hand-written animation inherits this
guard automatically — do not add JS animations that bypass it.

---

## 4. Typography

- Headings: `text-balance`. Body copy: `text-pretty`.
- All numeric data: `tabular-nums`, so digits do not jitter as counters tick.
- Dense UI: `truncate` or `line-clamp-*`.
- Do **not** alter `tracking-*` — only two existing uppercase micro-labels use
  `tracking-wider` deliberately.
- Font stack is the system UI stack. `Effra` applies only under `[dir="rtl"]`
  (Arabic). Language switching must set `document.documentElement.dir`.

---

## 5. Layout

- Use `h-dvh` / `min-h-dvh`, never `h-screen`. `100vh` overstates the viewport
  in the always-on-top Mini View window.
- `size-*` for square elements instead of `w-* h-*`.
- Card = `rounded-xl border shadow-sm`. Reserve `border-dashed` for empty states.
- Empty states carry **one clear next action**. See `Accounts` (no platforms →
  add platform; no models → add or import; no keys → add key) and the Dashboard
  no-platforms panel.

---

## 6. Accessibility

- **Every icon-only button needs `aria-label`.** `title` alone is unreliable for
  screen readers. This applies to dialog close buttons, row action buttons and
  the copy-address button.
- Interactive rows must be reachable by keyboard. Prefer a real `<button>` —
  the platform tree in `Accounts` is a `<button>` sibling to its row actions,
  not a `<div onClick>` (which cannot be focused or activated) and not a
  `role="button"` wrapper around buttons (invalid: interactive content nested in
  an interactive role). For a leaf row with no inner controls, `role="checkbox"`
  + `tabIndex` + Enter/Space handling is correct.
- Icon-button toolbars reveal on `group-hover`; pairing it with
  `group-focus-within` keeps them visible while tabbing.
- `prefers-reduced-motion` is an accessibility requirement, not a preference.

---

## 7. Upstream lineage and the legacy files

This repo is a **derivative work**, not a git fork. It credits
[`lbjlaq/Antigravity-Manager`](https://github.com/lbjlaq/Antigravity-Manager)
and is licensed **CC BY-NC-SA 4.0** (see `LICENSE`, `README.md`).

It was **not** forked through the GitHub UI: the two histories share **zero
commit SHAs**. Treat the whole tree as an independent codebase — `git merge` or
`cherry-pick` from upstream will not work, and upstream fixes do not arrive
automatically.

| | Upstream | This repo (after cleanup) |
|---|---|---|
| src files | 106 | 35 |
| routes | 9 (`/api-proxy`, `/monitor`, `/token-stats`, `/user-token`, `/apikey-fun`, `/security`, …) | 3 (`/`, `/accounts`, `/settings`) |
| scope | account manager **+ multi-protocol proxy** | key-rotation proxy only |

The local backend was deliberately rewritten as a pure pass-through key rotator
("remove all Codex features", `c392e36`). The frontend dropped 5 route
directories and their supporting components, but left the orphaned files behind.

**Those orphans have now been deleted.** `src/main.tsx` reaches 35 of 35 source
files; `scripts/ui-audit/reachability.mjs` reports zero unreachable modules.
The 21 removed files were:

```
components/accounts/{AccountDetailsDialog,AccountErrorDialog,AccountRow,
                     AddAccountDialog,QuotaItem,accountValidationStatus}.tsx
components/common/{AdminAuthGuard,DebouncedSlider,GroupedSelect,HelpTooltip,Pagination}.tsx
components/debug/{DebugConsole,DebugConsoleButton}.tsx
services/accountService.ts   stores/{useAccountStore,useDebugConsole}.ts
types/{account,quota_window}.ts   utils/{clipboard,cn,uuid}.ts
```

Deleting them removed their Tailwind selectors from the bundle too, because
Tailwind scans every file under `src/` whether or not it is reachable. That alone
took the built CSS from 145 kB to ~99 kB.

`utils/cn.ts` was among the removals: nothing reachable imported it, so `cn()`
is currently unavailable. `clsx` is still a dependency (MiniView imports it
directly); `tailwind-merge` was dropped with it. If conditional class merging is
wanted again, re-add both and restore `src/utils/cn.ts`.

Also removed as unreferenced: `src/assets/react.svg`, `public/vite.svg`,
`public/tauri.svg`, and `public/images/donate/*` (no component rendered donation
content). `public/icon.png` and `public/font/Effra/*` are still used.

### Defects inherited from upstream

These were **not** introduced by the second developer — upstream carries them
too. Do not re-report them as regressions, and do not expect upstream to fix
them:

- The dead `daisyui: { themes: [...] }` block in `tailwind.config.js` (upstream's
  file is byte-identical to this repo's original). daisyUI 5 ignores it.
- `animate-in fade-in zoom-in-95` with no `tailwindcss-animate` dependency, and
  `z-[100]` on `.modal-open` — both still present in upstream
  `components/common/ModalDialog.tsx`.

Fixes made here for the above are local. The license's **ShareAlike** term means
derivative distribution must carry the same license.

### Translation pruning

All 12 locale files were pruned to the keys the app actually asks for: **12,579
leaf entries down to 828** (locales on disk 815 KB → 35 KB). The built JS bundle
fell from **1334 KB to 686 KB** (gzip 419 KB → 201 KB) — dead translations, not
code, were what triggered the "chunk larger than 500 kB" warning.

This was safe because:
- every `t()` call in `src/` uses a **literal** key — no variables, no template
  literals (verified by `unused-i18n.mjs`);
- `modelConfig`'s `i18nKey` / `i18nDescKey` values are treated as a declared
  contract and kept, even though nothing renders them today;
- ancestor objects of kept keys are preserved, so no namespace collapses to `{}`.

Rendering was verified unchanged: all six route/scheme screenshots are
**byte-identical** (SHA-256) before and after the prune.

### Translation coverage

After pruning, the gap that had always existed became measurable and was closed.
Coverage of the 137 keys the app asks for, per locale:

| locale | before | after |
|---|---|---|
| `zh`, `en` | ~88% | **100%** |
| the other 10 | **~34%** | **100%** |

Before this pass, the ten non-zh/en locales defined only about a third of the
strings the UI used, so most of the interface silently fell back to English for
those languages (`fallbackLng: 'en'`). 928 keys were added across those ten
locales, plus 32 for `zh`/`en` and 20 model-metadata keys.

All 12 locale files are now **structurally identical** — same 149 keys, same
order. `locale-consistency.mjs` reports the divergence if that ever regresses.
Locales on disk went 815 KB → 82 KB while coverage went up, because ~89% of the
original content was dead.

Two remaining notes:

- All 12 locales are **statically imported** in `i18n.ts`, so every language ships
  in the main bundle regardless of which is active. Lazy-loading per locale would
  cut a further ~30 KB for single-language users.
- The translations for the 10 non-zh/en locales were produced during this pass and
  have **not been reviewed by native speakers**. `i18n-worklist.mjs` and
  `i18n-coverage.mjs` list what was added if they need checking.


---

## 8. Verifying UI changes

Because two classes in this codebase silently compiled to nothing, verify rather
than assume.

The repeatable check is to **build, then grep the emitted CSS**:

```bash
npx vite build                      # regenerate dist/assets/*.css
npx tsc --noEmit                    # strict type-check
```

Then confirm a class you rely on actually exists in the bundle:

```bash
grep -o '\.animate-enter' dist/assets/*.css     # present?
grep -o '\.animate-in'    dist/assets/*.css     # should be empty
```

Any `animate-*`, `fade-*` or `zoom-*` utility borrowed from tailwindcss-animate
will come back empty — that package is not a dependency here.

An audit kit for this pass lives in `scripts/ui-audit/`:

| script | purpose |
|---|---|
| `reachability.mjs` | module graph via the TypeScript compiler API; lists unreachable files |
| `audit-classes.mjs` | classes used in source but absent from the built CSS |
| `verify-tokens.mjs` | design tokens present / removed; rejects arbitrary `z-[...]` |
| `unused-deps.mjs` | declared deps no source imports |
| `deps-safe-removal.mjs` | the careful version: also closes over what kept packages load at runtime |
| `unused-i18n.mjs` | locale keys no source references (review list) |
| `prune-i18n.mjs` | removes them, in lockstep across all 12 locales (`--write` to apply) |
| `locale-bloat.mjs` | dead-key share per namespace |
| `locale-consistency.mjs` | whether the 12 locales share a key structure |
| `i18n-coverage.mjs` | per-locale coverage of the keys the app asks for |
| `rust-command-usage.mjs` | which Rust IPC commands only dead frontend called |
| `shoot.ps1` | headless Edge screenshots of all routes, light + dark |

Run them with `node scripts/ui-audit/<name>.mjs` from the repo root.
Note that `scripts/` is gitignored by this repo's "non-core auxiliary" policy, so
those files are **local dev helpers, not committed** — the grep steps above are
the committed, reproducible equivalent. `scripts/ui-audit/AUDIT.md` holds the
full findings report this pass was based on.

`audit-classes.mjs` reports four known false positives — `spring` (a
framer-motion `type`), `success`/`warning` (return values of `getQuotaColor`),
and `debug_console.scroll_to_bottom` (an i18n key). Anything else it flags is a
genuine dead class.

### Adding a UI string

Add the key to **all 12** locale files in `src/locales/`, then run
`i18n-coverage.mjs` to confirm coverage. A key present in only some locales
silently falls back to English (`fallbackLng: 'en'`). Several call sites pass a
literal fallback (`t('k', 'default text')`), which keeps the UI readable but
hides the gap — it is still a gap.

