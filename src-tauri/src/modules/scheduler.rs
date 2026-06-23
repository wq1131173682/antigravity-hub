use tokio::time::{self, Duration};
use tracing::{info, warn};

/// Start the background scheduler for periodic cleanup
pub fn start_scheduler() {
    tauri::async_runtime::spawn(async move {
        info!("Scheduler started. Cleaning expired data every 30 seconds...");

        // Run every 30 seconds
        let mut interval = time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            // 1. Clean expired sliding-window entries + flush dirty state to disk
            let _ = crate::modules::quota_window::clean_expired_windows();

            // 2. Clean expired disabled keys in quota window
            match crate::modules::quota_window::clean_expired_disabled() {
                Ok(reenabled) => {
                    if !reenabled.is_empty() {
                        info!("Re-enabled {} keys after backoff period ended", reenabled.len());
                        
                        // Also re-enable them in the keystore
                        for key_id in &reenabled {
                            let _ = crate::modules::keystore::set_key_status(key_id, false, None, None);
                        }
                    }
                }
                Err(e) => {
                    warn!("Scheduler cleanup failed: {}", e);
                }
            }
        }
    });
}
