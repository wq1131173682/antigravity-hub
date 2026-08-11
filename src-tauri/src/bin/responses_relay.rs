// ═══════════════════════════════════════════════════════════════════════════════
// Responses API 中转转发服务 — 独立可运行二进制入口
// ═══════════════════════════════════════════════════════════════════════════════
//
// 功能：将 Chat Completions 协议 ↔ Responses API 协议双向转换
// 客户端发送标准 OpenAI /v1/chat/completions 请求 → 转换为 Responses API 格式
// → 向上游发送 → 将 Responses API SSE 流实时转换为 Chat Completions delta 流
//
// 启动方式：
//   cargo run --bin responses_relay
//   cargo run --bin responses_relay -- --config /path/to/config.toml
//
// 环境变量覆盖：
//   RESPONSES_RELAY_PORT=8046
//   RESPONSES_RELAY_HOST=127.0.0.1
//   RESPONSES_RELAY_UPSTREAM_URL=https://api.openai.com
//   RESPONSES_RELAY_API_KEY=sk-xxx
//   RESPONSES_RELAY_MODEL=gpt-4o
//   RESPONSES_RELAY_HEARTBEAT_INTERVAL=15
//   RESPONSES_RELAY_CONNECT_TIMEOUT=30
//   RESPONSES_RELAY_READ_TIMEOUT=600
//   RESPONSES_RELAY_SESSION_MAX_DURATION=600
//   RESPONSES_RELAY_FIRST_CHUNK_TIMEOUT=120
//   RESPONSES_RELAY_CHUNK_IDLE_TIMEOUT=300

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};

// ── 确保 responses_bridge 模块可见 ──
// 注：此二进制文件在 src-tauri 包内，但以独立二进制方式运行
// 需要引用项目内的模块。由于 Cargo 二进制文件不能直接引用 lib 模块，
// 我们在此重写/引用 responses_bridge 模块。
// 实际上，我们直接使用嵌入式方式。
#[path = "../modules/responses_bridge.rs"]
mod responses_bridge;

use responses_bridge::{BridgeConfig, transform_chat_to_responses_request, transform_responses_stream_to_chat, transform_responses_to_chat_completions};

// ────────────────────────────────────────────────────────────────────────────
// 配置
// ────────────────────────────────────────────────────────────────────────────

/// 中继服务配置
#[derive(Debug, Clone, Serialize, Deserialize, Parser)]
#[command(name = "responses-relay", about = "Responses API ↔ Chat Completions 中转转发服务")]
pub struct RelayConfig {
    /// 监听地址
    #[arg(long, default_value = "127.0.0.1", env = "RESPONSES_RELAY_HOST")]
    pub host: String,

    /// 监听端口
    #[arg(long, default_value_t = 8046, env = "RESPONSES_RELAY_PORT")]
    pub port: u16,

    /// 上游 Responses API 基础 URL
    #[arg(long, default_value = "https://api.openai.com", env = "RESPONSES_RELAY_UPSTREAM_URL")]
    pub upstream_url: String,

    /// 上游 API Key
    #[arg(long, default_value = "", env = "RESPONSES_RELAY_API_KEY")]
    pub api_key: String,

    /// 默认模型名称（当请求中未指定时使用）
    #[arg(long, default_value = "gpt-4o", env = "RESPONSES_RELAY_MODEL")]
    pub default_model: String,

    // ── 超时与心跳配置 ──
    /// 上游连接超时（秒）
    #[arg(long, default_value_t = 30, env = "RESPONSES_RELAY_CONNECT_TIMEOUT")]
    pub connect_timeout_secs: u64,

    /// 上游读取超时（秒）
    #[arg(long, default_value_t = 600, env = "RESPONSES_RELAY_READ_TIMEOUT")]
    pub read_timeout_secs: u64,

    /// 整体会话最大时长（秒）
    #[arg(long, default_value_t = 600, env = "RESPONSES_RELAY_SESSION_MAX_DURATION")]
    pub session_max_duration_secs: u64,

    /// SSE 心跳间隔（秒）
    #[arg(long, default_value_t = 15, env = "RESPONSES_RELAY_HEARTBEAT_INTERVAL")]
    pub heartbeat_interval_secs: u64,

    /// 首次分片超时（秒）
    #[arg(long, default_value_t = 120, env = "RESPONSES_RELAY_FIRST_CHUNK_TIMEOUT")]
    pub first_chunk_timeout_secs: u64,

    /// 分片空闲超时（秒）
    #[arg(long, default_value_t = 300, env = "RESPONSES_RELAY_CHUNK_IDLE_TIMEOUT")]
    pub chunk_idle_timeout_secs: u64,
}

impl RelayConfig {
    pub fn from_toml(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file '{}': {}", path, e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config file '{}': {}", path, e))
    }

    pub fn bridge_config(&self) -> BridgeConfig {
        BridgeConfig {
            upstream_connect_timeout_secs: self.connect_timeout_secs,
            upstream_read_timeout_secs: self.read_timeout_secs,
            session_max_duration_secs: self.session_max_duration_secs,
            heartbeat_interval_secs: self.heartbeat_interval_secs,
            first_chunk_timeout_secs: self.first_chunk_timeout_secs,
            chunk_idle_timeout_secs: self.chunk_idle_timeout_secs,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 应用状态
// ────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    config: Arc<RelayConfig>,
    http_client: reqwest::Client,
}

// ────────────────────────────────────────────────────────────────────────────
// 请求/响应结构
// ────────────────────────────────────────────────────────────────────────────

/// 健康检查响应
#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    upstream: String,
    config: serde_json::Value,
}

// ────────────────────────────────────────────────────────────────────────────
// 路由处理器
// ────────────────────────────────────────────────────────────────────────────

/// 健康检查端点
async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        upstream: state.config.upstream_url.clone(),
        config: serde_json::json!({
            "host": state.config.host,
            "port": state.config.port,
            "connect_timeout_secs": state.config.connect_timeout_secs,
            "read_timeout_secs": state.config.read_timeout_secs,
            "session_max_duration_secs": state.config.session_max_duration_secs,
            "heartbeat_interval_secs": state.config.heartbeat_interval_secs,
            "first_chunk_timeout_secs": state.config.first_chunk_timeout_secs,
            "chunk_idle_timeout_secs": state.config.chunk_idle_timeout_secs,
        }),
    })
}

/// 模型列表端点（返回可用模型列表）
async fn models_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "object": "list",
        "data": [
            {
                "id": state.config.default_model,
                "object": "model",
                "created": 1677610602,
                "owned_by": "responses-relay"
            }
        ]
    }))
}

/// 主代理处理端点
async fn proxy_handler(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    let path = uri.path().trim_start_matches('/');

    // 路由分发
    match path {
        "v1/chat/completions" | "chat/completions" => {
            handle_chat_completions(state, method, headers, body).await
        }
        "v1/responses" | "responses" => {
            // 直接透传 Responses API 请求到上游
            handle_responses_pass_through(state, method, headers, body).await
        }
        _ => {
            // 未知路径
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("Unknown endpoint: /{}", path),
                        "type": "invalid_request_error"
                    }
                })),
            )
                .into_response()
        }
    }
}

/// 处理 Chat Completions 请求
async fn handle_chat_completions(
    state: AppState,
    method: Method,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    // 只接受 POST
    if method != Method::POST {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(serde_json::json!({
                "error": {"message": "Method not allowed", "type": "invalid_request_error"}
            })),
        ).into_response();
    }

    // 读取请求体
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {"message": format!("Failed to read body: {}", e)}
                })),
            ).into_response();
        }
    };

    // 解析请求体，判断是否 streaming
    let original_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {"message": format!("Invalid JSON body: {}", e)}
                })),
            ).into_response();
        }
    };

    let is_streaming = original_json.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let model_name = original_json.get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(&state.config.default_model)
        .to_string();
    let api_key = original_json.get("api_key")
        .and_then(|k| k.as_str())
        .map(|s| s.to_string());

    // 翻译请求体为 Responses API 格式
    let translated_body = match transform_chat_to_responses_request(&body_bytes) {
        Some(b) => b,
        None => {
            warn!("Failed to translate request body to Responses API format");
            // 降级：使用原始请求体直接发送
            body_bytes.to_vec()
        }
    };

    // 构建上游请求 URL
    let upstream_base = state.config.upstream_url.trim_end_matches('/').to_string();
    // Responses API 端点
    let upstream_url = format!("{}/v1/responses", upstream_base);

    // 获取 API Key
    let effective_api_key = api_key.unwrap_or_else(|| state.config.api_key.clone());

    // 构建上游请求
    let mut req_builder = state.http_client
        .post(&upstream_url)
        .header("Content-Type", "application/json")
        .body(translated_body);

    if !effective_api_key.is_empty() {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", effective_api_key));
    }

    // 转发客户端请求中的一些头信息
    if let Some(ct) = headers.get("content-type") {
        // 保留原始 content-type
    }
    // 转发用户代理
    if let Some(ua) = headers.get("user-agent") {
        req_builder = req_builder.header("User-Agent", ua.clone());
    }

    info!(
        "Forwarding Chat Completions request → Responses API upstream: {} (model={}, stream={})",
        upstream_url, model_name, is_streaming
    );

    if is_streaming {
        // ── 流式响应 ──
        match req_builder.send().await {
            Ok(upstream_resp) => {
                let status = upstream_resp.status();

                if !status.is_success() {
                    // 非成功状态码，读取错误体
                    let error_body = upstream_resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                    warn!("Upstream returned HTTP {}: {}", status, error_body);

                    // 尝试解析为 Responses API 错误格式
                    if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                        // 转换为 Chat Completions 错误格式
                        return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                            "error": {
                                "message": err_json.get("error")
                                    .and_then(|e| e.get("message"))
                                    .and_then(|m| m.as_str())
                                    .unwrap_or(&error_body),
                                "code": err_json.get("error")
                                    .and_then(|e| e.get("code"))
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("upstream_error"),
                                "type": "upstream_error"
                            }
                        }))).into_response();
                    }

                    return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                        "error": {"message": format!("Upstream error: HTTP {}", status), "type": "upstream_error"}
                    }))).into_response();
                }

                // 检测上游响应是否为流式
                let is_upstream_streaming = upstream_resp.headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.contains("text/event-stream"))
                    .unwrap_or(false);

                if is_upstream_streaming {
                    // 翻译 Responses API SSE 流 → Chat Completions delta 流
                    let bridge_config = state.config.bridge_config();
                    let chat_stream = transform_responses_stream_to_chat(
                        upstream_resp.bytes_stream(),
                        &model_name,
                        bridge_config,
                    );

                    let body = axum::body::Body::from_stream(chat_stream);
                    Response::builder()
                        .status(200)
                        .header("Content-Type", "text/event-stream; charset=utf-8")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .header("X-Accel-Buffering", "no")
                        .body(body)
                        .unwrap_or_else(|_| {
                            Response::new(axum::body::Body::from("data: [DONE]\n\n"))
                        })
                } else {
                    // 非流式响应，翻译后直接返回
                    let body_bytes = match upstream_resp.bytes().await {
                        Ok(b) => b,
                        Err(e) => {
                            return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                                "error": {"message": format!("Failed to read upstream response: {}", e)}
                            }))).into_response();
                        }
                    };

                    let translated = transform_responses_to_chat_completions(&body_bytes)
                        .unwrap_or(body_bytes.to_vec());

                    Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(axum::body::Body::from(translated))
                        .unwrap_or_else(|_| {
                            Response::new(axum::body::Body::from("{}"))
                        })
                }
            }
            Err(e) => {
                error!("Upstream connection failed: {}", e);
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": {
                            "message": format!("Upstream connection failed: {}", e),
                            "type": "upstream_error"
                        }
                    })),
                ).into_response()
            }
        }
    } else {
        // ── 非流式响应 ──
        match req_builder.send().await {
            Ok(upstream_resp) => {
                let status = upstream_resp.status();
                let body_bytes = match upstream_resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                            "error": {"message": format!("Failed to read upstream response: {}", e)}
                        }))).into_response();
                    }
                };

                if !status.is_success() {
                    // 转发错误
                    if let Ok(err_json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                        return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                            "error": {
                                "message": err_json.get("error")
                                    .and_then(|e| e.get("message"))
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("Upstream error"),
                                "code": err_json.get("error")
                                    .and_then(|e| e.get("code"))
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("upstream_error")
                            }
                        }))).into_response();
                    }
                    let error_text = String::from_utf8_lossy(&body_bytes);
                    return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                        "error": {"message": format!("Upstream HTTP {}: {}", status, error_text)}
                    }))).into_response();
                }

                // 翻译 Response API 响应为 Chat Completions 格式
                let translated = match transform_responses_to_chat_completions(&body_bytes) {
                    Some(b) => b,
                    None => {
                        warn!("Failed to translate Responses API response, returning raw body");
                        body_bytes.to_vec()
                    }
                };

                Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(axum::body::Body::from(translated))
                    .unwrap_or_else(|_| {
                        Response::new(axum::body::Body::from("{}"))
                    })
            }
            Err(e) => {
                error!("Upstream connection failed: {}", e);
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": {
                            "message": format!("Upstream connection failed: {}", e),
                            "type": "upstream_error"
                        }
                    })),
                ).into_response()
            }
        }
    }
}

/// 透传 Responses API 请求（不转换）
async fn handle_responses_pass_through(
    state: AppState,
    method: Method,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if method != Method::POST {
        return (StatusCode::METHOD_NOT_ALLOWED, Json(serde_json::json!({
            "error": {"message": "Method not allowed"}
        }))).into_response();
    }

    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": {"message": format!("Failed to read body: {}", e)}
            }))).into_response();
        }
    };

    let original_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(j) => j,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": {"message": format!("Invalid JSON: {}", e)}
            }))).into_response();
        }
    };

    let is_streaming = original_json.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let api_key = original_json.get("api_key")
        .and_then(|k| k.as_str())
        .map(|s| s.to_string());

    let upstream_base = state.config.upstream_url.trim_end_matches('/').to_string();
    let upstream_url = format!("{}/v1/responses", upstream_base);
    let effective_api_key = api_key.unwrap_or_else(|| state.config.api_key.clone());

    let mut req_builder = state.http_client
        .post(&upstream_url)
        .header("Content-Type", "application/json")
        .body(body_bytes);

    if !effective_api_key.is_empty() {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", effective_api_key));
    }

    match req_builder.send().await {
        Ok(upstream_resp) => {
            let status = upstream_resp.status();
            // 在消费 body 前捕获 content-type
            let content_type = upstream_resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let is_upstream_streaming = content_type.as_deref()
                .map(|v| v.contains("text/event-stream"))
                .unwrap_or(false);

            if is_upstream_streaming {
                let body = axum::body::Body::from_stream(upstream_resp.bytes_stream());
                let mut resp_builder = Response::builder().status(status);
                if let Some(ct) = content_type {
                    resp_builder = resp_builder.header("Content-Type", ct);
                }
                resp_builder = resp_builder
                    .header("Cache-Control", "no-cache")
                    .header("X-Accel-Buffering", "no");
                resp_builder.body(body).unwrap_or_else(|_| {
                    Response::new(axum::body::Body::from("data: [DONE]\n\n"))
                })
            } else {
                let body_bytes = upstream_resp.bytes().await.unwrap_or_default();
                Response::builder()
                    .status(status)
                    .header("Content-Type", "application/json")
                    .body(axum::body::Body::from(body_bytes))
                    .unwrap_or_else(|_| {
                        Response::new(axum::body::Body::from("{}"))
                    })
            }
        }
        Err(e) => {
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                "error": {"message": format!("Upstream connection failed: {}", e)}
            }))).into_response()
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 入口
// ────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("responses_relay=info".parse().unwrap())
                .add_directive("responses_bridge=info".parse().unwrap())
        )
        .init();

    // 解析配置
    let mut config = RelayConfig::parse();

    // 支持从文件加载配置
    // 如果提供了 --config 参数，从文件加载并覆盖 CLI 参数
    if let Ok(config_path) = std::env::var("RESPONSES_RELAY_CONFIG") {
        match RelayConfig::from_toml(&config_path) {
            Ok(file_config) => {
                // 环境变量覆盖文件配置
                config = file_config;
                // 环境变量再次覆盖
                if let Ok(v) = std::env::var("RESPONSES_RELAY_PORT") {
                    config.port = v.parse().unwrap_or(config.port);
                }
                if let Ok(v) = std::env::var("RESPONSES_RELAY_HOST") {
                    config.host = v;
                }
                if let Ok(v) = std::env::var("RESPONSES_RELAY_UPSTREAM_URL") {
                    config.upstream_url = v;
                }
                if let Ok(v) = std::env::var("RESPONSES_RELAY_API_KEY") {
                    config.api_key = v;
                }
                info!("Loaded config from: {}", config_path);
            }
            Err(e) => {
                warn!("Failed to load config file '{}': {}", config_path, e);
            }
        }
    }

    // 打印配置
    info!("═══════════════════════════════════════════════════");
    info!("  Responses API 中转转发服务");
    info!("═══════════════════════════════════════════════════");
    info!("  监听地址:    {}:{}", config.host, config.port);
    info!("  上游 URL:    {}", config.upstream_url);
    info!("  默认模型:    {}", config.default_model);
    info!("  心跳间隔:    {}s", config.heartbeat_interval_secs);
    info!("  连接超时:    {}s", config.connect_timeout_secs);
    info!("  读取超时:    {}s", config.read_timeout_secs);
    info!("  会话最大时长: {}s", config.session_max_duration_secs);
    info!("  首次分片超时: {}s", config.first_chunk_timeout_secs);
    info!("  分片空闲超时: {}s", config.chunk_idle_timeout_secs);
    info!("═══════════════════════════════════════════════════");
    info!("  端点列表:");
    info!("    POST /v1/chat/completions  — Chat → Responses 转换");
    info!("    POST /v1/responses         — 透传 Responses API");
    info!("    GET  /health               — 健康检查");
    info!("    GET  /v1/models             — 模型列表");
    info!("═══════════════════════════════════════════════════");

    // 构建 HTTP 客户端
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.read_timeout_secs))
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .build()
        .expect("Failed to build HTTP client");

    let state = AppState {
        config: Arc::new(config),
        http_client,
    };

    // 在 move 前捕获需要的信息
    let bind_host = state.config.host.clone();
    let bind_port = state.config.port;

    // 构建路由
    let app = Router::new()
        .route("/v1/chat/completions", any(proxy_handler))
        .route("/chat/completions", any(proxy_handler))
        .route("/v1/responses", any(proxy_handler))
        .route("/responses", any(proxy_handler))
        .route("/v1/models", any(proxy_handler))
        .route("/models", any(proxy_handler))
        .route("/health", any(health_handler))
        .route("/", any(proxy_handler))
        .with_state(state);

    // 启动服务
    let addr: SocketAddr = format!("{}:{}", bind_host, bind_port)
        .parse()
        .expect("Invalid bind address");

    info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .await
        .unwrap();
}