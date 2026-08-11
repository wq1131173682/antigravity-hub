// ═══════════════════════════════════════════════════════════════════════════════
// Responses API 中转转发服务 — 独立可运行二进制入口
// ═══════════════════════════════════════════════════════════════════════════════
//
// 功能：将 Chat Completions 协议 ↔ Responses API 协议双向转换
// 客户端发送标准 OpenAI /v1/chat/completions 请求 → 转换为 Responses API 格式
// → 向上游发送 → 将 Responses API SSE 流实时转换为 Chat Completions delta 流
//
// 线上 BUG 修复（v2）：
//   * response_id 会话上下文：为每条下游会话缓存 previous_response_id，
//     多轮工具调用请求注入 previous_response_id + 增量 input，防止上下文断裂
//   * 上游拒绝 previous_response_id 时自动降级为全量模式并重试一次
//   * 终结信号只在流真正结束时下发（详见 responses_bridge.rs）
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
//   RESPONSES_RELAY_CONTEXT_MODE=response_id|full
//   RESPONSES_RELAY_MAX_SESSION_CONTEXTS=1024
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
// 注：本二进制以 #[path] 内联该模块；其中部分类型（SessionStore 等）仅在本
// 二进制内构造，在 lib 目标视角下不可达，故允许 dead_code。
#[path = "../modules/responses_bridge.rs"]
#[allow(dead_code)]
mod responses_bridge;

use responses_bridge::{
    BridgeConfig, RequestContext, SessionContext, SessionStore, StreamHooks,
    transform_chat_to_responses_request, transform_chat_to_responses_request_ctx,
    transform_responses_stream_to_chat, transform_responses_to_chat_completions,
};

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

    /// 会话上下文模式："response_id"（多轮续接，默认）| "full"（全量无状态）
    #[arg(long, default_value = "response_id", env = "RESPONSES_RELAY_CONTEXT_MODE")]
    pub context_mode: String,

    /// 会话上下文缓存上限（条目数）
    #[arg(long, default_value_t = 1024, env = "RESPONSES_RELAY_MAX_SESSION_CONTEXTS")]
    pub max_session_contexts: usize,

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

    pub fn use_response_id_context(&self) -> bool {
        self.context_mode.eq_ignore_ascii_case("response_id")
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 应用状态
// ────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    config: Arc<RelayConfig>,
    http_client: reqwest::Client,
    sessions: Arc<SessionStore>,
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
            "context_mode": state.config.context_mode,
            "max_session_contexts": state.config.max_session_contexts,
            "active_session_contexts": state.sessions.len(),
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

/// 判断上游错误体是否与 previous_response_id 相关（用于自动降级）
fn is_context_error(body: &str) -> bool {
    let lower = body.to_lowercase();
    lower.contains("previous_response_id")
        || lower.contains("previous_response")
        || lower.contains("previous response")
        || lower.contains("previous_response_id")
}

/// 构造上游错误响应（解析 Responses 错误格式 → Chat 错误格式）
fn upstream_error_response(status_code: StatusCode, body: &str) -> Response {
    if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(body) {
        let err = err_json.get("error").cloned().unwrap_or(err_json);
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": {
                    "message": err.get("message").and_then(|m| m.as_str()).unwrap_or("Upstream error"),
                    "code": err.get("code").and_then(|c| c.as_str()).unwrap_or("upstream_error"),
                    "type": "upstream_error"
                }
            })),
        ).into_response();
    }
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": {"message": format!("Upstream error: HTTP {}", status_code), "type": "upstream_error"}
        })),
    ).into_response()
}

/// 向上游发送 Responses API 请求
async fn send_upstream_request(
    state: &AppState,
    body: Vec<u8>,
    api_key: &str,
    user_agent: Option<&str>,
) -> Result<reqwest::Response, String> {
    let upstream_base = state.config.upstream_url.trim_end_matches('/');
    let upstream_url = format!("{}/v1/responses", upstream_base);

    let mut req_builder = state.http_client
        .post(&upstream_url)
        .header("Content-Type", "application/json")
        .body(body);

    if !api_key.is_empty() {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }
    if let Some(ua) = user_agent {
        if !ua.is_empty() {
            req_builder = req_builder.header("User-Agent", ua);
        }
    }

    req_builder.send().await
        .map_err(|e| format!("Upstream connection failed: {}", e))
}

/// 非流式响应场景：从上游响应体中提取 response_id 回写会话上下文
fn maybe_update_session(
    state: &AppState,
    key: &str,
    upstream_body: &[u8],
    messages_len: usize,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(upstream_body) {
        if let Some(rid) = v.get("id").and_then(|i| i.as_str()) {
            if !rid.is_empty() {
                state.sessions.update(key, SessionContext {
                    previous_response_id: Some(rid.to_string()),
                    processed_msg_len: messages_len,
                });
            }
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
    let body_bytes_original = body_bytes.to_vec();

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

    // ── 会话上下文（response_id 多轮续接）──
    let messages: Vec<serde_json::Value> = original_json.get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let messages_len = messages.len();
    let use_response_id = state.config.use_response_id_context();
    let session_key = SessionStore::key_for_request(&model_name, &messages);
    let session_ctx = state.sessions.get(&session_key);

    // 构建转换后请求体
    let (translated_body, used_context) = if use_response_id {
        // 增量模式：有 previous_response_id 时只发送新增消息，其余由上下文携带
        let incremental_from = if session_ctx.previous_response_id.is_some() {
            if messages_len > session_ctx.processed_msg_len {
                Some(session_ctx.processed_msg_len)
            } else {
                Some(messages_len) // 无新增消息：空增量（上下文由 previous_response_id 携带）
            }
        } else {
            None
        };
        let ctx = RequestContext {
            previous_response_id: session_ctx.previous_response_id.clone(),
            incremental_from,
        };
        match transform_chat_to_responses_request_ctx(&body_bytes, &ctx) {
            Some(b) => (b, true),
            None => (body_bytes_original.clone(), false),
        }
    } else {
        (
            transform_chat_to_responses_request(&body_bytes)
                .unwrap_or_else(|| body_bytes_original.clone()),
            false,
        )
    };

    // 获取 API Key
    let effective_api_key = api_key.unwrap_or_else(|| state.config.api_key.clone());

    // 转发 user-agent（若有）
    let user_agent = headers.get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    info!(
        "Forwarding Chat Completions request → Responses API upstream (model={}, stream={}, context={}, session_key={})",
        model_name, is_streaming, state.config.context_mode, session_key
    );

    // ── 发送上游请求 ──
    let mut upstream_resp = match send_upstream_request(
        &state, translated_body.clone(), &effective_api_key, user_agent.as_deref(),
    ).await {
        Ok(resp) => resp,
        Err(e) => {
            error!("{}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": {"message": e, "type": "upstream_error"}
                })),
            ).into_response();
        }
    };

    // ── 降级：上游拒绝 previous_response_id → 清除上下文缓存，全量模式重试一次 ──
    if used_context
        && (upstream_resp.status() == StatusCode::BAD_REQUEST
            || upstream_resp.status() == StatusCode::UNPROCESSABLE_ENTITY
            || upstream_resp.status() == StatusCode::NOT_FOUND)
    {
        let err_body = upstream_resp.text().await.unwrap_or_default();
        if is_context_error(&err_body) {
            warn!(
                "Upstream rejected previous_response_id (session={}), falling back to full context mode: {}",
                session_key, err_body
            );
            state.sessions.clear(&session_key);
            let full_body = transform_chat_to_responses_request(&body_bytes_original)
                .unwrap_or_else(|| body_bytes_original.clone());
            match send_upstream_request(&state, full_body, &effective_api_key, user_agent.as_deref()).await {
                Ok(resp) => {
                    upstream_resp = resp;
                }
                Err(e) => {
                    error!("{}", e);
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({
                            "error": {"message": e, "type": "upstream_error"}
                        })),
                    ).into_response();
                }
            }
        } else {
            return upstream_error_response(StatusCode::BAD_REQUEST, &err_body);
        }
    }

    let status = upstream_resp.status();

    if !status.is_success() {
        let error_body = upstream_resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        warn!("Upstream returned HTTP {}: {}", status, error_body);
        return upstream_error_response(status, &error_body);
    }

    if is_streaming {
        // ── 流式响应 ──
        // 检测上游响应是否为流式
        let is_upstream_streaming = upstream_resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if is_upstream_streaming {
            // 流结束钩子：回写 response_id 上下文（多轮续接）
            let sessions = state.sessions.clone();
            let key_for_hook = session_key.clone();
            let hooks = if use_response_id {
                Some(StreamHooks {
                    on_complete: Some(Arc::new(move |resp_id, _ht, _htx, _dur| {
                        if let Some(rid) = resp_id {
                            sessions.update(&key_for_hook, SessionContext {
                                previous_response_id: Some(rid),
                                processed_msg_len: messages_len,
                            });
                        }
                    })),
                })
            } else {
                None
            };

            // 翻译 Responses API SSE 流 → Chat Completions delta 流
            let bridge_config = state.config.bridge_config();
            let chat_stream = transform_responses_stream_to_chat(
                upstream_resp.bytes_stream(),
                &model_name,
                bridge_config,
                hooks,
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
            // 客户端请求流式，但上游返回非流式 → 读取完整响应并翻译后返回
            let body_bytes = match upstream_resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                        "error": {"message": format!("Failed to read upstream response: {}", e)}
                    }))).into_response();
                }
            };

            // 非流式同样回写 response_id
            maybe_update_session(&state, &session_key, &body_bytes, messages_len, use_response_id);

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
    } else {
        // ── 非流式响应 ──
        let body_bytes = match upstream_resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                    "error": {"message": format!("Failed to read upstream response: {}", e)}
                }))).into_response();
            }
        };

        // 回写 response_id（多轮续接）
        maybe_update_session(&state, &session_key, &body_bytes, messages_len, use_response_id);

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
}

/// 透传 Responses API 请求（不转换）
async fn handle_responses_pass_through(
    state: AppState,
    method: Method,
    _headers: HeaderMap,
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

    let _is_streaming = original_json.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let api_key = original_json.get("api_key")
        .and_then(|k| k.as_str())
        .map(|s| s.to_string());

    let effective_api_key = api_key.unwrap_or_else(|| state.config.api_key.clone());

    match send_upstream_request(&state, body_bytes.to_vec(), &effective_api_key, None).await {
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
                "error": {"message": e, "type": "upstream_error"}
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
                if let Ok(v) = std::env::var("RESPONSES_RELAY_CONTEXT_MODE") {
                    config.context_mode = v;
                }
                if let Ok(v) = std::env::var("RESPONSES_RELAY_MAX_SESSION_CONTEXTS") {
                    config.max_session_contexts = v.parse().unwrap_or(config.max_session_contexts);
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
    info!("  上下文模式:  {} (多轮续接)", config.context_mode);
    info!("  会话缓存上限: {}", config.max_session_contexts);
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

    let max_session_contexts = config.max_session_contexts;
    let state = AppState {
        config: Arc::new(config),
        http_client,
        sessions: Arc::new(SessionStore::with_capacity(max_session_contexts)),
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
        .route("/v1/models", any(models_handler))
        .route("/models", any(models_handler))
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
