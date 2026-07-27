use std::sync::Arc;
use tokio::sync::watch;
use serde::Serialize;
use reqwest::Client;
use std::sync::Mutex;
use once_cell::sync::Lazy;
use rand::seq::SliceRandom;

use futures::StreamExt;
use tracing::{info, warn, error};

/// Helper: build an HTTP error response without `.unwrap()`.
/// Returns an axum Response with the given status code and body text.
fn error_response(status: u16, body: String) -> axum::response::Response {
    axum::response::Response::builder()
        .status(status)
        .body(axum::body::Body::from(body))
        // If builder() itself fails (e.g., invalid status code), fall back to 500.
        .unwrap_or_else(|_| {
            axum::response::Response::new(axum::body::Body::from(
                format!("Internal error (status={})", status),
            ))
        })
}

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
    let mut lock = match SHUTDOWN_TX.lock() {
        Ok(l) => l,
        Err(poisoned) => {
            // If poisoned, recover by taking the inner value
            poisoned.into_inner()
        }
    };
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
    let host = PROXY_HOST.lock()
        .map(|h| h.clone())
        .unwrap_or_else(|e| e.into_inner().clone());
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

/// Parse JSON body once and return both the model_name and the modified body bytes
/// with max_tokens injected if needed. Returns (model_name_opt, modified_body_bytes).
fn parse_and_prepare_body(
    body_bytes: &[u8],
    target_path: &str,
) -> (Option<String>, Vec<u8>) {
    // Only inject for completion/message endpoints
    let needs_max_tokens = target_path.contains("/chat/completions")
        || target_path.contains("/v1/messages")
        || target_path.contains("/completions");

    if !needs_max_tokens {
        // Still try to extract model name (single parse)
        let model_name = serde_json::from_slice::<serde_json::Value>(body_bytes).ok()
            .and_then(|v| v.get("model")?.as_str().map(String::from));
        return (model_name, body_bytes.to_vec());
    }

    // Parse once and extract both model_name and potentially inject max_tokens
    match serde_json::from_slice::<serde_json::Value>(body_bytes) {
        Ok(mut json) => {
            let model_name = json.get("model").and_then(|v| v.as_str().map(String::from));

            let needs_fix = match json.get("max_tokens") {
                None => true,
                Some(val) if val.is_null() => true,
                Some(val) => {
                    val.as_u64().map_or(true, |n| n == 0 || n > 65536)
                }
            };
            if needs_fix {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert(
                        "max_tokens".to_string(),
                        serde_json::Value::Number(4096.into()),
                    );
                }
                info!("Injected default max_tokens=4096 into request body (was missing or invalid)");
                let modified = serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec());
                return (model_name, modified);
            }

            (model_name, body_bytes.to_vec())
        }
        Err(_) => (None, body_bytes.to_vec()),
    }
}

/// Handle `/v1/models` requests — return the models configured in Antigravity Hub
/// in OpenAI-compatible format.
///
/// OpenAI format:
/// ```json
/// {
///   "object": "list",
///   "data": [
///     {"id": "model-name", "object": "model", "created": 1234567890, "owned_by": "platform-id"}
///   ]
/// }
/// ```
fn handle_models_request() -> axum::response::Response {
    use crate::modules::config;
    use crate::modules::model_manager;

    let mut data: Vec<serde_json::Value> = Vec::new();

    // Iterate over all configured platforms
    match config::load_app_config() {
        Ok(cfg) => {
            info!("handle_models_request: loaded config with {} platforms", cfg.platforms.len());
            for platform in &cfg.platforms {
                match model_manager::list_models(&platform.id) {
                    Ok(models) => {
                        info!("handle_models_request: platform '{}' has {} models", platform.id, models.len());
                        for model in &models {
                            data.push(serde_json::json!({
                                "id": model.model_name,
                                "object": "model",
                                "created": model.created_at,
                                "owned_by": platform.id,
                            }));
                        }
                    }
                    Err(e) => {
                        warn!("handle_models_request: list_models error for platform '{}': {}", platform.id, e);
                    }
                }
            }
        }
        Err(e) => {
            warn!("handle_models_request: load_app_config failed: {}", e);
        }
    }

    info!("handle_models_request: returning {} models total", data.len());

    let response_body = serde_json::json!({
        "object": "list",
        "data": data,
    });

    let body_bytes = serde_json::to_vec(&response_body).unwrap_or_default();
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body_bytes))
        .unwrap_or_else(|_| {
            axum::response::Response::new(axum::body::Body::from("{\"error\":\"internal error\"}"))
        })
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
        None => (path.to_string(), String::new()),
    };

    // Look up the platform by path_prefix
    let platform_lookup = get_platform_info(&platform_prefix);

    // Determine the effective base_url, platform_id, auto_switch, and target_path
    let (base_url, platform_id, auto_switch, target_path) = match platform_lookup {
        Some((base, id, auto)) => {
            // Normal: platform prefix matched, use the split remaining path
            (base, id, auto, remaining_path)
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
                    return error_response(404, format!(
                        "Unknown platform: {}. Check your proxy path prefix.", platform_prefix
                    ));
                }
            }
        }
    };

    // ── Responses API compatibility ──
    // Codex CLI uses the OpenAI Responses API (/v1/responses), but most
    // upstream providers only support Chat Completions (/v1/chat/completions).
    // We detect and translate the API format transparently so the proxy
    // works with Codex CLI and any provider.
    let is_responses_api = target_path == "/v1/responses"
        || target_path.starts_with("/v1/responses/");
    let target_path = if is_responses_api {
        let new_path = if target_path == "/v1/responses" {
            "/v1/chat/completions".to_string()
        } else {
            target_path.replacen("/v1/responses", "/v1/chat/completions", 1)
        };
        info!("Responses API: path mapped '{}' → '{}'", target_path, new_path);
        new_path
    } else {
        target_path.clone()
    };

    // ── Model list (/v1/models) ──
    // Codex CLI calls /v1/models to discover available models. Instead of
    // forwarding to the upstream provider (which may not return the configured
    // models), we intercept and return the models configured in Antigravity Hub.
    if target_path == "/v1/models" || target_path == "/v1/models/" {
        info!("Intercepting /v1/models request, returning configured models");
        return handle_models_request();
    }

    // Check for path-specific base URL overrides (e.g., /agnesapi at the API root, not under /v1)
    let effective_base_url = resolve_base_url(&platform_id, &target_path, &base_url);

    // Build the target URL with dedup for version-like path segments (e.g., /v1)
    let mut target_url_str = deduplicate_url_path(&effective_base_url, &target_path);
    // Preserve query parameters from the original request (e.g., ?video_id=xxx)
    if let Some(query) = uri.query() {
        if !query.is_empty() {
            target_url_str.push('?');
            target_url_str.push_str(query);
        }
    }
    let target_url = match url::Url::parse(&target_url_str) {
        Ok(u) => u,
        Err(e) => {
            error!("Invalid target URL {}: {}", target_url_str, e);
            return error_response(500, format!("Invalid target URL: {}", e));
        }
    };

    // Read the full body once for parsing and forwarding
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return error_response(400, format!("Failed to read body: {}", e));
        }
    };

    // ── Responses API request body transformation ──
    // Translate request body from Responses API format to Chat Completions format
    // BEFORE parse_and_prepare_body so it sees the correct field names.
    let body_bytes = if is_responses_api {
        transform_responses_to_chat_completions(&body_bytes)
            .map(|t| {
                info!("Responses API: request body translated ({} bytes → {} bytes)", body_bytes.len(), t.len());
                t.into()
            })
            .unwrap_or(body_bytes)
    } else {
        body_bytes
    };

    // Parse once: extract model name AND inject max_tokens in a single pass
    let (model_name, body_bytes) = parse_and_prepare_body(&body_bytes, &target_path);
    let body_bytes: axum::body::Bytes = body_bytes.into();

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
        is_responses_api,
    ).await;

    match result {
        Ok(response) => response,
        Err(e) => {
            error!("Proxy error for {}: {}", target_url_str, e);
            error_response(502, format!("Proxy error: {}", e))
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

/// Resolve the effective base URL for a given target path, taking into account
/// any path-specific base URL overrides defined in the platform config.
/// E.g., if the platform's base_url is "https://apihub.agnes-ai.com/v1" and
/// there's an override for path "/agnesapi" with base_url "https://apihub.agnes-ai.com",
/// then requests to "/agnesapi/..." will use the override base_url (without /v1).
fn resolve_base_url(platform_id: &str, target_path: &str, default_base_url: &str) -> String {
    use crate::modules::config;
    if let Ok(config) = config::load_app_config() {
        if let Some(platform) = config.platforms.iter().find(|p| p.id == platform_id) {
            for override_entry in &platform.base_url_overrides {
                // Match if target_path starts with the override prefix
                if target_path == &override_entry.path_prefix
                    || target_path.starts_with(&format!("{}/", override_entry.path_prefix))
                {
                    info!(
                        "Using base URL override for path prefix '{}': '{}' (override '{}')",
                        override_entry.path_prefix,
                        override_entry.base_url,
                        default_base_url
                    );
                    return override_entry.base_url.clone();
                }
            }
        }
    }
    default_base_url.to_string()
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
    // If target_path is empty, just return the base URL
    let path = target_path.trim_start_matches('/');
    if path.is_empty() {
        return base.to_string();
    }

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

/// Get key IDs to try for a platform+model combination.
///
/// Resolution order:
///   1. If a key_model_map entry exists for the resolved model_id, use it.
///   2. Otherwise, fall back to every active key on the platform (sorted by sort_order).
///
/// After resolution, drop any key whose local sliding-window tracker reports the
/// (key_id, model_id) pair as over quota or in backoff. This is what triggers an
/// automatic key switch when the local counter is past the configured daily /
/// weekly / monthly limit, even if the upstream keeps returning 200 OK.
fn get_keys_to_try(platform_id: &str, model_name: Option<String>) -> Vec<String> {
    // Resolve model_id once (used both for the mapping lookup and the quota filter).
    let model_id: Option<String> = model_name.as_ref().and_then(|name| {
        crate::modules::model_manager::list_models(platform_id)
            .ok()?
            .into_iter()
            .find(|m| m.model_name == *name)
            .map(|m| m.id)
    });

    // Build candidate set: explicit mapping wins, otherwise fall back to all active keys.
    // NOTE: key_model_map returns ALL associated key IDs regardless of keystore status,
    // so we must intersect with active keys to exclude manually-disabled ones.
    let candidates: Vec<String> = if let Some(mid) = model_id.as_ref() {
        match crate::modules::key_model_map::get_keys_for_model(mid) {
            Ok(ids) if !ids.is_empty() => {
                // Filter out keys that are disabled in the keystore (manually disabled)
                let active_ids = list_active_key_ids(platform_id);
                let active_set: std::collections::HashSet<&String> = active_ids.iter().collect();
                ids.into_iter().filter(|id| active_set.contains(id)).collect()
            }
            _ => list_active_key_ids(platform_id),
        }
    } else {
        list_active_key_ids(platform_id)
    };

    // Skip keys whose quota window is already exceeded or currently in backoff.
    let mut available = if let Some(mid) = model_id.as_ref() {
        crate::modules::quota_window::filter_available_keys(&candidates, mid, platform_id)
    } else {
        candidates
    };

    // Load balancing: shuffle available keys so traffic is distributed
    // rather than always hitting key[0] first.
    let mut rng = rand::thread_rng();
    available.shuffle(&mut rng);

    available
}

/// Helper: return IDs of every active key on a platform, preserving sort_order.
fn list_active_key_ids(platform_id: &str) -> Vec<String> {
    crate::modules::keystore::list_keys(platform_id)
        .map(|keys| {
            keys.into_iter()
                .filter(|k| k.is_active())
                .map(|k| k.id)
                .collect()
        })
        .unwrap_or_default()
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
    is_responses_api: bool,
) -> Result<axum::response::Response, String> {
    let max_retries = if auto_switch { 5 } else { 1 };
    let mut last_error = String::new();

    // Resolve model_id from model_name (done once outside the loop)
    let model_id: Option<String> = model_name.as_ref().and_then(|name| {
        crate::modules::model_manager::list_models(platform_id).ok()?
            .into_iter().find(|m| m.model_name == *name)
            .map(|m| m.id)
    });

    let model_identifier = model_name.clone().unwrap_or_else(|| "unknown".to_string());

    // Pre-load key values into a map to avoid repeated list_keys calls
    let key_value_map: std::collections::HashMap<String, String> = {
        let keys = crate::modules::keystore::list_keys(platform_id)?;
        keys.into_iter().map(|k| (k.id, k.key_value)).collect()
    };

    for attempt in 0..max_retries {
        // Refresh keys_to_try on each attempt (except the first) so that
        // keys disabled by a 500 error on a previous attempt are excluded.
        let keys_to_try = if attempt == 0 {
            get_keys_to_try(platform_id, model_name.clone())
        } else {
            // After a failure, re-fetch available keys to skip disabled ones
            get_keys_to_try(platform_id, model_name.clone())
        };

        if keys_to_try.is_empty() {
            return Err("No active API keys available for this platform".to_string());
        }

        // Pick the next key (cycling through available keys; after a failure
        // the list is refreshed, so we try attempt % len to pick a new key).
        let key_idx = attempt % keys_to_try.len();
        let key_id = &keys_to_try[key_idx];

        // Look up key value from pre-loaded map (avoids file I/O per retry)
        let api_key_value = match key_value_map.get(key_id) {
            Some(val) => val.clone(),
            None => {
                // Key was deleted between retries, skip to next attempt
                last_error = format!("Key not found: {}", key_id);
                info!("Key '{}' not found in key map, skipping to next attempt", key_id);
                continue;
            }
        };

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

        // 429: rate limited — wait with exponential backoff, then retry SAME key
        // Do NOT switch keys for 429; the key is still valid, just temporarily throttled
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            warn!("429 from {}, key[{}]={}, model={}, will retry same key",
                target_url, key_idx, key_id, model_identifier);
            if let Some(mid) = &model_id {
                let _ = crate::modules::quota_window::record_429_error(key_id, mid, platform_id);
            }
            last_error = "Rate limited (429)".to_string();
            // Exponential backoff: 2s, 4s, 8s, 16s, 32s (capped at 32s)
            let backoff_secs = std::cmp::min(32, 2_u64.pow(attempt as u32 + 1));
            info!("Waiting {}s before retrying same key...", backoff_secs);
            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            continue; // Retry the same key
        }

        let is_error = status.is_server_error();

        if is_error {
            let key_label = format!("key[{}]", key_idx);
            let (reason_str, disabled_until_ts) = {
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
                // After record_500, the key is disabled, so get_keys_to_try on the
                // next iteration will exclude it. Pick the next available key from
                // the refreshed list (which will be fetched at the top of the loop).
                let next_key_id = keys_to_try
                    .get((key_idx + 1) % keys_to_try.len())
                    .cloned()
                    .unwrap_or_default();
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

        // Detect streaming responses so we don't break token-by-token delivery
        // to the client. OpenAI / Anthropic streaming uses `text/event-stream`;
        // non-streaming responses use `application/json` (or no content-type).
        let is_streaming = response_headers.iter().any(|(k, v)| {
            k.to_lowercase() == "content-type"
                && v.to_lowercase().contains("text/event-stream")
        });

        let mut response_builder = axum::response::Response::builder().status(status);
        for (key, value) in &response_headers {
            response_builder = response_builder.header(key.as_str(), value.as_str());
        }

        if is_streaming {
            crate::modules::token_stats::record_streaming_request();
            if is_responses_api {
                // Translate SSE stream from Chat Completions format to
                // Responses API format on-the-fly so Codex CLI can parse it.
                // The upstream returns Chat Completions SSE chunks, but Codex
                // CLI expects Responses API SSE events.
                let body = axum::body::Body::from_stream(
                    transform_stream_to_responses(resp.bytes_stream(), &model_identifier)
                );
                return response_builder
                    .body(body)
                    .map_err(|e| format!("Failed to build response: {}", e));
            } else {
                // Pass-through for non-Responses API streaming.
                let body = axum::body::Body::from_stream(resp.bytes_stream());
                return response_builder
                    .body(body)
                    .map_err(|e| format!("Failed to build response: {}", e));
            }
        }

        // Non-streaming: buffer the body so we can inspect `usage` before
        // forwarding to the client. This adds one round-trip of latency
        // (wait for full response) but is required to count tokens.
        let body_bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Err(format!("Failed to read upstream response: {}", e));
            }
        };

        // Try to extract `usage` from the JSON body. Both OpenAI and Anthropic
        // expose prompt/completion token counts under `usage.{prompt_tokens,
        // completion_tokens}`; missing fields are silently ignored.
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            if let Some(usage) = json.get("usage") {
                let prompt = usage
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let completion = usage
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if prompt > 0 || completion > 0 {
                    crate::modules::token_stats::record_usage(prompt, completion);
                }
            }
        }

        // ── Responses API response translation ──
        // Translate the response body from Chat Completions format back to
        // Responses API format so Codex CLI can understand it.
        let body_bytes = if is_responses_api {
            transform_chat_completions_to_responses(&body_bytes)
                .map(|t| {
                    info!("Responses API: response body translated ({} bytes → {} bytes)", body_bytes.len(), t.len());
                    t.into()
                })
                .unwrap_or(body_bytes)
        } else {
            body_bytes
        };

        let body = axum::body::Body::from(body_bytes);
        return response_builder
            .body(body)
            .map_err(|e| format!("Failed to build response: {}", e));
    }

    Err(format!("All keys exhausted for platform '{}': {}", platform_prefix, last_error))
}

// ── Responses API ↔ Chat Completions API translation ──
// Codex CLI uses the OpenAI Responses API (/v1/responses), but most
// upstream providers only support Chat Completions (/v1/chat/completions).
// These functions translate between the two formats transparently.

/// Translate a Responses API request body to Chat Completions API format.
///
/// Responses API format:  {"model":"...","input":"...","max_output_tokens":...}
/// Chat Completions format: {"model":"...","messages":[...],"max_tokens":...}
fn transform_responses_to_chat_completions(body_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut json: serde_json::Value = serde_json::from_slice(body_bytes).ok()?;
    let obj = json.as_object_mut()?;

    // Helper: convert a pending function_call item into a Chat Completions tool_call object
    let fn_call_to_tool_call = |fc: &serde_json::Value| -> serde_json::Value {
        let id = fc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
        let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let arguments = fc.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}").to_string();
        serde_json::json!({
            "id": id,
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments
            }
        })
    };

    // Helper: convert a single Responses API content part to Chat Completions format.
    // Responses API content types: input_text, input_image, input_file, input_audio, output_text
    // Chat Completions content types: text, image_url, file, audio
    let convert_content_part = |part: &mut serde_json::Value| {
        if let Some(obj) = part.as_object_mut() {
            let part_type = obj.get("type").and_then(|t| t.as_str()).map(|s| s.to_string());
            match part_type.as_deref() {
                Some("input_text") => {
                    obj["type"] = serde_json::Value::String("text".to_string());
                }
                Some("input_image") => {
                    obj["type"] = serde_json::Value::String("image_url".to_string());
                    // image_url might be a string (raw URL) or an object {url: string}
                    if let Some(image_url) = obj.get("image_url") {
                        if image_url.is_string() {
                            let url = image_url.as_str().unwrap_or("").to_string();
                            obj["image_url"] = serde_json::json!({"url": url});
                        }
                    }
                }
                Some("input_file") | Some("input_audio") => {
                    // Strip "input_" prefix: input_file → file, input_audio → audio
                    if let Some(rest) = part_type.as_deref().and_then(|t| t.strip_prefix("input_")) {
                        obj["type"] = serde_json::Value::String(rest.to_string());
                    }
                }
                Some("output_text") => {
                    // output_text → text (in input context, e.g. assistant messages)
                    obj["type"] = serde_json::Value::String("text".to_string());
                }
                _ => {}
            }
        }
    };

    // Helper: convert content array in a message from Responses API format to Chat Completions format
    let convert_message_content = |msg: &mut serde_json::Value| {
        if let Some(obj) = msg.as_object_mut() {
            if let Some(content) = obj.get_mut("content") {
                match content {
                    serde_json::Value::Array(arr) => {
                        for part in arr.iter_mut() {
                            convert_content_part(part);
                        }
                    }
                    _ => {} // string content is fine as-is
                }
            }
        }
    };

    // Helper: flush pending function_calls into an assistant message with tool_calls
    let flush_fn_calls = |pending: &mut Vec<serde_json::Value>, messages: &mut Vec<serde_json::Value>| {
        if pending.is_empty() {
            return;
        }
        let tool_calls: Vec<serde_json::Value> = pending.drain(..).map(|fc| fn_call_to_tool_call(&fc)).collect();
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": tool_calls
        }));
    };

    // Translate `input` → `messages`
    if let Some(input) = obj.remove("input") {
        let messages = match input {
            serde_json::Value::String(s) => {
                vec![serde_json::json!({"role": "user", "content": s})]
            }
            serde_json::Value::Array(items) => {
                let mut messages: Vec<serde_json::Value> = Vec::new();
                let mut pending_function_calls: Vec<serde_json::Value> = Vec::new();

                for item in items {
                    match item {
                        serde_json::Value::String(s) => {
                            flush_fn_calls(&mut pending_function_calls, &mut messages);
                            messages.push(serde_json::json!({"role": "user", "content": s}));
                        }
                        serde_json::Value::Object(m) => {
                            let item_type = m.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            let has_role = m.contains_key("role");

                            if item_type == "function_call_output" {
                                // ── Tool result: function_call_output → tool role message ──
                                flush_fn_calls(&mut pending_function_calls, &mut messages);
                                let call_id = m.get("call_id").and_then(|c| c.as_str()).unwrap_or("").to_string();
                                let output = m.get("output").and_then(|o| o.as_str()).unwrap_or("").to_string();
                                messages.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": call_id,
                                    "content": output
                                }));
                            } else if item_type == "function_call" {
                                // ── function_call: buffer for grouping with next function_call_output ──
                                pending_function_calls.push(serde_json::Value::Object(m));
                            } else if has_role {
                                // ── Regular message with role (user/assistant/system) ──
                                flush_fn_calls(&mut pending_function_calls, &mut messages);
                                // Convert Responses API content types (input_text, input_image)
                                // to Chat Completions types (text, image_url) for upstream compatibility
                                let mut msg = serde_json::Value::Object(m);
                                // Remove Responses API-specific fields that are not valid in Chat Completions
                                if let Some(obj) = msg.as_object_mut() {
                                    obj.remove("type");
                                    obj.remove("input");
                                    obj.remove("from_messages");
                                }
                                convert_message_content(&mut msg);
                                messages.push(msg);
                            } else if let Some(content) = m.get("content") {
                                // ── Object with content but no role → assume user message ──
                                flush_fn_calls(&mut pending_function_calls, &mut messages);
                                let mut msg = serde_json::json!({"role": "user"});
                                msg["content"] = content.clone();
                                convert_message_content(&mut msg);
                                messages.push(msg);
                            } else {
                                // ── Unknown object → fallback to user message ──
                                flush_fn_calls(&mut pending_function_calls, &mut messages);
                                messages.push(serde_json::json!({"role": "user", "content": serde_json::Value::Object(m)}));
                            }
                        }
                        other => {
                            flush_fn_calls(&mut pending_function_calls, &mut messages);
                            messages.push(serde_json::json!({"role": "user", "content": other}));
                        }
                    }
                }

                // Flush any remaining pending function calls at the end
                flush_fn_calls(&mut pending_function_calls, &mut messages);

                messages
            }
            _ => {
                vec![serde_json::json!({"role": "user", "content": input})]
            }
        };
        obj.insert("messages".to_string(), serde_json::Value::Array(messages));
        // Clean up other Responses API fields that might be in the request body
        obj.remove("from_messages");
    } else if let Some(from_messages) = obj.remove("from_messages") {
        // `from_messages` is a simpler Responses API format that uses Chat Completions-style
        // messages directly. Just rename to `messages` and convert content types.
        if let Some(arr) = from_messages.as_array() {
            let mut messages: Vec<serde_json::Value> = Vec::new();
            for mut msg in arr.clone() {
                // Remove Responses API-specific fields
                if let Some(obj) = msg.as_object_mut() {
                    obj.remove("type");
                    obj.remove("input");
                    obj.remove("from_messages");
                }
                convert_message_content(&mut msg);
                messages.push(msg);
            }
            obj.insert("messages".to_string(), serde_json::Value::Array(messages));
        } else {
            obj.insert("messages".to_string(), from_messages);
        }
    }

    // Translate `instructions` → system message (prepended to messages)
    if let Some(instructions) = obj.remove("instructions") {
        if let Some(instructions_str) = instructions.as_str() {
            let empty_array = serde_json::Value::Array(vec![]);
            let mut messages = obj.remove("messages").unwrap_or(empty_array);
            if let Some(msg_array) = messages.as_array_mut() {
                msg_array.insert(0, serde_json::json!({"role": "system", "content": instructions_str}));
            }
            obj.insert("messages".to_string(), messages);
        }
    }

    // Translate `max_output_tokens` → `max_tokens`
    if let Some(max_output) = obj.remove("max_output_tokens") {
        if !obj.contains_key("max_tokens") {
            obj.insert("max_tokens".to_string(), max_output);
        }
    }

    // Convert tools from Responses API format to Chat Completions format.
    // Responses API tool format:
    //   {"type": "function", "name": "...", "description": "...", "parameters": {...}}
    // Chat Completions tool format:
    //   {"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}
    // Responses API supports additional types: file_search, code_interpreter, web_search,
    // computer_use, custom — these must be removed since Chat Completions only supports
    // "function".
    if let Some(tools) = obj.get_mut("tools") {
        if let Some(tools_array) = tools.as_array_mut() {
            let mut converted: Vec<serde_json::Value> = Vec::new();
            for mut tool in tools_array.drain(..) {
                let tool_type = tool.get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();

                if tool_type == "function" {
                    // Convert Responses API format to Chat Completions format
                    if let Some(tool_obj) = tool.as_object_mut() {
                        // Check if the function properties are at the top level (Responses API format)
                        let has_top_level_name = tool_obj.contains_key("name");
                        let has_function_nesting = tool_obj.contains_key("function");

                        if has_top_level_name && !has_function_nesting {
                            // Move name, description, parameters into a nested "function" object
                            let mut fn_obj = serde_json::Map::new();
                            if let Some(name) = tool_obj.remove("name") {
                                fn_obj.insert("name".to_string(), name);
                            }
                            if let Some(desc) = tool_obj.remove("description") {
                                fn_obj.insert("description".to_string(), desc);
                            }
                            if let Some(params) = tool_obj.remove("parameters") {
                                fn_obj.insert("parameters".to_string(), params);
                            }
                            if !fn_obj.is_empty() {
                                tool_obj.insert("function".to_string(), serde_json::Value::Object(fn_obj));
                            }
                        }
                    }
                    converted.push(tool);
                } else {
                    // Remove non-function tool types (file_search, code_interpreter,
                    // web_search, computer_use, custom, etc.)
                    info!("Removing unsupported tool type '{}' from Responses API request", tool_type);
                }
            }
            if converted.is_empty() {
                obj.remove("tools");
            } else {
                *tools = serde_json::Value::Array(converted);
            }
        }
    }

    // ── Post-processing: validate messages and clean up Responses API fields ──

    // Rename "developer" role to "system" for upstream compatibility.
    // The "developer" role is a newer OpenAI concept that most third-party
    // providers (Sensenova, Agnes, etc.) don't support, but it is semantically
    // equivalent to "system" — same positioning in the context window.
    if let Some(messages) = obj.get_mut("messages") {
        if let Some(arr) = messages.as_array_mut() {
            for msg in arr.iter_mut() {
                if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
                    if role == "developer" {
                        if let Some(obj) = msg.as_object_mut() {
                            obj.insert("role".to_string(), serde_json::Value::String("system".to_string()));
                        }
                    }
                }
            }
        }
    }

    // Ensure every remaining message has a valid role (fallback to "user" if missing/invalid)
    let valid_roles: std::collections::HashSet<&str> = ["user", "assistant", "system", "tool"]
        .iter().cloned().collect();
    if let Some(messages) = obj.get_mut("messages") {
        if let Some(arr) = messages.as_array_mut() {
            for (idx, msg) in arr.iter_mut().enumerate() {
                let role_valid = msg.get("role")
                    .and_then(|r| r.as_str())
                    .map(|r| valid_roles.contains(r))
                    .unwrap_or(false);
                if !role_valid {
                    warn!("transform: messages[{}] has invalid role, setting to 'user'", idx);
                    if let Some(obj) = msg.as_object_mut() {
                        obj.insert("role".to_string(), serde_json::Value::String("user".to_string()));
                    }
                }
            }
        }
    }

    // Remove Responses API-specific fields that don't exist in Chat Completions
    obj.remove("previous_response_id");
    obj.remove("store");

    // Log the transformed request body for debugging
    if let Ok(log_body) = serde_json::to_string_pretty(&json) {
        if log_body.len() < 2000 {
            info!("transform: transformed request body:\n{}", log_body);
        } else {
            let preview: String = log_body.chars().take(1000).collect();
            info!("transform: transformed request body (truncated):\n{}", preview);
        }
    }

    Some(serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec()))
}

/// Translate a Chat Completions API response body to Responses API format.
///
/// Chat Completions response format:
///   {"id":"chatcmpl-...","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"...","tool_calls":[...]}}],"usage":{...}}
/// Responses API format:
///   {"id":"resp_...","object":"response","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"..."}]}, {"type":"function_call",...}],"usage":{...}}
fn transform_chat_completions_to_responses(body_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut json: serde_json::Value = serde_json::from_slice(body_bytes).ok()?;
    let obj = json.as_object_mut()?;

    // Change `object` from "chat.completion" to "response"
    if let Some(object) = obj.get("object") {
        if object == "chat.completion" {
            obj.insert("object".to_string(), serde_json::json!("response"));
        }
    }

    // Translate `choices` → `output`
    if let Some(choices) = obj.remove("choices") {
        if let Some(choices_array) = choices.as_array() {
            let mut output: Vec<serde_json::Value> = Vec::new();

            for choice in choices_array {
                if let Some(message) = choice.get("message") {
                    let role = message.get("role").and_then(|r| r.as_str()).unwrap_or("assistant");

                    // ── Text content (may be null when tool_calls is present) ──
                    let has_text = message.get("content").and_then(|c| c.as_str()).map_or(false, |s| !s.is_empty());
                    if has_text {
                        if let Some(content_str) = message.get("content").and_then(|c| c.as_str()) {
                            output.push(serde_json::json!({
                                "type": "message",
                                "role": role,
                                "content": [
                                    {"type": "output_text", "text": content_str}
                                ]
                            }));
                        }
                    }

                    // ── Tool calls ──
                    // Chat Completions tool_calls → Responses API function_call output items
                    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
                        for tc in tool_calls {
                            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let name = tc.get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str())
                                .unwrap_or("");
                            let arguments = tc.get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str())
                                .unwrap_or("{}");
                            output.push(serde_json::json!({
                                "type": "function_call",
                                "id": id,
                                "call_id": id,
                                "name": name,
                                "arguments": arguments
                            }));
                        }
                    }

                    // ── Reasoning / thinking content ──
                    // Some providers return reasoning_content in the Chat Completions response.
                    // Translate it to a reasoning output item in Responses API format.
                    if let Some(reasoning) = message.get("reasoning_content").and_then(|r| r.as_str()) {
                        if !reasoning.is_empty() {
                            output.push(serde_json::json!({
                                "type": "reasoning",
                                "reasoning": reasoning
                            }));
                        }
                    }
                }
            }

            obj.insert("output".to_string(), serde_json::Value::Array(output));
        }
    }

    // Translate usage: prompt_tokens → input_tokens, completion_tokens → output_tokens
    if let Some(usage) = obj.get_mut("usage") {
        if let Some(usage_obj) = usage.as_object() {
            let mut new_usage = serde_json::Map::new();
            if let Some(prompt) = usage_obj.get("prompt_tokens") {
                new_usage.insert("input_tokens".to_string(), prompt.clone());
            }
            if let Some(completion) = usage_obj.get("completion_tokens") {
                new_usage.insert("output_tokens".to_string(), completion.clone());
            }
            if let Some(total) = usage_obj.get("total_tokens") {
                new_usage.insert("total_tokens".to_string(), total.clone());
            }
            if !new_usage.is_empty() {
                *usage = serde_json::Value::Object(new_usage);
            }
        }
    }

    // Remove Chat Completions-specific fields
    obj.remove("choices");

    Some(serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec()))
}

/// Translate a Chat Completions streaming SSE response to Responses API SSE format.
///
/// Reads SSE chunks from the upstream stream, translates each chunk on-the-fly,
/// and emits Responses API format events. This allows Codex CLI to receive
/// streaming responses in the format it expects.
///
/// Chat Completions streaming SSE:
///   data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}
///   data: [DONE]
///
/// Responses API streaming SSE:
///   event: response.created
///   data: {"type":"response.created","response":{"id":"resp_xxx","status":"in_progress"}}
///   event: response.output_text.delta
///   data: {"type":"response.output_text.delta","delta":"Hello","item_id":"...","output_index":0}
///   event: response.done
///   data: {"type":"response.done","response":{"id":"resp_xxx","status":"completed"}}
///   data: [DONE]
fn transform_stream_to_responses(
    upstream_stream: impl futures::stream::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static,
    model: &str,
) -> impl futures::stream::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, reqwest::Error>>(64);
    let model_owned = model.to_string();

    // ── Helper: emit a data-only SSE line (Responses API uses `data: {json}\n\n` format) ──
    // Use try_send (non-blocking) instead of blocking_send because we're in a
    // tokio async context. blocking_send would block the tokio worker thread
    // and can cause stream stalls or deadlocks.
    let tx_clone = tx.clone();
    let emit_data_sse = move |data: &serde_json::Value| {
        let sse = format!(
            "data: {}\n\n",
            serde_json::to_string(data).unwrap_or_default()
        );
        let result = tx_clone.try_send(Ok(axum::body::Bytes::from(sse)));
        if let Err(e) = result {
            warn!("transform_stream: try_send failed (data event): {:?}", e);
        }
    };

    // ── Helper: emit a raw data line (for [DONE]) ──
    let tx_clone = tx.clone();
    let emit_raw = move |data: &str| {
        let sse = format!("data: {}\n\n", data);
        let result = tx_clone.try_send(Ok(axum::body::Bytes::from(sse)));
        if let Err(e) = result {
            warn!("transform_stream: try_send failed (raw event): {:?}", e);
        }
    };

    tokio::spawn(async move {
        info!("transform_stream_to_responses: stream processing started");

        let now = chrono::Utc::now().timestamp();
        let mut st = StreamState {
            response_id: String::new(),
            model_name: model_owned.clone(),
            item_id: String::new(),
            content_index: 0,
            output_index: 0,
            has_sent_created: false,
            has_sent_in_progress: false,
            has_sent_output_item: false,
            has_sent_content_part: false,
            text_buffer: String::new(),
            is_completed: false,
            tool_call_ids: std::collections::HashMap::new(),
            tool_call_names: std::collections::HashMap::new(),
            tool_call_args: std::collections::HashMap::new(),
            tool_call_output_index: 1,
            now,
        };

        // ── Main stream processing loop ──
        futures::pin_mut!(upstream_stream);

        while let Some(chunk_result) = upstream_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    warn!("transform_stream_to_responses: upstream stream error: {}", e);
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            };

            let chunk_str = String::from_utf8_lossy(&chunk);
            let lines: Vec<&str> = chunk_str.split('\n').collect();

            for line in &lines {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let data = match line.strip_prefix("data: ") {
                    Some(d) => d.trim(),
                    None => continue,
                };

                if data == "[DONE]" {
                    st.flush_done_events(&emit_data_sse);
                    st.send_completed(&emit_data_sse, &emit_raw);
                    continue;
                }

                let json: serde_json::Value = match serde_json::from_str(data) {
                    Ok(j) => j,
                    Err(_) => continue,
                };

                if st.response_id.is_empty() {
                    st.response_id = if let Some(id) = json.get("id").and_then(|i| i.as_str()) {
                        format!("resp_{}", id.trim_start_matches("chatcmpl-"))
                    } else {
                        format!("resp_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
                    };
                }

                if st.model_name.is_empty() {
                    if let Some(m) = json.get("model").and_then(|m| m.as_str()) {
                        st.model_name = m.to_string();
                    }
                }

                let choices = match json.get("choices").and_then(|c| c.as_array()) {
                    Some(arr) => arr,
                    None => continue,
                };

                for choice in choices {
                    let delta = match choice.get("delta") {
                        Some(d) => d,
                        None => continue,
                    };
                    let finish_reason = choice.get("finish_reason").and_then(|f| f.as_str());

                    // ── Send initial events on first chunk ──
                    if !st.has_sent_created {
                        st.has_sent_created = true;
                        emit_data_sse(&serde_json::json!({
                            "type": "response.created",
                            "response": {
                                "id": st.response_id,
                                "object": "response",
                                "created_at": st.now,
                                "model": st.model_name,
                                "status": "in_progress",
                                "output": []
                            }
                        }));
                    }

                    if !st.has_sent_in_progress {
                        st.has_sent_in_progress = true;
                        emit_data_sse(&serde_json::json!({
                            "type": "response.in_progress",
                            "response": {
                                "id": st.response_id,
                                "object": "response",
                                "created_at": st.now,
                                "model": st.model_name,
                                "status": "in_progress",
                                "output": []
                            }
                        }));
                    }

                    // ── Text content ──
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            if !st.has_sent_output_item {
                                st.has_sent_output_item = true;
                                st.item_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
                                emit_data_sse(&serde_json::json!({
                                    "type": "response.output_item.added",
                                    "output_index": st.output_index,
                                    "item": {
                                        "id": st.item_id,
                                        "type": "message",
                                        "role": "assistant",
                                        "content": []
                                    }
                                }));
                            }
                            if !st.has_sent_content_part {
                                st.has_sent_content_part = true;
                                emit_data_sse(&serde_json::json!({
                                    "type": "response.content_part.added",
                                    "item_id": st.item_id,
                                    "output_index": st.output_index,
                                    "content_index": st.content_index,
                                    "part": {"type": "output_text", "text": ""}
                                }));
                            }
                            st.text_buffer.push_str(content);
                            emit_data_sse(&serde_json::json!({
                                "type": "response.output_text.delta",
                                "delta": content,
                                "item_id": st.item_id,
                                "output_index": st.output_index,
                                "content_index": st.content_index
                            }));
                        }
                    }

                    // ── Reasoning / thinking content ──
                    // Some providers (DeepSeek, Qwen, etc.) send reasoning_content
                    // BEFORE the actual content. We need to ensure the output item
                    // and content part flags are set so flush_done_events works
                    // correctly even when only reasoning_content is sent.
                    if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                        if !reasoning.is_empty() {
                            // Ensure output item and content part are initialized
                            if !st.has_sent_output_item {
                                st.has_sent_output_item = true;
                                st.item_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
                                emit_data_sse(&serde_json::json!({
                                    "type": "response.output_item.added",
                                    "output_index": st.output_index,
                                    "item": {
                                        "id": st.item_id,
                                        "type": "message",
                                        "role": "assistant",
                                        "content": []
                                    }
                                }));
                            }
                            if !st.has_sent_content_part {
                                st.has_sent_content_part = true;
                                emit_data_sse(&serde_json::json!({
                                    "type": "response.content_part.added",
                                    "item_id": st.item_id,
                                    "output_index": st.output_index,
                                    "content_index": st.content_index,
                                    "part": {"type": "output_text", "text": ""}
                                }));
                            }
                            st.text_buffer.push_str(reasoning);
                            emit_data_sse(&serde_json::json!({
                                "type": "response.output_text.delta",
                                "delta": reasoning,
                                "item_id": st.item_id,
                                "output_index": st.output_index,
                                "content_index": st.content_index
                            }));
                        }
                    }

                    // ── Tool calls ──
                    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                        for tc in tool_calls {
                            let tc_index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                            let tc_id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                            let tc_name = tc.get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str()).unwrap_or("").to_string();
                            let tc_args = tc.get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str()).unwrap_or("").to_string();

                            if !st.tool_call_ids.contains_key(&tc_index) && !tc_id.is_empty() {
                                st.tool_call_ids.insert(tc_index, tc_id.clone());
                                st.tool_call_names.insert(tc_index, tc_name.clone());
                                st.tool_call_args.insert(tc_index, String::new());
                                emit_data_sse(&serde_json::json!({
                                    "type": "response.output_item.added",
                                    "output_index": st.tool_call_output_index,
                                    "item": {
                                        "id": tc_id, "type": "function_call",
                                        "call_id": tc_id, "name": tc_name, "arguments": ""
                                    }
                                }));
                            }

                            if let Some(args_buf) = st.tool_call_args.get_mut(&tc_index) {
                                if !tc_args.is_empty() {
                                    args_buf.push_str(&tc_args);
                                    let tc_item_id = st.tool_call_ids.get(&tc_index).cloned().unwrap_or_default();
                                    emit_data_sse(&serde_json::json!({
                                        "type": "response.function_call_arguments.delta",
                                        "delta": tc_args, "item_id": tc_item_id,
                                        "output_index": st.tool_call_output_index
                                    }));
                                }
                            }
                        }
                    }

                    // ── Finish reason ──
                    if let Some(reason) = finish_reason {
                        if reason == "stop" || reason == "length" || reason == "tool_calls" || reason == "content_filter" {
                            st.flush_done_events(&emit_data_sse);
                            st.send_completed(&emit_data_sse, &emit_raw);
                        }
                    }
                }
            }
        }

        // ── Stream ended — flush if not yet completed ──
        if st.has_sent_created && !st.is_completed {
            st.flush_done_events(&emit_data_sse);
            st.send_completed(&emit_data_sse, &emit_raw);
        }
    });

    // Convert the mpsc receiver into a futures::stream::Stream
    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
}

/// Internal state for the stream translator
struct StreamState {
    response_id: String,
    model_name: String,
    item_id: String,
    content_index: u32,
    output_index: u32,
    has_sent_created: bool,
    has_sent_in_progress: bool,
    has_sent_output_item: bool,
    has_sent_content_part: bool,
    text_buffer: String,
    is_completed: bool,
    tool_call_ids: std::collections::HashMap<u32, String>,
    tool_call_names: std::collections::HashMap<u32, String>,
    tool_call_args: std::collections::HashMap<u32, String>,
    tool_call_output_index: u32,
    now: i64,
}

impl StreamState {
    fn flush_done_events(&mut self, emit: &impl Fn(&serde_json::Value)) {
        // Correct event order per Responses API spec:
        // 1. output_text.done → 2. content_part.done → 3. output_item.done
        if self.has_sent_content_part {
            emit(&serde_json::json!({
                "type": "response.output_text.done",
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": self.content_index,
                "text": self.text_buffer
            }));
            emit(&serde_json::json!({
                "type": "response.content_part.done",
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": self.content_index,
                "part": {"type": "output_text", "text": self.text_buffer}
            }));
        }
        if self.has_sent_output_item && self.has_sent_content_part {
            emit(&serde_json::json!({
                "type": "response.output_item.done",
                "output_index": self.output_index,
                "item": {
                    "id": self.item_id, "type": "message", "role": "assistant",
                    "content": [{"type": "output_text", "text": self.text_buffer}]
                }
            }));
        }
        let mut tc_indices: Vec<u32> = self.tool_call_ids.keys().copied().collect();
        tc_indices.sort();
        let mut tc_idx_counter = 0u32;
        for tc_idx in &tc_indices {
            if let Some(tc_id) = self.tool_call_ids.get(tc_idx) {
                let tc_output_idx = if self.has_sent_output_item { 1 + tc_idx_counter } else { tc_idx_counter };
                let tc_name = self.tool_call_names.get(tc_idx).cloned().unwrap_or_default();
                let tc_args = self.tool_call_args.get(tc_idx).cloned().unwrap_or_default();
                emit(&serde_json::json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": tc_id, "output_index": tc_output_idx, "arguments": tc_args
                }));
                emit(&serde_json::json!({
                    "type": "response.output_item.done",
                    "output_index": tc_output_idx,
                    "item": {
                        "id": tc_id, "type": "function_call",
                        "call_id": tc_id, "name": tc_name, "arguments": tc_args
                    }
                }));
                tc_idx_counter += 1;
            }
        }
    }

    fn send_completed(&mut self, emit: &impl Fn(&serde_json::Value), emit_raw: &impl Fn(&str)) {
        if self.is_completed {
            return;
        }
        self.is_completed = true;
        let mut output: Vec<serde_json::Value> = Vec::new();
        if self.has_sent_output_item {
            output.push(serde_json::json!({
                "id": self.item_id, "type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": self.text_buffer}]
            }));
        }
        let mut tc_indices: Vec<u32> = self.tool_call_ids.keys().copied().collect();
        tc_indices.sort();
        for tc_idx in &tc_indices {
            if let Some(tc_id) = self.tool_call_ids.get(tc_idx) {
                let tc_name = self.tool_call_names.get(tc_idx).cloned().unwrap_or_default();
                let tc_args = self.tool_call_args.get(tc_idx).cloned().unwrap_or_default();
                output.push(serde_json::json!({
                    "id": tc_id, "type": "function_call",
                    "call_id": tc_id, "name": tc_name, "arguments": tc_args
                }));
            }
        }
        emit(&serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": self.response_id, "object": "response",
                "created_at": self.now, "model": self.model_name,
                "status": "completed", "output": output
            }
        }));
        emit_raw("[DONE]");
    }
}
