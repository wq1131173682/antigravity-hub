---
name: tauri-v2-tray-menu
description: Optimize system tray context menu in Tauri v2 apps — i18n, separators, click-to-show, event emission to frontend
source: auto-skill
extracted_at: '2026-06-15T08:58:46.212Z'
---

# Tauri v2 System Tray Menu Optimization

When a Tauri v2 app has a bare-bones system tray (only "Show" + "Quit", hardcoded English, no click-to-show), this pattern upgrades it to a fully-featured, i18n-aware tray with event-driven frontend integration.

## Prerequisites

Ensure `Cargo.toml` has the tray feature and `capabilities/default.json` has the tray permission:

```toml
# Cargo.toml
tauri = { version = "^2.2.5", features = ["tray-icon"] }
```

```json
// capabilities/default.json → permissions array
"core:tray:default"
```

## Key Imports

```rust
use tauri::{Emitter, Manager};  // Emitter trait is REQUIRED for app.emit()
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
```

**Gotcha**: `app.emit("event", payload)` fails to compile with "no method named `emit` found for reference `&AppHandle`" unless `use tauri::Emitter;` is in scope. The compiler hint says exactly this — always add the `Emitter` import.

## Full Pattern

```rust
.setup(|app| {
    // 1. Detect system language (cross-platform)
    let sys_lang = {
        #[cfg(target_os = "windows")]
        {
            std::env::var("VSLang")
                .or_else(|_| std::env::var("LANG"))
                .unwrap_or_else(|_| "zh-CN".to_string())
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("LANG").unwrap_or_else(|_| "zh_CN.UTF-8".to_string())
        }
    };
    let texts = your_i18n_module::get_tray_texts(&sys_lang);

    // 2. Build menu items with separators for visual grouping
    let show = MenuItem::with_id(app, "show", &texts.show_window, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let action1 = MenuItem::with_id(app, "action1", &texts.some_action, true, None::<&str>)?;
    let action2 = MenuItem::with_id(app, "action2", &texts.another_action, true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", &texts.quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &sep1, &action1, &action2, &sep2, &quit])?;

    // 3. Create tray with icon, tooltip, and event handlers
    let tray = tauri::tray::TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .icon_as_template(true)   // macOS: auto-adapt to dark/light menu bar
        .tooltip("App Name")
        .menu(&menu)
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "action1" => {
                    // Emit event to frontend — decouples tray from business logic
                    let _ = app.emit("tray:action1", ());
                }
                "action2" => {
                    let _ = app.emit("tray:action2", ());
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click tray icon to show/focus window
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;
    app.manage(tray);  // Prevent TrayIcon from being dropped
    Ok(())
})
```

## i18n Language Detection

`VSLang` is a Windows environment variable set by Visual Studio / VS Code containing a locale string. On Linux/macOS, `LANG` is standard. Fallback to a sensible default for the target audience.

The i18n module should use `starts_with` prefix matching (not exact match) to handle variants like `en-US`, `zh-TW`, `zh_Hant`:

```rust
fn load_translations(lang: &str) -> HashMap<String, String> {
    let normalized = lang.to_lowercase();
    let json = if normalized.starts_with("en") {
        include_str!("locales/en.json")
    } else if normalized.starts_with("zh-tw") || normalized.starts_with("zh_hant") {
        include_str!("locales/zh-TW.json")
    } else {
        include_str!("locales/zh.json")
    };
    // ... parse tray section from JSON
}
```

## Event-Based Frontend Integration

Tray menu actions should **not** directly invoke complex Rust business logic. Instead, emit events that the frontend listens to:

```typescript
// Frontend (React)
import { listen } from '@tauri-apps/api/event';

useEffect(() => {
  const unlisten1 = listen('tray:action1', () => {
    // Handle action1 — e.g., switch account
  });
  const unlisten2 = listen('tray:action2', () => {
    // Handle action2 — e.g., refresh quota
  });
  return () => { unlisten1.then(fn => fn()); unlisten2.then(fn => fn()); };
}, []);
```

## Menu Structure Best Practices

```
┌──────────────────────────────┐
│  Show Main Window             │
├──────────────────────────────┤  ← PredefinedMenuItem::separator
│  Action 1                     │
│  Action 2                     │
├──────────────────────────────┤  ← PredefinedMenuItem::separator
│  Quit                         │
└──────────────────────────────┘
```

- Group "Show" alone at top (most frequent action)
- Group functional actions in the middle
- Group "Quit" alone at bottom (destructive, separated to prevent misclick)
- Use `PredefinedMenuItem::separator(app)?` for native-looking dividers

## Checklist

1. ✅ `Emitter` trait imported (`use tauri::{Emitter, Manager}`)
2. ✅ `tray-icon` feature in `Cargo.toml`
3. ✅ `core:tray:default` permission in capabilities
4. ✅ `app.manage(tray)` to prevent TrayIcon from being dropped
5. ✅ `icon_as_template(true)` for macOS dark/light mode compatibility
6. ✅ `on_tray_icon_event` for click-to-show (separate from `on_menu_event`)
7. ✅ Events emitted to frontend, not direct Rust logic calls from tray
8. ✅ i18n uses prefix matching, not exact locale string match
