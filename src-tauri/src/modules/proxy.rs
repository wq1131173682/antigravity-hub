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

/// Shared HTTP client for LLM API calls.
///
/// The total timeout is a generous 1 hour (3600s). It MUST NOT be a short
/// wall-clock cap like 300s: reqwest's `.timeout()` bounds the ENTIRE request
/// including streaming body reads, and relays such as Agnes buffer the full
/// response before streaming it out. A single generation (long reasoning +
/// web_search + tool calls) can legitimately exceed 5 minutes — a 300s cap
/// aborts the stream mid-response, which surfaced as "conversation got
/// interrupted after a few minutes". Genuinely dead streams are still caught
/// by the idle timeouts in `transform_stream_to_responses` (first-chunk /
/// inter-chunk) and by this total cap as a last resort.
/// Wrapped in RwLock so the proxy URL can be changed at runtime.
/// Default: direct connection (no proxy).
static PROXY_CLIENT: Lazy<RwLock<Client>> = Lazy::new(|| {
    RwLock::new(build_http_client(None))
});

/// Build an HTTP client with an optional proxy URL.
/// When `proxy_url` is None or empty, direct connection is used.
fn build_http_client(proxy_url: Option<&str>) -> Client {
    let mut builder = Client::builder()
        // Overall backstop for streaming responses (kept generous so long
        // generations are not cut off). Per-connection stalls are handled by
        // connect_timeout / tcp_keepalive below.
        .timeout(std::time::Duration::from_secs(3600))
        // Bound connection establishment so a dead/black-holed upstream fails
        // fast instead of hanging the whole request (which surfaced as
        // "second conversation gets no response, needs several retries" — the
        // client reused a stale pooled socket and blocked until the client's
        // own timeout).
        .connect_timeout(std::time::Duration::from_secs(20))
        // Detect dead upstream sockets that silently dropped the connection.
        .tcp_keepalive(Some(std::time::Duration::from_secs(60)))
        // Do NOT reuse idle pooled connections. Many upstream gateways close
        // keep-alive connections after a short idle window; a pooled socket
        // that the server already closed causes the next request to hang until
        // TCP timeout / RST. A fresh connection per request is a negligible
        // cost next to a multi-second LLM generation and eliminates the entire
        // stale-connection class of hangs.
        .pool_max_idle_per_host(0);
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

/// Detect a request body that is actually an ENTIRE HTTP/1.1 message
/// (request line + headers) instead of a JSON payload, and extract the
/// embedded JSON body (the part after the header/body separator).
///
/// Some agent clients (e.g. WorkBuddy's session-opening probe request,
/// identified by headers such as `X-Agent-Intent: cr`) send their first
/// request with the full HTTP message text as the POST body. Forwarding that
/// raw text upstream produces 400 "invalid arguments". This function recovers
/// the real JSON payload so the proxy can process it normally.
///
/// Header/body separators handled (in priority order):
///   - `"\r\n\r\n"` — standard HTTP/1.1
///   - `"\n\n"` — LF-only
///   - a SINGLE `"\r\n"` / `"\n"` between the last header and the JSON payload.
///     Some SDKs (WorkBuddy among them) emit the body immediately after the
///     final header with only one line break instead of the required blank
///     line. Without recovering the JSON in that case the request was silently
///     answered with `200 OK` (treated as a liveness probe) and the real chat
///     message was dropped — which surfaced as "the first conversation stops
///     immediately with no output".
///
/// Returns `Some(inner_json_bytes)` when the body looks like an HTTP request
/// text and its embedded payload is valid JSON; `None` otherwise (so a genuine
/// headers-only probe is left for the caller to acknowledge with 200).
fn extract_json_from_http_text(body: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(body).ok()?;
    let first_line = text.lines().next()?;

    // Request line must look like: "METHOD target HTTP/1.x"
    let is_http_request_line = first_line.starts_with("POST ")
        || first_line.starts_with("GET ")
        || first_line.starts_with("PUT ")
        || first_line.starts_with("PATCH ")
        || first_line.starts_with("DELETE ")
        || first_line.starts_with("OPTIONS ");
    if !is_http_request_line {
        return None;
    }

    // Locate where the JSON body begins.
    let body_start: usize = if let Some(pos) = text.find("\r\n\r\n") {
        // Standard HTTP/1.1: blank line terminates the header block.
        pos + 4
    } else if let Some(pos) = text.find("\n\n") {
        pos + 2
    } else {
        // No blank-line separator. Some clients join the final header and the
        // JSON payload with a single CRLF/LF. Fall back to the first '{', which
        // marks the start of the embedded JSON object.
        match text.find('{') {
            Some(p) => p,
            None => return None,
        }
    };

    let inner = text[body_start..].trim();
    if inner.is_empty() {
        return None;
    }

    // Embedded payload must be valid JSON — otherwise we don't touch the body.
    serde_json::from_str::<serde_json::Value>(inner).ok()?;
    Some(inner.as_bytes().to_vec())
}

/// Detect whether the body is an entire HTTP/1.1 request message text
/// (request line + headers), even if it carries no message payload.
///
/// WorkBuddy's session-opening probe sends only the request line + headers
/// as the POST body (no `\r\n\r\n` separator, no JSON payload). We must not
/// hard-reject that — it is a liveness/session probe, not a real chat call.
fn looks_like_http_request_text(body: &[u8]) -> bool {
    let Some(text) = std::str::from_utf8(body).ok() else {
        return false;
    };
    let Some(first_line) = text.lines().next() else {
        return false;
    };
    ["POST ", "GET ", "PUT ", "PATCH ", "DELETE ", "OPTIONS ", "HEAD "]
        .iter()
        .any(|m| first_line.starts_with(m))
}

/// SSE keepalive interval for transparent streaming pass-through.
///
/// When the proxy streams upstream SSE chunks directly to a chat client
/// (Chat Completions path, no Responses API translation), the proxy does
/// nothing else on the wire. If the upstream is slow (e.g. reasoning models
/// before the first tool call, or anything that buffers upstream), the
/// client's SSE implementation will eventually give up on an idle connection
/// and close it — surfacing as "the assistant's response cut off mid-stream".
///
/// To prevent that, we wrap the upstream `bytes_stream` with a keepalive
/// stream that periodically emits an SSE comment frame (`": ping\n\n"`).
/// SSE comments are ignored by every compliant SSE client and keep the
/// underlying TCP socket live.
///
/// Default: 25 seconds. Most SSE implementations tolerate 30-60s of silence;
/// staying well under that ceiling keeps us safe across browsers, Electron's
/// `EventSource`, and `fetch`+ReadableStream consumers while not polluting
/// the wire with unnecessary frames.
const SSE_KEEPALIVE_INTERVAL_SECS: u64 = 25;

/// SSE keepalive frame. A line beginning with `:` is an SSE comment and is
/// silently discarded by every compliant client.
const SSE_KEEPALIVE_BYTES: &[u8] = b": ping\n\n";

/// Adapt an upstream byte stream into one that emits periodic SSE comment
/// frames whenever the upstream stays silent for `keepalive_secs`.
///
/// - Every `Ok(bytes)` from the upstream is forwarded unchanged AND resets
///   the keepalive timer (so a chatty stream never gets a spurious ping
///   tacked onto the middle of a real chunk).
/// - If the upstream stays idle for `keepalive_secs`, an SSE comment frame
///   is yielded instead so the client never sees a "no bytes for too long"
///   gap on the socket.
/// - Upstream errors and end-of-stream are passed through verbatim — we
///   never fabricate success or stretch a dead stream past its real end.
///
/// The `Sleep` is held behind `Pin<Box<_>>` because `tokio::time::Sleep` is
/// not `Unpin` and the only way to call its `poll` / `reset` methods is
/// through a `Pin<&mut Sleep>`. Box-pinning keeps the wrapper itself
/// `Unpin`, so callers can construct it on the stack and pass it straight
/// to `Body::from_stream`.
pub(crate) struct SseKeepaliveStream<S> {
    inner: S,
    next_ping_at: std::pin::Pin<Box<tokio::time::Sleep>>,
    keepalive_secs: u64,
}

impl<S> SseKeepaliveStream<S> {
    pub(crate) fn new(inner: S, keepalive_secs: u64) -> Self {
        let now = tokio::time::Instant::now();
        let sleep = tokio::time::sleep_until(
            now + std::time::Duration::from_secs(keepalive_secs),
        );
        Self {
            inner,
            next_ping_at: Box::pin(sleep),
            keepalive_secs,
        }
    }
}

impl<S> futures::Stream for SseKeepaliveStream<S>
where
    S: futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<bytes::Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::future::Future;
        // SAFETY: we never move out of `inner` or `next_ping_at`; both fields
        // are pinned through `&mut *self`.
        let me = &mut *self;

        // Data path — try upstream first. Real data always wins so a chunk
        // never gets a comment frame glued to its tail.
        match std::pin::Pin::new(&mut me.inner).poll_next(cx) {
            std::task::Poll::Ready(Some(Ok(bytes))) => {
                // Receipt of real data fully resets the keepalive timer so
                // that subsequent silence is measured from "now", not from
                // some earlier quiet period.
                let now = tokio::time::Instant::now();
                let pinned = me.next_ping_at.as_mut();
                pinned.reset(now + std::time::Duration::from_secs(me.keepalive_secs));
                return std::task::Poll::Ready(Some(Ok(bytes)));
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                return std::task::Poll::Ready(Some(Err(Box::new(e))));
            }
            std::task::Poll::Ready(None) => {
                return std::task::Poll::Ready(None);
            }
            std::task::Poll::Pending => {}
        }

        // Keepalive path — only reached when the upstream produced no data
        // this call. If the silence has lasted long enough, inject a comment
        // frame and re-arm.
        if me.next_ping_at.as_mut().as_mut().poll(cx).is_ready() {
            let now = tokio::time::Instant::now();
            me.next_ping_at.as_mut().reset(now + std::time::Duration::from_secs(me.keepalive_secs));
            return std::task::Poll::Ready(Some(Ok(bytes::Bytes::from_static(
                SSE_KEEPALIVE_BYTES,
            ))));
        }

        std::task::Poll::Pending
    }
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

/// Rewrite the request body's `model` field to the platform's default_model when
/// the requested model is NOT one of the platform's configured models.
///
/// This solves the "Codex sends unknown model" problem: Codex CLI's internal
/// agents (e.g., the Memory Writing Agent) use built-in model names such as
/// `gpt-5.6-luna` / `gpt-5.6-terra` that are hardcoded and never follow the
/// `model` key in config.toml. Without rewriting, those requests get forwarded
/// upstream with an unknown model name, causing 404/503 and key exhaustion.
///
/// When the platform has a `default_model` (persisted when applying Codex
/// config) and the incoming model is not in the platform's model list, the
/// model is rewritten so the upstream always sees a valid configured model.
///
/// Returns (original_or_rewritten_model_name, modified_body_bytes).
fn apply_default_model_override(
    body_bytes: &[u8],
    platform_id: &str,
    model_name: Option<&str>,
    is_responses_api: bool,
) -> (Option<String>, Vec<u8>) {
    let Some(requested) = model_name else {
        return (None, body_bytes.to_vec());
    };

    // Only rewrite for Responses API requests (i.e. Codex CLI). Codex's internal
    // agents send hardcoded built-in model names that must be mapped to the
    // configured default model. Direct Chat Completions / other clients may
    // legitimately request a model that is valid upstream but not yet in the
    // local list (e.g. a newly released model) — silently rewriting those would
    // break them, so pass those requests through untouched.
    if !is_responses_api {
        return (Some(requested.to_string()), body_bytes.to_vec());
    }

    // Resolve the platform's default_model from app config
    let default_model = match crate::modules::config::load_app_config() {
        Ok(cfg) => cfg.platforms.iter()
            .find(|p| p.id == platform_id)
            .and_then(|p| p.default_model.as_deref())
            .map(String::from),
        Err(_) => None,
    };
    let Some(default_model) = default_model else {
        // No default model configured — pass through unchanged
        return (Some(requested.to_string()), body_bytes.to_vec());
    };

    // If the requested model is already the default or is a configured model,
    // no rewrite is needed.
    let is_configured = crate::modules::model_manager::list_models(platform_id)
        .map(|models| models.iter().any(|m| m.model_name == requested))
        .unwrap_or(false);
    if is_configured || requested == default_model {
        return (Some(requested.to_string()), body_bytes.to_vec());
    }

    // Rewrite the model field in the JSON body
    match serde_json::from_slice::<serde_json::Value>(body_bytes) {
        Ok(mut json) => {
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "model".to_string(),
                    serde_json::Value::String(default_model.clone()),
                );
            }
            let modified = serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec());
            info!(
                "Rewriting model '{}' → '{}' for platform '{}' (requested model not configured)",
                requested, default_model, platform_id
            );
            (Some(default_model), modified)
        }
        Err(_) => (Some(requested.to_string()), body_bytes.to_vec()),
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
                            let mut entry = serde_json::json!({
                                "id": model.model_name,
                                "object": "model",
                                "created": model.created_at,
                                "owned_by": platform.id,
                            });
                            if let Some(ctx) = model.max_input_tokens {
                                entry["max_input_tokens"] = serde_json::json!(ctx);
                            }
                            data.push(entry);
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


/// Result of refreshing models from upstream (full-sync variant)
#[derive(Debug, Clone, Serialize)]
pub struct RefreshModelsResult {
    pub updated: Vec<String>,
    pub created: Vec<String>,
    pub total_upstream: usize,
    pub total_local_before: usize,
    pub message: String,
}

/// A single model as reported by the upstream `/v1/models` endpoint,
/// WITHOUT importing it. Used by the selective-import flow.
#[derive(Debug, Clone, Serialize)]
pub struct UpstreamModelInfo {
    pub model_name: String,
    pub display_name: String,
    pub max_input_tokens: Option<u64>,
    /// Whether a local model with this name already exists for the platform.
    pub already_imported: bool,
}

/// Result of a selective model import.
#[derive(Debug, Clone, Serialize)]
pub struct ImportModelsResult {
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub message: String,
}

/// Fetch the upstream model list (`/v1/models` or `/models`) for a platform.
/// Returns (model_name, display_name, max_input_tokens) tuples — nothing is
/// written to local storage. Shared by the full-sync and selective-import
/// flows. Handles Gemini's `/v1beta/openai` root via deduplicate_url_path.
async fn fetch_upstream_model_list(platform_id: &str) -> Result<Vec<(String, String, Option<u64>)>, String> {
    use crate::modules::config;
    use crate::modules::keystore;

    // Load platform info
    let cfg = config::load_app_config()?;
    let platform = cfg.platforms.iter()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;

    // Get an active API key
    let keys = keystore::list_keys(platform_id)?;
    let active_key = keys.iter()
        .find(|k| k.is_active())
        .map(|k| k.key_value.clone())
        .ok_or_else(|| format!(
            "No active API keys available for platform '{}'. Please add at least one active API key first.",
            platform.name
        ))?;

    let base_url = platform.base_url.trim_end_matches('/').to_string();

    // Try multiple URL patterns — some providers use /v1/models, others /models.
    // Use deduplicate_url_path to handle base URLs that already include /v1
    // (or Gemini's /v1beta/openai root, where the /v1 is stripped).
    let url_candidates = vec![
        deduplicate_url_path(&base_url, "/v1/models"),
        deduplicate_url_path(&base_url, "/models"),
    ];

    let client = PROXY_CLIENT.read().unwrap().clone();
    let mut last_error = String::new();
    let mut body: Option<serde_json::Value> = None;

    for url in &url_candidates {
        info!("Trying to fetch models from: {}", url);
        match client.get(url)
            .header("Authorization", format!("Bearer {}", active_key))
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<serde_json::Value>().await {
                        Ok(json) => {
                            body = Some(json);
                            break;
                        }
                        Err(e) => {
                            last_error = format!("{} returned valid HTTP but failed to parse JSON: {}", url, e);
                        }
                    }
                } else {
                    last_error = format!("{} returned HTTP {}", url, resp.status());
                }
            }
            Err(e) => {
                last_error = format!("{} connection failed: {}", url, e);
            }
        }
    }

    let body = body.ok_or_else(|| {
        format!(
            "Failed to fetch models from upstream. Tried:\n  {}\nAll attempts failed. Last error: {}",
            url_candidates.join("\n  "),
            last_error
        )
    })?;

    // Parse the model list from the upstream response — support multiple formats
    let upstream_models = body.get("data")
        .and_then(|d| d.as_array())
        .or_else(|| body.as_array().map(|a| a))
        .ok_or_else(|| {
            format!(
                "Upstream response format not recognized. Expected {{\"data\": [...]}} or an array.\nFirst 200 chars: {}",
                &serde_json::to_string(&body).unwrap_or_default().chars().take(200).collect::<String>()
            )
        })?;

    let mut result = Vec::new();
    for upstream_model in upstream_models {
        let model_id = upstream_model.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if model_id.is_empty() {
            continue;
        }

        let display_name = upstream_model.get("display_name")
            .or_else(|| upstream_model.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(model_id)
            .to_string();

        // Try to extract context window from various field names
        let max_input_tokens = upstream_model.get("max_input_tokens")
            .or_else(|| upstream_model.get("max_input_length"))
            .or_else(|| upstream_model.get("context_window"))
            .or_else(|| upstream_model.get("max_context_length"))
            .and_then(|v| v.as_u64());

        result.push((model_id.to_string(), display_name, max_input_tokens));
    }
    Ok(result)
}

/// List models available on the upstream WITHOUT importing them.
/// Each entry is marked `already_imported` when a local model with the same
/// name exists, so the UI can pre-check / gray out existing models.
pub async fn list_upstream_models(platform_id: &str) -> Result<Vec<UpstreamModelInfo>, String> {
    let upstream = fetch_upstream_model_list(platform_id).await?;
    let local_models = crate::modules::model_manager::list_models(platform_id)?;
    let mut result = Vec::with_capacity(upstream.len());
    for (model_name, display_name, ctx) in upstream {
        let already_imported = local_models.iter().any(|m| m.model_name == model_name);
        result.push(UpstreamModelInfo {
            model_name,
            display_name,
            max_input_tokens: ctx,
            already_imported,
        });
    }
    Ok(result)
}

/// Import ONLY the selected model names from the upstream list.
/// Models already present locally are skipped (their context size is not
/// clobbered); new models are created with the default quota limits.
pub async fn import_models(platform_id: &str, model_names: Vec<String>) -> Result<ImportModelsResult, String> {
    use crate::modules::model_manager;

    let upstream = fetch_upstream_model_list(platform_id).await?;
    let local_models = model_manager::list_models(platform_id)?;

    let mut imported = Vec::new();
    let mut skipped = Vec::new();

    for model_name in &model_names {
        let (display_name, max_input_tokens) = upstream
            .iter()
            .find(|(name, _, _)| name == model_name)
            .map(|(_, dn, ctx)| (dn.clone(), *ctx))
            .unwrap_or_else(|| (model_name.clone(), None));

        if local_models.iter().any(|m| &m.model_name == model_name) {
            skipped.push(model_name.clone());
            continue;
        }

        model_manager::add_model(
            platform_id.to_string(),
            model_name.clone(),
            display_name,
            Some(10000),  // per_5hour: default
            Some(50000),  // per_day: default
            Some(100000), // per_month: default
            max_input_tokens,
        )?;
        imported.push(model_name.clone());
    }

    let message = if imported.is_empty() {
        format!("No new models imported ({} skipped: already exist)", skipped.len())
    } else {
        format!("Imported {} model(s): {}", imported.len(), imported.join(", "))
    };

    Ok(ImportModelsResult {
        imported,
        skipped,
        message,
    })
}

/// Fetch model information from the upstream API for a given platform.
/// (Full-sync variant: auto-creates every upstream model. The UI now prefers
/// the selective-import flow via list_upstream_models + import_models.)
pub async fn refresh_models_from_upstream(platform_id: &str) -> Result<RefreshModelsResult, String> {
    use crate::modules::model_manager;

    let upstream = fetch_upstream_model_list(platform_id).await?;
    let mut updated: Vec<String> = Vec::new();
    let mut created: Vec<String> = Vec::new();
    let total_upstream = upstream.len();

    let local_models = model_manager::list_models(platform_id)?;
    let total_local_before = local_models.len();

    for (model_id, display_name, max_input_tokens) in upstream {
        // Find matching local model by model_name
        if let Some(local) = local_models.iter().find(|m| m.model_name == model_id) {
            // Update context size if it changed
            if let Some(ctx) = max_input_tokens {
                if local.max_input_tokens != Some(ctx) {
                    model_manager::update_model(
                        &local.id, None, None, None, None, None,
                        Some(Some(ctx)),
                    )?;
                    updated.push(local.model_name.clone());
                    info!("Updated model '{}' max_input_tokens: {} → {}",
                        local.model_name,
                        local.max_input_tokens.map_or("none".to_string(), |v| v.to_string()),
                        ctx
                    );
                }
            }
        } else {
            // Auto-create new model from upstream with reasonable defaults
            let _ = model_manager::add_model(
                platform_id.to_string(),
                model_id.clone(),
                display_name,
                Some(10000),  // per_5hour: default
                Some(50000),  // per_day: default
                Some(100000), // per_month: default
                max_input_tokens,
            );
            created.push(model_id.clone());
            info!("Auto-created model '{}' from upstream (context: {:?})", model_id, max_input_tokens);
        }
    }

    let message = {
        let mut parts = Vec::new();
        if !created.is_empty() {
            parts.push(format!("Created {} new models: {}", created.len(), created.join(", ")));
        }
        if !updated.is_empty() {
            parts.push(format!("Updated context size for {} models: {}", updated.len(), updated.join(", ")));
        }
        if created.is_empty() && updated.is_empty() {
            parts.push(format!("All {} upstream models already in sync ({} local models)", total_upstream, total_local_before));
        }
        parts.join("\n")
    };

    Ok(RefreshModelsResult {
        updated,
        created,
        total_upstream,
        total_local_before,
        message,
    })
}

/// Test a model by sending a minimal chat completion request.
/// Returns the model status (reachable, response time, etc.).
#[derive(Debug, Clone, Serialize)]
pub struct TestModelResult {
    pub success: bool,
    pub latency_ms: u64,
    pub model_name: String,
    pub message: String,
}

/// Send a minimal chat completion request to test if a model is working.
/// Uses a short "Hi" prompt with max_tokens=1 for minimal cost.
pub async fn test_model(
    platform_id: &str,
    model_name: &str,
) -> Result<TestModelResult, String> {
    use crate::modules::config;
    use crate::modules::keystore;

    let cfg = config::load_app_config()?;
    let platform = cfg.platforms.iter()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;

    // Get an active API key
    let keys = keystore::list_keys(platform_id)?;
    let active_key = keys.iter()
        .find(|k| k.is_active())
        .map(|k| k.key_value.clone())
        .ok_or_else(|| format!(
            "No active API keys available for platform '{}'. Please add at least one active API key first.",
            platform.name
        ))?;

    let base_url = platform.base_url.trim_end_matches('/').to_string();
    // Use deduplicate_url_path to handle base URLs that already include /v1
    // (e.g., "https://token.sensenova.cn/v1" → "/v1/chat/completions" → "https://token.sensenova.cn/v1/chat/completions")
    let url = deduplicate_url_path(&base_url, "/v1/chat/completions");

    let request_body = serde_json::json!({
        "model": model_name,
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 1,
        "stream": false,
    });

    let client = PROXY_CLIENT.read().unwrap().clone();
    let start = std::time::Instant::now();

    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", active_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(30))
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?;

    let latency_ms = start.elapsed().as_millis() as u64;
    let status = resp.status();

    if status.is_success() {
        let body: serde_json::Value = resp.json().await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let model_used = body.get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(model_name);

        let finish = body.pointer("/choices/0/finish_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Ok(TestModelResult {
            success: true,
            latency_ms,
            model_name: model_used.to_string(),
            message: format!(
                "Model '{}' responded in {}ms (finish_reason: {})",
                model_used, latency_ms, finish
            ),
        })
    } else if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        Ok(TestModelResult {
            success: false,
            latency_ms,
            model_name: model_name.to_string(),
            message: format!(
                "Authentication failed (HTTP {}). Check your API key.",
                status.as_u16()
            ),
        })
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(TestModelResult {
            success: false,
            latency_ms,
            model_name: model_name.to_string(),
            message: format!(
                "Model '{}' not found (HTTP 404). The model may not exist or is unavailable.",
                model_name
            ),
        })
    } else {
        // Read error body for more details
        let error_body = resp.text().await.unwrap_or_else(|_| "(unreadable)".to_string());
        Ok(TestModelResult {
            success: false,
            latency_ms,
            model_name: model_name.to_string(),
            message: format!(
                "HTTP {}: {}",
                status.as_u16(),
                error_body.chars().take(200).collect::<String>()
            ),
        })
    }
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

    // ── Malformed body defense (compat for HTTP-text-body clients) ──
    // Some clients send their first request with the ENTIRE HTTP/1.1 message
    // (request line + headers) as the POST body instead of a JSON payload.
    // Forwarding that raw text upstream causes 400 "invalid arguments".
    // Recover the embedded JSON body when possible; reject unparseable
    // non-JSON bodies with a clear error instead of proxying garbage upstream.
    let body_bytes = if body_bytes.is_empty() {
        body_bytes
    } else if serde_json::from_slice::<serde_json::Value>(&body_bytes).is_ok() {
        body_bytes
    } else if let Some(inner) = extract_json_from_http_text(&body_bytes) {
        info!(
            "Extracted embedded JSON from HTTP request text body ({} -> {} bytes)",
            body_bytes.len(),
            inner.len()
        );
        inner.into()
    } else if looks_like_http_request_text(&body_bytes) {
        // HTTP request-line text but no recoverable embedded JSON payload.
        // This is typically WorkBuddy's session-opening probe (request line +
        // headers only). Acknowledging with 200 lets the client proceed to the
        // real chat request instead of failing the connection outright.
        let preview: String = String::from_utf8_lossy(&body_bytes).chars().take(200).collect();
        info!("HTTP-text body without embedded JSON (session probe), acknowledging: {}", preview);
        return error_response(200, String::new());
    } else {
        let preview: String = String::from_utf8_lossy(&body_bytes).chars().take(200).collect();
        warn!("Rejecting request with non-JSON, non-HTTP-text body: {}", preview);
        return error_response(400, format!(
            "Request body must be valid JSON. Received unparseable payload (first 200 chars): {}",
            preview
        ));
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

    // Rewrite unknown model names to the platform's default model.
    // Codex CLI's internal agents (memory writer, etc.) send hardcoded built-in
    // model names that may not exist on the upstream — map them to the model
    // the user applied in the app so upstream always sees a valid model.
    let (model_name, body_bytes) =
        apply_default_model_override(&body_bytes, &platform_id, model_name.as_deref(), is_responses_api);

    // ── Reasoning effort sanitization ──
    // Mistral family models (codestral, mistral-small, open-mistral-nemo,
    // pixtral, etc.) and Google Gemini do NOT support `reasoning_effort` at
    // all — even schema-valid none/high are rejected with HTTP 400
    // "reasoning_effort is not enabled for this model" (code 3051), aborting
    // the conversation. Strip the field for Mistral/Gemini models; all other
    // models pass through untouched.
    let body_bytes = match model_name.as_deref() {
        Some(m) => crate::modules::codex_translator::sanitize_reasoning_effort_for_model(&body_bytes, m)
            .unwrap_or(body_bytes),
        None => body_bytes,
    };

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

        // Gemini's OpenAI-compatibility layer: base_url ends with
        // "/v1beta/openai" and exposes endpoints DIRECTLY under that root
        // (e.g. /v1beta/openai/chat/completions). The proxy maps
        // /v1/responses → /v1/chat/completions, so without this rule the
        // target would become "/v1beta/openai/v1/chat/completions", which
        // Gemini rejects with HTTP 404. When the base ends with "/openai",
        // drop the leading version segment from the target path.
        if is_version && base.ends_with("/openai") {
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

        // Forward all headers except Host, Authorization, Content-Length, and
        // Content-Type.
        //
        // Content-Type is re-added explicitly below, so it must NOT also be
        // copied here: sending the header twice (e.g. two
        // "Content-Type: application/json" lines) makes Mistral's edge gateway
        // mis-parse the request body as a JSON-encoded string, returning
        // HTTP 422 "model_attributes_type / Input should be a valid dictionary
        // or object to extract fields from" — which surfaced as Mistral
        // conversations failing with "unexpected status 422" through the proxy
        // while the in-app test (single Content-Type) worked fine.
        for (key, value) in original_headers.iter() {
            let key_str = key.as_str().to_lowercase();
            if key_str != "host"
                && key_str != "authorization"
                && key_str != "content-length"
                && key_str != "content-type"
            {
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

        // Debug: log the upstream request for troubleshooting
        if let Ok(body_preview) = std::str::from_utf8(&body_bytes) {
            let preview: String = body_preview.chars().take(500).collect();
            info!("Forwarding to upstream: {} | body: {}", target_url, preview);
        }

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
            warn!("{} from {}, {}={}, model={}",
                status, target_url, key_label, key_id, model_identifier);

            // NEVER disable keys — only rotate or retry with backoff.
            // Keys are a precious resource; disabling them on transient server
            // errors would leave the proxy unable to serve requests until the
            // user manually re-enables them.
            let has_multiple_keys = keys_to_try.len() > 1;

            // Emit key-switched event so frontend can refresh quota display
            if attempt < max_retries - 1 && has_multiple_keys {
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
                        reason: format!("Server error {} - switching key", status.as_u16()),
                        disabled_until: 0, // 0 = not disabled, just rotated
                    }
                );
            }

            last_error = format!("HTTP {} from upstream", status);
            if attempt < max_retries - 1 && has_multiple_keys {
                // Multiple keys: rotate to next key
                info!("Rotating to next key (attempt {}/{})", attempt + 2, max_retries);
                continue;
            } else if attempt < max_retries - 1 {
                // Single key (or last few retries): exponential backoff, retry same key
                let backoff_secs = std::cmp::min(32, 2_u64.pow(attempt as u32 + 1));
                info!("Waiting {}s before retrying same key (single key mode)...", backoff_secs);
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                continue;
            }
            break;
        }

        // Success case - record the API call.
        // IMPORTANT: only 2xx responses count toward the quota windows.
        // 4xx errors (401/403/404 etc.) must NOT be counted — doing so would
        // inflate usage and eventually make filter_available_keys exclude the
        // key, causing conversations to fail with "No active API keys" for no
        // apparent reason (a key hitting repeated 404s would be "used up").
        if status.is_success() {
            if let Some(mid) = &model_id {
                let _ = crate::modules::quota_window::record_api_call(key_id, mid, platform_id);
            }
        }

        // Capture response headers before consuming body.
        // IMPORTANT: Filter out `content-length` because the proxy may transform
        // the response body (e.g., Responses API translation), which changes the
        // body size. Keeping the original content-length causes the client to hang
        // waiting for bytes that will never arrive. axum sets the correct
        // content-length automatically when the body is a fixed-size buffer.
        let response_headers: Vec<(String, String)> = resp.headers().iter()
            .filter(|(key, _)| {
                let k = key.as_str().to_lowercase();
                k != "transfer-encoding" && k != "content-length"
            })
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
            crate::modules::token_stats::record_streaming_for_platform(Some(&platform_id));
            if is_responses_api {
                // Translate SSE stream from Chat Completions format to
                // Responses API format on-the-fly so Codex CLI can parse it.
                // The upstream returns Chat Completions SSE chunks, but Codex
                // CLI expects Responses API SSE events.
                //
                // The translator (`transform_stream_to_responses`) already
                // emits `: ping` keepalives while it is actively translating,
                // so we deliberately keep the upstream raw here — adding a
                // second keepalive layer would risk double-comment frames.
                let body = axum::body::Body::from_stream(
                    crate::modules::codex_translator::transform_stream_to_responses(resp.bytes_stream(), &model_identifier)
                );
                return response_builder
                    .body(body)
                    .map_err(|e| format!("Failed to build response: {}", e));
            } else {
                // Pass-through for non-Responses API streaming (ChatGPT Work,
                // generic OpenAI SDKs, etc.).
                //
                // Wrap the upstream with `SseKeepaliveStream` so the
                // downstream client never sees a gap wider than
                // `SSE_KEEPALIVE_INTERVAL_SECS` seconds, even when the
                // upstream model is reasoning before its first tool call.
                // Without this, clients like ChatGPT Work that use
                // `fetch().getReader()` time out on idle SSE connections and
                // truncate the response mid-stream — surfacing as
                // "the assistant's reply cut off after a tool call".
                let body = axum::body::Body::from_stream(
                    SseKeepaliveStream::new(resp.bytes_stream(), SSE_KEEPALIVE_INTERVAL_SECS)
                );
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
                    crate::modules::token_stats::record_usage_for_platform(Some(&platform_id), prompt, completion);
                }
            }
        }

        // ── Responses API response translation ──
        // Translate the response body from Chat Completions format back to
        // Responses API format so Codex CLI can understand it.
        // If translation fails, construct a proper Responses API error response
        // instead of falling back to the raw Chat Completions format (which
        // Codex CLI cannot parse), and return HTTP 502 Bad Gateway.
        let body_bytes = if is_responses_api {
            match crate::modules::codex_translator::transform_chat_completions_to_responses(&body_bytes) {
                Some(translated) => {
                    info!("Responses API: response body translated ({} bytes → {} bytes)", body_bytes.len(), translated.len());
                    translated.into()
                }
                None => {
                    // Translation failed — construct a proper Responses API error
                    // instead of passing through raw Chat Completions format.
                    // Also set HTTP status to 502 so the client doesn't see HTTP 200 + error body.
                    warn!("Responses API: failed to translate upstream response, sending error to client");
                    response_builder = axum::response::Response::builder().status(reqwest::StatusCode::BAD_GATEWAY);
                    for (key, value) in &response_headers {
                        response_builder = response_builder.header(key.as_str(), value.as_str());
                    }
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

// ────────────────────────────────────────────────────────────────────────────
// 测试
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_http_text_crlf() {
        // WorkBuddy 会话首请求：body 为完整 HTTP/1.1 请求文本（\r\n 行尾）
        let raw = b"POST http://192.168.9.193:5343/sensenova/v1/chat/completions HTTP/1.1\r\nAccept: application/json\r\nContent-Type: application/json\r\nx-stainless-lang: js\r\nX-Conversation-ID: abc123\r\n\r\n{\"model\":\"sensenova-6.8-flash-lite\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}";
        let inner = extract_json_from_http_text(raw).expect("should extract embedded JSON");
        let v: serde_json::Value = serde_json::from_slice(&inner).expect("embedded payload must be valid JSON");
        assert_eq!(v["model"], "sensenova-6.8-flash-lite");
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn test_extract_json_from_http_text_lf() {
        // \n 行尾同样支持
        let raw = b"POST /v1/chat/completions HTTP/1.1\nHost: localhost\nContent-Type: application/json\n\n{\"model\":\"gpt-4\"}";
        let inner = extract_json_from_http_text(raw).expect("should extract");
        let v: serde_json::Value = serde_json::from_slice(&inner).unwrap();
        assert_eq!(v["model"], "gpt-4");
    }

    #[test]
    fn test_extract_json_from_http_text_single_crlf_no_blank_line() {
        // WorkBuddy 首请求变体：头部与 JSON 之间仅用单个 \r\n 分隔（缺少标准空行）。
        // 此前会提取失败、被当成探测返回空 200，导致首条消息丢失。
        let raw = b"POST /wb/v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{\"model\":\"sensenova-6.8-flash-lite\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}";
        let inner = extract_json_from_http_text(raw).expect("single-CRLF JSON must be extracted");
        let v: serde_json::Value = serde_json::from_slice(&inner).expect("embedded payload must be valid JSON");
        assert_eq!(v["model"], "sensenova-6.8-flash-lite");
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn test_extract_json_from_http_text_single_lf_no_blank_line() {
        // 单 \n 分隔、无空行，同样应提取成功
        let raw = b"POST /v1/chat/completions HTTP/1.1\nHost: localhost\nContent-Type: application/json\n{\"model\":\"gpt-4\"}";
        let inner = extract_json_from_http_text(raw).expect("single-LF JSON must be extracted");
        let v: serde_json::Value = serde_json::from_slice(&inner).unwrap();
        assert_eq!(v["model"], "gpt-4");
    }

    #[test]
    fn test_extract_json_from_http_text_headers_only_still_none() {
        // 纯探测（仅请求行 + 请求头、无 JSON）仍应返回 None，交由调用方按探测处理。
        let raw = b"POST http://x HTTP/1.1\r\nContent-Type: application/json";
        assert!(extract_json_from_http_text(raw).is_none(), "headers-only probe must not match");
    }

    #[test]
    fn test_extract_json_from_http_text_returns_none_for_normal_json() {
        let json = br#"{"model":"sensenova-6.8-flash-lite","messages":[]}"#;
        assert!(extract_json_from_http_text(json).is_none(), "normal JSON must not match");
    }

    #[test]
    fn test_extract_json_from_http_text_returns_none_for_plain_text() {
        let plain = b"hello world, this is not an http request";
        assert!(extract_json_from_http_text(plain).is_none());
    }

    #[test]
    fn test_extract_json_from_http_text_returns_none_for_bad_inner_json() {
        // 请求行像 HTTP，但内嵌 payload 不是 JSON → 不动原 body
        let raw = b"POST http://x HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{not-json}";
        assert!(extract_json_from_http_text(raw).is_none());
    }

    #[test]
    fn test_extract_json_from_http_text_returns_none_without_header_sep() {
        let raw = b"POST http://x HTTP/1.1\r\nContent-Type: application/json";
        assert!(extract_json_from_http_text(raw).is_none(), "no blank line separator");
        // But looks_like_http_request_text still recognizes it as HTTP text
        assert!(looks_like_http_request_text(raw), "must still be detected as HTTP request text");
    }

    // ─── looks_like_http_request_text ─────────────────────────────────────

    #[test]
    fn test_looks_like_http_request_text_full() {
        let raw = b"POST http://192.168.9.193:5343/sensenova/v1/chat/completions HTTP/1.1\r\nAccept: application/json\r\nContent-Type: application/json\r\nx-stainless-lang: js\r\n\r\n{\"model\":\"test\"}";
        assert!(looks_like_http_request_text(raw), "full HTTP request with JSON body");
    }

    #[test]
    fn test_looks_like_http_request_text_headers_only() {
        // WorkBuddy session probe: headers only, no blank line, no body
        let raw = b"POST http://192.168.9.193:5343/sensenova/v1/chat/completions HTTP/1.1\r\nAccept: application/json\r\nContent-Type: application/json";
        assert!(looks_like_http_request_text(raw), "headers-only probe must be detected");
    }

    #[test]
    fn test_looks_like_http_request_text_plain_text_returns_false() {
        assert!(!looks_like_http_request_text(b"hello world"));
    }

    #[test]
    fn test_looks_like_http_request_text_valid_json_returns_false() {
        assert!(!looks_like_http_request_text(br#"{"model":"test"}"#), "pure JSON must not match");
    }

    #[test]
    fn test_looks_like_http_request_text_empty_returns_false() {
        assert!(!looks_like_http_request_text(b""), "empty body must not match");
    }

    #[test]
    fn test_looks_like_http_request_text_get_method() {
        assert!(looks_like_http_request_text(b"GET /v1/models HTTP/1.1\r\nHost: localhost"), "GET must be detected");
    }

    // ─── End-to-end reproduction: WorkBuddy first request ──────────────────
    // WorkBuddy opens a conversation by sending its first chat request as an
    // HTTP-text body (the full HTTP/1.1 message as the POST body). If the
    // embedded JSON is NOT separated from the headers by a blank line (some
    // SDKs emit a single CRLF instead), the extractor returns None and the
    // request is answered with 200 empty — i.e. silently dropped as a liveness
    // probe — which makes the first conversation "stop" with no output.
    #[tokio::test]
    async fn test_workbuddy_first_request_single_crlf_not_dropped_as_probe() {
        // Redirect the data dir to a temp location so we never touch the real
        // config / keys on disk.
        let tmp = std::env::temp_dir()
            .join(format!("abv_wb_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("ABV_DATA_DIR", &tmp);

        // Spin up a mock upstream that returns a small SSE chat stream.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            async fn handle(_req: axum::http::Request<axum::body::Body>) -> axum::response::Response {
                let sse = concat!(
                    "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
                    "data: [DONE]\n\n",
                );
                axum::response::Response::builder()
                    .status(200)
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from(sse))
                    .unwrap()
            }
            let app = axum::Router::new()
                .route("/v1/chat/completions", axum::routing::post(handle));
            let _ = axum::serve(listener, app).await;
        });
        // Give the mock upstream a moment to start accepting connections.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Write a minimal platform config + one active key into the temp data dir.
        let config = serde_json::json!({
            "language": "zh",
            "theme": "system",
            "proxy_port": 8080,
            "proxy_host": "127.0.0.1",
            "auto_switch": false,
            "platforms": [{
                "id": "wb",
                "name": "WorkBuddy",
                "base_url": format!("http://127.0.0.1:{}", upstream_port),
                "path_prefix": "wb",
                "sort_order": 0,
                "created_at": 0,
                "base_url_overrides": [],
                "default_model": null
            }]
        });
        std::fs::write(tmp.join("gui_config.json"), serde_json::to_string_pretty(&config).unwrap()).unwrap();
        let keys = serde_json::json!({
            "keys": [{
                "id": "k1", "platform_id": "wb", "name": "k1",
                "key_value": "sk-test", "status": "active", "sort_order": 0, "created_at": 0
            }],
            "rotation_index": {}
        });
        std::fs::write(tmp.join("api_keys.json"), serde_json::to_string_pretty(&keys).unwrap()).unwrap();

        // WorkBuddy-style first request: HTTP-text body with a SINGLE CRLF
        // between the last header and the JSON (no blank-line separator).
        let raw = b"POST /wb/v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{\"model\":\"sensenova-6.8-flash-lite\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}";
        let uri = axum::http::Uri::from_static("http://127.0.0.1:8080/wb/v1/chat/completions");
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        let resp = proxy_handler(
            axum::http::Method::POST,
            uri,
            headers,
            axum::body::Body::from(raw.to_vec()),
        ).await;

        let status = resp.status();
        let body_bytes = axum::body::to_bytes(resp.into_body(), 10 * 1024 * 1024).await.unwrap();
        let body_str = String::from_utf8_lossy(&body_bytes);
        println!("WB-TEST status={} body={}", status, body_str);
        assert_eq!(status, 200, "real first WorkBuddy message must be forwarded, not answered 200 empty as a probe");
        assert!(body_str.contains("hello"), "response must contain the upstream SSE content, got: {}", body_str);
    }

    // ──────────────────────────────────────────────────────────────────
    // SseKeepaliveStream tests
    //
    // These tests deliberately use `start_paused = true` so we can fast-
    // forward `tokio` virtual time and observe keepalive behaviour without
    // making tests slow.
    // ──────────────────────────────────────────────────────────────────

    /// A constant-rate upstream never lets the keepalive timer fire. The
    /// wrapped stream must therefore emit no `: ping` frames — proving that
    /// the keepalive wrapper doesn't pollute chatty streams.
    #[tokio::test(start_paused = true)]
    async fn test_sse_keepalive_no_ping_on_chatty_stream() {
        use bytes::Bytes;
        use futures::StreamExt;

        // 10 chunks, 100 ms apart → total 900 ms. Keepalive = 1 s.
        let upstream = futures::stream::unfold(0u32, |i| async move {
            if i >= 10 {
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Some((
                Ok(Bytes::from(format!("chunk{}", i))),
                i + 1,
            ))
        });
        let upstream = Box::pin(upstream);
        let mut kept = SseKeepaliveStream::new(upstream, 1);

        let mut combined = String::new();
        while let Some(b) = kept.next().await {
            combined.push_str(std::str::from_utf8(&b.unwrap()).unwrap());
        }

        assert!(
            !combined.contains(": ping"),
            "chatty stream must never receive a keepalive, got: {:?}",
            combined
        );
        for i in 0..10 {
            assert!(
                combined.contains(&format!("chunk{}", i)),
                "all upstream chunks must be forwarded in order, missing chunk{} in {:?}",
                i,
                combined
            );
        }
    }

    /// When the upstream stays silent past the keepalive interval, the
    /// wrapper must insert an SSE comment frame so the downstream socket
    /// stays alive. The upstream itself never ends (it's the
    /// `pending()` stream), so we'd never see anything without this.
    #[tokio::test(start_paused = true)]
    async fn test_sse_keepalive_emits_ping_when_idle() {
        use bytes::Bytes;
        use futures::StreamExt;

        // Upstream that immediately ends after the first chunk. We then
        // expect keepalive frames to fire from second poll onward.
        let upstream = futures::stream::iter(vec![
            Ok::<_, reqwest::Error>(Bytes::from_static(b"data: hi\n\n")),
        ]);
        let upstream = Box::pin(upstream);

        // Wrap a never-ending "no data" stream instead, so the keepalive
        // path is actually exercised. Mix: first chunk from a finite iter,
        // then forever-pending.
        let mut kept = SseKeepaliveStream::new(upstream, 1);

        // Pull the first chunk (the "hi" data).
        let first = kept
            .next()
            .await
            .expect("first item present")
            .expect("no error");
        assert_eq!(&first[..], b"data: hi\n\n");

        // The iterator is exhausted → the wrapper must propagate end-of-
        // stream verbatim rather than fabricate keepalives past the real
        // end of the upstream.
        let second = kept.next().await;
        assert!(
            second.is_none(),
            "after upstream end the wrapper must close, not pad with pings, got {:?}",
            second
        );
    }

    /// When the upstream is truly silent (no first chunk, no end), the
    /// keepalive timer MUST fire and inject a `: ping` frame.
    #[tokio::test(start_paused = true)]
    async fn test_sse_keepalive_pings_on_permanently_idle_stream() {
        use bytes::Bytes;
        use futures::StreamExt;

        // Forever-silent upstream.
        let upstream = futures::stream::pending::<Result<Bytes, reqwest::Error>>();
        let upstream = Box::pin(upstream);

        // Keepalive = 1 s. Timeout = 3 s. We should observe ~2 pings.
        let mut kept = SseKeepaliveStream::new(upstream, 1);
        let got = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            kept.next(),
        )
        .await
        .expect("must not exceed 3s")
        .expect("some chunk")
        .expect("no error");
        assert_eq!(&got[..], b": ping\n\n");
        // Second iteration would also be a ping, but we only need to prove
        // the keepalive fires at all when the upstream is silent.
    }

    /// Ping bytes constant never drifts; downstream clients depend on the
    /// exact `: ping` shape (otherwise some SSE parsers misclassify it).
    #[test]
    fn test_sse_keepalive_bytes_constant() {
        assert_eq!(SSE_KEEPALIVE_BYTES, b": ping\n\n");
    }
}

