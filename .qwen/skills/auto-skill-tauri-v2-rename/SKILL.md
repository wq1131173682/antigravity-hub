---
name: tauri-v2-rename
description: Rename a Tauri v2 + React application across all required config, source, and i18n files
source: auto-skill
extracted_at: '2026-06-15T07:30:00.000Z'
---

# Rename a Tauri v2 + React Application

When renaming a Tauri v2 desktop app, the name appears in **many** places that must all be updated together. Missing even one causes inconsistent branding in the UI, system tray, installer, or window title.

## Files to Update

### 1. Tauri config (critical)
- **`src-tauri/tauri.conf.json`**
  - `"productName"` — used for the installed app name, window title, bundle name
  - `"windows[].title"` — the default window title shown in the OS title bar (if decorations are enabled) and in taskbar tooltips

### 2. Rust crate (if renaming the binary)
- **`src-tauri/Cargo.toml`** — `name = "..."` field (affects the compiled binary name)

### 3. Frontend HTML
- **`index.html`** — `<title>...</title>` (shown in web/dev mode)

### 4. Custom TitleBar component
- The custom title bar renders the app name as hardcoded text. Find it (usually in `src/components/layout/TitleBar.tsx`) and update the visible string.

### 5. Navigation logo
- The nav/logo component (e.g. `src/components/navbar/NavLogo.tsx`) often shows the app name with an i18n fallback like `t('common.app_name', 'Old Name')`. Update the fallback string.

### 6. Package metadata (optional but recommended)
- **`package.json`** — `"name"` field (affects npm/node identity)

### 7. Internationalization (i18n) files
- Search **all** locale JSON files in `src/locales/` for the old name string and replace globally. Common occurrences:
  - `auto_launch_desc` descriptions
  - About page titles
  - Brew command examples
  - Any user-facing text mentioning the app

## Procedure

```bash
# 1. Find all occurrences first
grep -r "Old App Name" src/ src-tauri/ index.html package.json

# 2. Update tauri.conf.json (productName + window title)
# 3. Update index.html <title>
# 4. Update TitleBar visible text
# 5. Update NavLogo fallback
# 6. Update package.json name
# 7. Update all i18n locale files (use replace_all)
```

## Gotchas

- **i18n files are easy to miss** — they contain the app name in descriptive sentences, not just as a standalone label. Always grep.
- **`identifier` in tauri.conf.json** (e.g. `com.vendor.app-name`) should generally NOT change — it's a stable reverse-domain identifier used by the OS for data persistence and updates.
- **README.md** — update separately if desired; it's documentation, not runtime code.
- After renaming, run `npx tsc --noEmit` to verify no broken references.
