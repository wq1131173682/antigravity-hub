//! Emits backend events to the frontend over Tauri's event channel.
//!
//! This module used to be a full log bridge: a `tracing` layer that mirrored
//! every log record into a ring buffer and pushed it to a debug-console UI, with
//! five `#[tauri::command]` wrappers to enable, disable, read and clear it.
//!
//! That whole subsystem is gone. The debug-console frontend was deleted when
//! this fork dropped the upstream account-management UI, which left the bridge
//! unreachable: `LOG_BRIDGE_ENABLED` was only ever set by `enable_log_bridge()`,
//! whose only caller was the deleted `enable_debug_console` command. The flag
//! therefore stayed `false` forever, and `TauriLogBridgeLayer::on_event` took
//! its early-return on line 1 of every log record - the layer was already a
//! no-op at runtime, just with the cost of building the `tracing` layer stack.
//!
//! Logging itself is unaffected: `logger.rs` still installs the console and
//! file layers, so logs continue to reach the terminal and the log file.
//!
//! What remains is the one thing other modules actually use: the app handle,
//! captured once at startup, so `emit_key_switched` can tell the dashboard that
//! the proxy rotated away from a rate-limited key.

use serde::Serialize;
use std::sync::OnceLock;
use tauri::Emitter;

/// Global app handle for emitting events (set once during setup).
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Initialize the bridge with the app handle (call from setup).
pub fn init_log_bridge(app_handle: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app_handle);
    tracing::debug!("[LogBridge] Initialized with app handle");
}

/// Payload for the key-switched event emitted to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeySwitchedPayload {
    pub platform_id: String,
    pub platform_name: String,
    pub model_name: String,
    pub disabled_key_id: String,
    pub next_key_id: String,
    pub reason: String,
    pub disabled_until: i64,
}

/// Emit a key-switched event to the frontend when a key is disabled (429/5xx)
/// and the proxy rotates to the next available key.
pub fn emit_key_switched(payload: KeySwitchedPayload) {
    if let Some(handle) = APP_HANDLE.get() {
        let _ = handle.emit("key-switched", payload);
    }
}
