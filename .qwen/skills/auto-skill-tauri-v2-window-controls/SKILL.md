---
name: tauri-v2-window-controls
description: Fix minimize/maximize/close not working in Tauri v2 apps with custom title bar (decorations: false)
source: auto-skill
extracted_at: '2026-06-15T07:30:00.000Z'
---

# Tauri v2 Window Controls with Custom Title Bar

When a Tauri v2 app uses `"decorations": false` in `tauri.conf.json` and implements a custom TitleBar component, the window control buttons (minimize, maximize, close) often silently fail **not because of frontend code bugs, but because of missing Tauri capability permissions**.

## Symptom

- Custom title bar renders correctly with minimize/maximize/close buttons
- Clicking the buttons does nothing — no errors in console, no window state change
- The `@tauri-apps/api/window` API calls (`getCurrentWindow().minimize()`, etc.) silently return without effect

## Root Cause

Tauri v2 uses a **capabilities-based permission system**. Each window API method requires an explicit permission grant in `src-tauri/capabilities/default.json`. The default capability template does NOT include all window control permissions.

## Required Permissions

Add these to `src-tauri/capabilities/default.json` → `permissions` array:

```json
{
  "permissions": [
    "core:default",
    "core:window:default",
    "core:window:allow-start-dragging",
    "core:window:allow-set-background-color",
    "core:window:allow-set-size",
    "core:window:allow-set-always-on-top",
    "core:window:allow-center",
    "core:window:allow-set-decorations",
    "core:window:allow-set-resizable",
    "core:window:allow-set-shadow",
    "core:window:allow-show",
    "core:window:allow-hide",
    "core:window:allow-minimize",
    "core:window:allow-maximize",
    "core:window:allow-unmaximize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-is-maximized",
    "core:window:allow-close",
    "core:window:allow-set-focus",
    "core:window:allow-outer-size",
    "core:window:allow-is-visible"
  ]
}
```

## Common TitleBar Pattern

The standard custom TitleBar component pattern in React:

```tsx
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { Minus, Square, X } from 'lucide-react';

export default function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    win.isMaximized().then(setMaximized);
    const setup = async () => {
      await win.onResized(() => {
        win.isMaximized().then(setMaximized);
      });
    };
    setup();
  }, []);

  const handleMinimize = () => getCurrentWindow().minimize();
  const handleMaximize = async () => {
    const win = getCurrentWindow();
    if (await win.isMaximized()) await win.unmaximize();
    else await win.maximize();
  };
  const handleClose = () => invoke('close_window'); // or getCurrentWindow().close()

  return (
    <div data-tauri-drag-region className="flex items-center justify-between h-9">
      <span>App Name</span>
      <div onMouseDown={e => e.stopPropagation()}>
        <button onClick={handleMinimize}><Minus /></button>
        <button onClick={handleMaximize}><Square /></button>
        <button onClick={handleClose}><X /></button>
      </div>
    </div>
  );
}
```

## Key Points

- **`data-tauri-drag-region`** on the title bar div enables window dragging
- **`onMouseDown={e => e.stopPropagation()}`** on the buttons container prevents drag from intercepting clicks
- **`close_window`** is typically a custom Rust command that calls `window.hide()` (minimize to tray) rather than actual close
- **`onResized` listener** keeps the maximize/restore icon in sync
- The TitleBar should only render in Tauri environment: `{isTauri() && <TitleBar />}`

## Checklist When Window Controls Don't Work

1. ✅ Check `src-tauri/capabilities/default.json` for missing permissions (most common cause)
2. ✅ Verify `onMouseDown` stop propagation is on the buttons container, not the drag region div
3. ✅ Confirm the component only mounts in Tauri environment (not web dev mode where `getCurrentWindow()` would fail)
4. ✅ Check that `transparent: true` in tauri.conf.json isn't causing invisible window (set a background color)
