use std::sync::{Mutex, RwLock};
use tokio::sync::watch;
use serde::Serialize;
use reqwest::Client;
use once_cell::sync::Lazy;
use rand::seq::SliceRandom;

use tracing::{info, warn, error};

/// Cached models list response, refreshed every 60 seconds
static MODELS_CACHE: once_cell::sync::Lazy<std::sync::Mutex<Option<(String, std::time::Instant)>>> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));
const MODELS_CACHE_TTL_SECS: u64 = 60;

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

/// Shared HTTP client with 300s timeout for LLM API calls.
/// Wrapped in RwLock so the proxy URL can be changed at runtime.
/// Default: direct connection (no proxy).
static PROXY_CLIENT: Lazy<RwLock<Client>> = Lazy::new(|| {
    RwLock::new(build_http_client(None))
});

/// Build an HTTP client with an optional proxy URL.
/// When `proxy_url` is None or empty, direct connection is used.
fn build_http_client(proxy_url: Option<&str>) -> Client {
    let mut builder = Client::builder()
        .timeout(std::time::Duration::from_secs(300));
    if let Some(url) = proxy_url {
        if !url.is_empty() {
            match reqwest::Proxy::all(url) {
                Ok(proxy) => {
                    info!("Using upstream proxy: {}", url);
                    builder = builder.proxy(proxy);
                }
                Err(e) => {
                    warn!("Invalid proxy URL '{}': {}, falling back to direct connection", url, e);
                }
            }
        }
    }
    builder.build().expect("Failed to create HTTP client")
}

/// Initialize the proxy client from config (called on app startup).
/// Overwrites the current client with one built from the given proxy URL.
pub fn init_proxy_client(proxy_url: Option<String>) {
    if let Ok(mut client) = PROXY_CLIENT.write() {
        *client = build_http_client(proxy_url.as_deref());
    }
}

/// Update the proxy client at runtime with a new proxy URL.
/// When `proxy_url` is None or empty, reverts to direct connection.
/// The proxy server does NOT need to be restarted — the new client is used
/// immediately for subsequent requests.
pub fn update_proxy_client(proxy_url: Option<String>) {
    info!("Updating proxy client, proxy_url={:?}", proxy_url);
    if let Ok(mut client) = PROXY_CLIENT.write() {
        *client = build_http_client(proxy_url.as_deref());
    }
}

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
    axum::Router::new()
        .route("/*path", axum::routing::any(proxy_handler))
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
    // Check cache first
    {
        let cache = MODELS_CACHE.lock().unwrap();
        if let Some((ref cached_body, ref cached_at)) = *cache {
            if cached_at.elapsed().as_secs() < MODELS_CACHE_TTL_SECS {
                return axum::response::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .header("x-cache", "HIT")
                    .body(axum::body::Body::from(cached_body.clone()))
                    .unwrap_or_else(|_| {
                        axum::response::Response::new(axum::body::Body::from("{\"error\":\"internal error\"}"))
                    });
            }
        }
    }
    let mut data: Vec<serde_json::Value> = Vec::new();
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
    let body_str = serde_json::to_string(&response_body).unwrap_or_else(|_| "{\"error\":\"internal error\"}".to_string());
    // Update cache
    {
        let mut cache = MODELS_CACHE.lock().unwrap();
        *cache = Some((body_str.clone(), std::time::Instant::now()));
    }
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("x-cache", "MISS")
        .body(axum::body::Body::from(body_str))
        .unwrap_or_else(|_| {
            axum::response::Response::new(axum::body::Body::from("{\"error\":\"internal error\"}"))
        })
}


/// Handle all incoming proxy requests
async fn proxy_handler(
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
        crate::modules::codex_translator::transform_responses_to_chat_completions(&body_bytes)
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
    let client = PROXY_CLIENT.read().unwrap().clone();
    let result = forward_with_retry(
        client,
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
    client: Client,
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
                    crate::modules::codex_translator::transform_stream_to_responses(resp.bytes_stream(), &model_identifier)
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
        // If translation fails, construct a proper Responses API error response
        // instead of falling back to the raw Chat Completions format (which
        // Codex CLI cannot parse).
        let body_bytes = if is_responses_api {
            match crate::modules::codex_translator::transform_chat_completions_to_responses(&body_bytes) {
                Some(translated) => {
                    info!("Responses API: response body translated ({} bytes → {} bytes)", body_bytes.len(), translated.len());
                    translated.into()
                }
                None => {
                    // Translation failed — construct a proper Responses API error
                    // instead of passing through raw Chat Completions format.
                    warn!("Responses API: failed to translate upstream response, sending error to client");
                    let error_response = serde_json::json!({
                        "id": format!("resp_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
                        "object": "response",
                        "created_at": chrono::Utc::now().timestamp(),
                        "model": model_identifier,
                        "status": "failed",
                        "error": {
                            "code": "response_translation_failed",
                            "message": "Upstream returned an unparseable response. The provider may use an incompatible format."
                        },
                        "output": []
                    });
                    serde_json::to_vec(&error_response).unwrap_or_else(|_| body_bytes.to_vec()).into()
                }
            }
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

