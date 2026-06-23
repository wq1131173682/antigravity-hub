use std::sync::Arc;
use tokio::sync::watch;
use serde::Serialize;
use reqwest::Client;
use std::sync::Mutex;
use once_cell::sync::Lazy;

use tracing::{info, warn, error};

/// Shared HTTP client with 300s timeout for LLM API calls
static SHARED_PROXY_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .no_proxy()
        .build()
        .expect("Failed to create HTTP client")
});

/// Proxy server status
#[derive(Debug, Clone, Serialize)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub host: String,
}

static PROXY_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PROXY_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(8080);
static PROXY_HOST: once_cell::sync::Lazy<std::sync::Mutex<String>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new("127.0.0.1".to_string()));
static SHUTDOWN_TX: Mutex<Option<watch::Sender<bool>>> = Mutex::new(None);

/// Initialize proxy port from config (called on app startup)
pub fn init_proxy_port(port: u16) {
    PROXY_PORT.store(port, std::sync::atomic::Ordering::Relaxed);
}

/// Initialize proxy host from config (called on app startup)
pub fn init_proxy_host(host: String) {
    if let Ok(mut h) = PROXY_HOST.lock() {
        *h = host;
    }
}

/// Set the proxy port at runtime (persisted to config separately)
pub fn set_proxy_port_static(port: u16) {
    info!("PROXY_PORT static set to {}", port);
    PROXY_PORT.store(port, std::sync::atomic::Ordering::Relaxed);
}

/// Set the proxy host at runtime (persisted to config separately)
pub fn set_proxy_host_static(host: String) {
    if let Ok(mut h) = PROXY_HOST.lock() {
        info!("PROXY_HOST static set to {}", host);
        *h = host;
    }
}

/// Start the proxy server using the current PROXY_PORT and PROXY_HOST static values
pub async fn start_proxy() -> Result<(), String> {
    let port = PROXY_PORT.load(std::sync::atomic::Ordering::Relaxed);
    let host = PROXY_HOST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    info!("start_proxy called, host={}, port={}", host, port);

    if PROXY_RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Proxy is already running".to_string());
    }
    info!("start_proxy: not already running, proceeding...");

    let (tx, rx) = watch::channel(false);
    {
        info!("start_proxy: acquiring SHUTDOWN_TX lock...");
        let mut lock = SHUTDOWN_TX.lock().map_err(|e| format!("Failed to lock shutdown: {}", e))?;
        info!("start_proxy: SHUTDOWN_TX lock acquired");
        *lock = Some(tx);
    }
    info!("start_proxy: shutdown channel set up");

    info!("Starting proxy server on port {}", port);

    info!("start_proxy: creating router...");
    let app = create_router();
    info!("start_proxy: router created");

    info!("start_proxy: binding to {}:{}...", host, port);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port))
        .await
        .map_err(|e| format!("Failed to bind to {}:{}: {}", host, port, e))?;
    info!("start_proxy: bound successfully to {}:{}", host, port);

    PROXY_RUNNING.store(true, std::sync::atomic::Ordering::Relaxed);
    info!("Proxy server listening on {}:{}", host, port);

    // Run the server with graceful shutdown
    info!("start_proxy: spawning axum server...");
    tokio::spawn(async move {
        info!("axum server task started");
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                info!("graceful shutdown listener started");
                let mut rx = rx;
                loop {
                    rx.changed().await.ok();
                    if *rx.borrow() {
                        info!("shutdown signal received, breaking");
                        break;
                    }
                }
            })
            .await
            .ok();
        info!("Proxy server stopped");
        PROXY_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
    });
    info!("start_proxy: axum server spawned, returning Ok");

    Ok(())
}

/// Stop the proxy server
pub fn stop_proxy() -> Result<(), String> {
    let mut lock = SHUTDOWN_TX.lock().map_err(|e| format!("Failed to lock shutdown: {}", e))?;
    if let Some(tx) = lock.take() {
        tx.send(true).map_err(|_| "Failed to send shutdown signal".to_string())?;
        PROXY_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
        info!("Proxy shutdown signal sent");
        Ok(())
    } else {
        Err("Proxy was not running".to_string())
    }
}

/// Get proxy status
pub fn get_proxy_status() -> ProxyStatus {
    let host = PROXY_HOST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    ProxyStatus {
        running: PROXY_RUNNING.load(std::sync::atomic::Ordering::Relaxed),
        port: PROXY_PORT.load(std::sync::atomic::Ordering::Relaxed),
        host,
    }
}

/// Create the Axum router with proxy handler
fn create_router() -> axum::Router {
    let client = SHARED_PROXY_CLIENT.clone();
    let state = Arc::new(AppState { client });

    axum::Router::new()
        .route("/*path", axum::routing::any(proxy_handler))
        .with_state(state)
}

struct AppState {
    client: Client,
}

/// Extract model name from request body JSON
fn extract_model_name(body_bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body_bytes).ok()
        .and_then(|v| v.get("model")?.as_str().map(String::from))
}

/// Handle all incoming proxy requests
async fn proxy_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> axum::response::Response {
    let path = uri.path().trim_start_matches('/');

    // Split path into platform prefix and remaining API path
    let (platform_prefix, remaining_path) = match path.split_once('/') {
        Some((prefix, rest)) => (prefix.to_string(), format!("/{}", rest)),
        None => (path.to_string(), "".to_string()),
    };

    // Look up the platform by path_prefix
    let platform_lookup = get_platform_info(&platform_prefix);

    // Determine the effective base_url, platform_id, auto_switch, and target_path
    let (base_url, platform_id, auto_switch, target_path) = match platform_lookup {
        Some((base, id, auto)) => {
            // Normal: platform prefix matched, use the split remaining path
            (base, id, auto, remaining_path.clone())
        }
        None => {
            // Fallback: no platform matches this prefix => use the first configured platform
            // and treat the ENTIRE original path as the API path
            match get_first_platform() {
                Some((base, id, auto)) => {
                    info!(
                        "No platform matches prefix '{}', falling back to '{}' with full path /{}",
                        platform_prefix, id, path
                    );
                    (base, id, auto, format!("/{}", path))
                }
                None => {
                    warn!("Unknown platform prefix: {} (no platforms configured)", platform_prefix);
                    return axum::response::Response::builder()
                        .status(404)
                        .body(axum::body::Body::from(format!(
                            "Unknown platform: {}. Check your proxy path prefix.", platform_prefix
                        )))
                        .unwrap();
                }
            }
        }
    };

    // Build the target URL with dedup for version-like path segments (e.g., /v1)
    let target_url_str = deduplicate_url_path(&base_url, &target_path);
    let target_url = match url::Url::parse(&target_url_str) {
        Ok(u) => u,
        Err(e) => {
            error!("Invalid target URL {}: {}", target_url_str, e);
            return axum::response::Response::builder()
                .status(500)
                .body(axum::body::Body::from(format!("Invalid target URL: {}", e)))
                .unwrap();
        }
    };

    // Read the full body once for parsing and forwarding
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return axum::response::Response::builder()
                .status(400)
                .body(axum::body::Body::from(format!("Failed to read body: {}", e)))
                .unwrap();
        }
    };

    // Try to extract model name from request body
    let model_name = extract_model_name(&body_bytes);

    // Try forwarding the request with key rotation
    let result = forward_with_retry(
        &state.client,
        &method,
        &target_url,
        &headers,
        body_bytes,
        &platform_id,
        &platform_prefix,
        auto_switch,
        model_name,
    ).await;

    match result {
        Ok(response) => response,
        Err(e) => {
            error!("Proxy error for {}: {}", target_url_str, e);
            axum::response::Response::builder()
                .status(502)
                .body(axum::body::Body::from(format!("Proxy error: {}", e)))
                .unwrap()
        }
    }
}

/// Get platform info by path prefix
fn get_platform_info(prefix: &str) -> Option<(String, String, bool)> {
    use crate::modules::config;
    let config = config::load_app_config().ok()?;
    let platform = config.platforms.iter().find(|p| p.path_prefix == prefix)?;
    let auto_switch = config.auto_switch;
    Some((platform.base_url.clone(), platform.id.clone(), auto_switch))
}

/// Get the first configured platform (fallback when no prefix matches)
fn get_first_platform() -> Option<(String, String, bool)> {
    use crate::modules::config;
    let config = config::load_app_config().ok()?;
    let platform = config.platforms.first()?;
    let auto_switch = config.auto_switch;
    Some((platform.base_url.clone(), platform.id.clone(), auto_switch))
}

/// Deduplicate overlapping version-like path segments between base_url and target_path.
/// e.g., base_url="https://api.sensenova.com/v1", target_path="/v1/chat/completions"
///       → "https://api.sensenova.com/v1/chat/completions" (not /v1/v1/...)
/// If target_path does not start with a version prefix, or the prefixes differ,
/// the raw concatenation is returned unchanged.
fn deduplicate_url_path(base_url: &str, target_path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let path = target_path.trim_start_matches('/');

    if let Some((first_seg, rest_path)) = path.split_once('/') {
        // Only deduplicate version-like segments: v1, v2, v3, v2023...
        let is_version = first_seg.len() >= 2
            && first_seg.starts_with('v')
            && first_seg[1..].chars().all(|c| c.is_ascii_digit());

        if is_version && base.ends_with(&format!("/{}", first_seg)) {
            return format!("{}/{}", base, rest_path);
        }
    }

    format!("{}/{}", base, path)
}

/// Get key IDs to try for a platform+model combination
fn get_keys_to_try(platform_id: &str, model_name: Option<String>) -> Vec<String> {
    if let Some(model_name) = model_name {
        // Try to find the model and its associated keys
        if let Ok(models) = crate::modules::model_manager::list_models(platform_id) {
            if let Some(model) = models.iter().find(|m| m.model_name == model_name) {
                if let Ok(key_ids) = crate::modules::key_model_map::get_keys_for_model(&model.id) {
                    if !key_ids.is_empty() {
                        return key_ids;
                    }
                }
            }
        }
    }

    // Fallback: get all active keys for the platform
    if let Ok(keys) = crate::modules::keystore::list_keys(platform_id) {
        return keys.into_iter()
            .filter(|k| k.is_active())
            .map(|k| k.id.clone())
            .collect();
    }

    Vec::new()
}

/// Forward request with automatic key rotation on 429/500.
/// Tracks quota per (model_id, key_id).
async fn forward_with_retry(
    client: &Client,
    method: &axum::http::Method,
    target_url: &url::Url,
    original_headers: &axum::http::HeaderMap,
    body_bytes: axum::body::Bytes,
    platform_id: &str,
    platform_prefix: &str,
    auto_switch: bool,
    model_name: Option<String>,
) -> Result<axum::response::Response, String> {
    let max_retries = if auto_switch { 5 } else { 1 };
    let mut last_error = String::new();

    // Resolve model_id from model_name
    let resolved_model = model_name.as_ref().and_then(|name| {
        crate::modules::model_manager::list_models(platform_id).ok()?
            .into_iter().find(|m| m.model_name == *name)
    });

    let model_id = resolved_model.as_ref().map(|m| m.id.clone());
    let model_identifier = model_name.clone().unwrap_or_else(|| "unknown".to_string());

    // Get keys to try
    let keys_to_try = get_keys_to_try(platform_id, model_name);

    if keys_to_try.is_empty() {
        return Err("No active API keys available for this platform".to_string());
    }

    // Pre-load key values into a map to avoid repeated list_keys calls
    let key_value_map: std::collections::HashMap<String, String> = {
        let keys = crate::modules::keystore::list_keys(platform_id)?;
        keys.into_iter().map(|k| (k.id, k.key_value)).collect()
    };

    for attempt in 0..max_retries {
        let key_idx = attempt % keys_to_try.len();
        let key_id = &keys_to_try[key_idx];

        // Look up key value from pre-loaded map (avoids file I/O per retry)
        let api_key_value = key_value_map.get(key_id)
            .ok_or_else(|| format!("Key not found: {}", key_id))?
            .clone();

        // Build the forwarded request
        let mut req_builder = client.request(method.clone(), target_url.as_str());

        // Forward all headers except Host, Authorization, Content-Length
        for (key, value) in original_headers.iter() {
            let key_str = key.as_str().to_lowercase();
            if key_str != "host" && key_str != "authorization" && key_str != "content-length" {
                req_builder = req_builder.header(key.clone(), value.clone());
            }
        }

        // Set the managed API key
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key_value));
        // Keep Content-Type from original request
        if let Some(ct) = original_headers.get("content-type") {
            req_builder = req_builder.header("Content-Type", ct.clone());
        }

        // Add the full body
        req_builder = req_builder.body(body_bytes.clone());

        // Send the request
        let resp = match req_builder.send().await {
            Ok(r) => r,
            Err(e) => {
                last_error = format!("Request failed: {}", e);
                if attempt < max_retries - 1 {
                    info!("Connection error, trying next key: {}", e);
                    continue;
                }
                break;
            }
        };

        let status = resp.status();
        let is_error = status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;

        if is_error {
            let key_label = format!("key[{}]", key_idx);
            let (reason_str, disabled_until_ts) = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                warn!("429 from {}, {}={}, model={}, trying next",
                    target_url, key_label, key_id, model_identifier);
                if let Some(mid) = &model_id {
                    let _ = crate::modules::quota_window::record_429_error(key_id, mid, platform_id);
                }
                let reason = format!("Rate limited at {}", chrono::Utc::now().format("%H:%M:%S"));
                let until = chrono::Utc::now().timestamp() + 120;
                let _ = crate::modules::keystore::set_key_status(key_id, true, Some(reason.clone()), Some(until));
                (reason, until)
            } else {
                warn!("{} from {}, {}={}, model={}, trying next",
                    status, target_url, key_label, key_id, model_identifier);
                if let Some(mid) = &model_id {
                    let _ = crate::modules::quota_window::record_500_error(key_id, mid, platform_id);
                }
                let reason = format!("Server error {} at {}", status.as_u16(), chrono::Utc::now().format("%H:%M:%S"));
                let until = chrono::Utc::now().timestamp() + 60;
                let _ = crate::modules::keystore::set_key_status(key_id, true, Some(reason.clone()), Some(until));
                (reason, until)
            };

            // Emit key-switched event so frontend can refresh quota display
            if attempt < max_retries - 1 {
                let next_idx = (attempt + 1) % keys_to_try.len();
                let next_key_id = keys_to_try[next_idx].clone();
                let platform_name = crate::modules::config::load_app_config()
                    .ok()
                    .and_then(|c| c.platforms.into_iter().find(|p| p.id == platform_id))
                    .map(|p| p.name)
                    .unwrap_or_else(|| platform_prefix.to_string());
                crate::modules::log_bridge::emit_key_switched(
                    crate::modules::log_bridge::KeySwitchedPayload {
                        platform_id: platform_id.to_string(),
                        platform_name,
                        model_name: model_identifier.clone(),
                        disabled_key_id: key_id.clone(),
                        next_key_id,
                        reason: reason_str,
                        disabled_until: disabled_until_ts,
                    }
                );
            }

            last_error = format!("HTTP {} from upstream", status);
            if attempt < max_retries - 1 {
                info!("Retrying with next key (attempt {}/{})", attempt + 2, max_retries);
                continue;
            }
            break;
        }

        // Success case - record the API call
        if let Some(mid) = &model_id {
            let _ = crate::modules::quota_window::record_api_call(key_id, mid, platform_id);
        }

        // Capture response headers before consuming body
        let response_headers: Vec<(String, String)> = resp.headers().iter()
            .filter(|(key, _)| key.as_str().to_lowercase() != "transfer-encoding")
            .map(|(key, value)| (key.as_str().to_string(), value.to_str().unwrap_or("").to_string()))
            .collect();

        // Stream the response body back to client (avoids hanging on streaming APIs)
        let body = axum::body::Body::from_stream(resp.bytes_stream());

        let mut response_builder = axum::response::Response::builder().status(status);
        for (key, value) in &response_headers {
            response_builder = response_builder.header(key.as_str(), value.as_str());
        }

        return response_builder
            .body(body)
            .map_err(|e| format!("Failed to build response: {}", e));
    }

    Err(format!("All keys exhausted for platform '{}': {}", platform_prefix, last_error))
}
