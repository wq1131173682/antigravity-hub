// ── Responses API ↔ Chat Completions API 双向转换 ──
//
// 本模块实现 Chat Completions 协议 ↔ Responses API 协议的双向转换，
// 核心目标：
//   1. 客户端发送 Chat Completions 请求 → 转换为 Responses API 请求向上游转发
//   2. 上游推送 Responses API SSE 流 → 实时转换为 Chat Completions delta 流下发客户端
//
// ── 线上 BUG 修复（v2）：Agent 多轮工具调用会话被提前终止 ──
// 旧实现的问题：收到 `response.completed` / `data: [DONE]` 就立即向下游发送
// `finish_reason`（无工具调用时注入 `stop`），而 Chat 客户端（如 ChatGPT Work）
// 收到 `finish_reason: stop` 即认为对话结束，不再发起下一轮工具调用。
// 修复原则：
//   * 终结信号只在「上游 SSE 流真正结束」时下发（EOF / 会话超时 / 硬错误）
//   * `response.completed` / `[DONE]` 只更新状态，绝不注入 stop
//   * finish_reason 在流结束时统一判定：出现过 function_call → "tool_calls"，否则 "stop"
//   * 多 response 会话（上游在一个连接里连续推送多个 response）不再互相覆盖
//   * 心跳计时与空闲超时解耦，死流仍会被清理
//
// 协议字段映射关系（Chat Completions → Responses API）：
//   ┌──────────────────────┬──────────────────────────────────────┐
//   │ Chat Completions     │ Responses API                        │
//   ├──────────────────────┼──────────────────────────────────────┤
//   │ messages[].role      │ input[].role + type="message"        │
//   │ messages[].content   │ input[].content (input_text/input_image) │
//   │ assistant.tool_calls │ input[type="function_call"]          │
//   │ tool.tool_call_id    │ function_call_output.call_id         │
//   │ max_tokens           │ max_output_tokens                    │
//   │ tools[].function.xxx │ tools[].xxx (un-nest)                │
//   │ response_format      │ text.format                          │
//   │ system message       │ instructions (top-level field)       │
//   │ stream               │ stream (pass-through)                │
//   │ previous_response_id │ 由会话上下文管理器注入（多轮续接）      │
//   └──────────────────────┴──────────────────────────────────────┘
//
// 协议字段映射关系（Responses API SSE → Chat Completions delta）：
//   ┌────────────────────────────────┬──────────────────────────────────┐
//   │ Responses API SSE event        │ Chat Completions delta           │
//   ├────────────────────────────────┼──────────────────────────────────┤
//   │ response.created               │ → role="assistant" initial chunk │
//   │ response.output_text.delta     │ → choices[0].delta.content       │
//   │ function_call output_item.added│ → choices[0].delta.tool_calls    │
//   │ function_call_arguments.delta  │ → tool_calls[].function.arguments│
//   │ response.completed             │ → 仅更新状态，不注入 finish      │
//   │ response.failed                │ → error chunk + 结束流           │
//   │ data: [DONE]                   │ → 仅标记，不结束流（等待 EOF）    │
//   │ 上游流 EOF                     │ → finish_reason + [DONE]         │
//   └────────────────────────────────┴──────────────────────────────────┘

use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use futures::StreamExt;
use tracing::{info, warn};

// ────────────────────────────────────────────────────────────────────────────
// 公共配置类型
// ────────────────────────────────────────────────────────────────────────────

/// Responses API 转换器配置参数
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// 上游连接超时（秒）
    pub upstream_connect_timeout_secs: u64,
    /// 上游读取超时（秒）
    pub upstream_read_timeout_secs: u64,
    /// 整体会话最大时长（秒）
    pub session_max_duration_secs: u64,
    /// SSE 心跳间隔（秒）—— 无数据时发送注释心跳
    pub heartbeat_interval_secs: u64,
    /// 首次分片超时（秒）
    pub first_chunk_timeout_secs: u64,
    /// 分片空闲超时（秒）
    pub chunk_idle_timeout_secs: u64,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            upstream_connect_timeout_secs: 30,
            upstream_read_timeout_secs: 600,
            session_max_duration_secs: 600,
            heartbeat_interval_secs: 15,
            first_chunk_timeout_secs: 120,
            chunk_idle_timeout_secs: 300,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 会话上下文（response_id 多轮续接）
// ────────────────────────────────────────────────────────────────────────────

/// 单条下游会话的上下文缓存项。
///
/// Responses API 依靠 `previous_response_id` 维持连续多轮工具调用链路；
/// Chat 客户端每轮发送完整 messages，但部分上游必须用 response_id 续接。
/// 转换器为每一条下游会话缓存上一个 response_id 与已处理的消息数，
/// 下一轮请求时注入 `previous_response_id` + 增量 input，避免上下文断裂。
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    /// 最近一轮响应的 response_id（resp_xxx），下一轮请求注入 previous_response_id
    pub previous_response_id: Option<String>,
    /// 上次请求已处理的消息条数（用于计算增量 input）
    pub processed_msg_len: usize,
}

/// 会话上下文存储（线程安全，多请求并发）
pub struct SessionStore {
    inner: Mutex<HashMap<String, SessionContext>>,
    /// 插入顺序（简单 LRU 淘汰用）
    order: Mutex<std::collections::VecDeque<String>>,
    max_entries: usize,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::with_capacity(1024)
    }

    /// 指定缓存条目上限的会话存储
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            order: Mutex::new(std::collections::VecDeque::new()),
            max_entries: max_entries.max(1),
        }
    }

    /// 生成稳定的会话 key：model + 第一条 user 消息内容（截断）。
    /// 同一轮多轮工具调用的所有请求共享同一个根 user 消息 → key 稳定。
    pub fn key_for_request(model: &str, messages: &[serde_json::Value]) -> String {
        let root = messages
            .iter()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            .and_then(|m| m.get("content").and_then(|c| c.as_str()))
            .unwrap_or("")
            .chars()
            .take(128)
            .collect::<String>();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        h.write(model.as_bytes());
        h.write(b"|");
        h.write(root.as_bytes());
        let digest = h.finish();
        format!("{}-{:016x}", model.replace(['/', ':', ' '], "_"), digest)
    }

    /// 读取会话上下文（不存在则返回默认值）
    pub fn get(&self, key: &str) -> SessionContext {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.get(key).cloned().unwrap_or_default()
    }

    /// 写入/更新会话上下文（LRU 淘汰）
    pub fn update(&self, key: &str, ctx: SessionContext) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let existed = guard.contains_key(key);
        guard.insert(key.to_string(), ctx);
        drop(guard);

        if !existed {
            let mut order = self.order.lock().unwrap_or_else(|e| e.into_inner());
            order.push_back(key.to_string());
            while order.len() > self.max_entries {
                if let Some(oldest) = order.pop_front() {
                    let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    guard.remove(&oldest);
                }
            }
        }
    }

    /// 清除某条会话（用于上游拒绝 previous_response_id 时的降级）
    pub fn clear(&self, key: &str) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(key);
        drop(guard);
        let mut order = self.order.lock().unwrap_or_else(|e| e.into_inner());
        order.retain(|k| k != key);
    }

    /// 当前缓存条数
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 1. 请求转换：Chat Completions → Responses API
// ────────────────────────────────────────────────────────────────────────────

/// 请求转换上下文
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    /// 上一轮响应的 response_id，注入顶层 previous_response_id
    pub previous_response_id: Option<String>,
    /// 增量模式：Some(i) 时只转换 messages[i..]（i 之后为新增消息）
    pub incremental_from: Option<usize>,
}

/// 将 Chat Completions 请求体转换为 Responses API 请求体（无上下文，全量模式）。
///
/// # 输入格式（Chat Completions）
/// ```json
/// {
///   "model": "gpt-4",
///   "messages": [
///     {"role": "system", "content": "You are helpful"},
///     {"role": "user", "content": "Hello"},
///     {"role": "assistant", "content": null, "tool_calls": [...]},
///     {"role": "tool", "tool_call_id": "call_1", "content": "result"}
///   ],
///   "max_tokens": 4096,
///   "stream": true,
///   "tools": [...],
///   "tool_choice": "auto",
///   "temperature": 0.7,
///   "stop": ["\n"]
/// }
/// ```
///
/// # 输出格式（Responses API）
/// ```json
/// {
///   "model": "gpt-4",
///   "input": [
///     {"type": "message", "role": "system", "content": [{"type": "input_text", "text": "You are helpful"}]},
///     {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]},
///     {"type": "function_call", "id": "call_1", "call_id": "call_1", "name": "tool", "arguments": "{}"},
///     {"type": "function_call_output", "call_id": "call_1", "output": "result"}
///   ],
///   "instructions": "You are helpful",
///   "max_output_tokens": 4096,
///   "stream": true,
///   "tools": [...],
///   "tool_choice": "auto",
///   "temperature": 0.7
/// }
/// ```
pub fn transform_chat_to_responses_request(body_bytes: &[u8]) -> Option<Vec<u8>> {
    transform_chat_to_responses_request_ctx(body_bytes, &RequestContext::default())
}

/// 带会话上下文的请求转换。
///
/// - `ctx.previous_response_id`：非空时注入顶层 `previous_response_id`（多轮续接）。
/// - `ctx.incremental_from`：Some(i) 时只转换 `messages[i..]` 为 input（增量模式），
///   system 提取为 instructions 仅在非增量模式执行。
pub fn transform_chat_to_responses_request_ctx(body_bytes: &[u8], ctx: &RequestContext) -> Option<Vec<u8>> {
    let mut json: serde_json::Value = serde_json::from_slice(body_bytes).ok()?;
    let obj = json.as_object_mut()?;

    // 增量起点：默认从 0 开始
    let start_idx = ctx.incremental_from.unwrap_or(0);
    let incremental = ctx.incremental_from.is_some();

    // 1. 提取 system prompt → instructions（仅非增量模式）
    let mut instructions: Option<String> = None;
    let mut messages: Vec<serde_json::Value> = Vec::new();
    if let Some(msg_array) = obj.remove("messages").and_then(|m| m.as_array().cloned()) {
        for (i, msg) in msg_array.into_iter().enumerate() {
            if i < start_idx {
                continue;
            }
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string();
            if !incremental && role == "system" && instructions.is_none() && i == 0 {
                // 只将第一个 system 消息提取为 instructions
                if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                    instructions = Some(content.to_string());
                    continue; // 跳过，不加入 input
                }
            }
            messages.push(msg);
        }
    }

    // 2. 将 messages 转换为 Responses API input 数组
    let mut input: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string();

        match role.as_str() {
            "user" | "system" => {
                // User/system 消息 → {"type": "message", "role": "...", "content": [...]}
                let content = msg.get("content");
                let converted_content = convert_chat_content_to_responses(content);
                input.push(serde_json::json!({
                    "type": "message",
                    "role": role,
                    "content": converted_content
                }));
            }
            "assistant" => {
                // Assistant 消息可能同时有 text content 和 tool_calls
                let has_tool_calls = msg.get("tool_calls").and_then(|t| t.as_array())
                    .map(|a| !a.is_empty()).unwrap_or(false);

                if has_tool_calls {
                    // 有 tool_calls → 先发 text（如果有），再发 function_call 项
                    if let Some(content) = msg.get("content") {
                        if !content.is_null() {
                            let text = if let Some(s) = content.as_str() {
                                if !s.is_empty() {
                                    Some(s.to_string())
                                } else { None }
                            } else { None };
                            if let Some(text) = text {
                                input.push(serde_json::json!({
                                    "type": "message",
                                    "role": "assistant",
                                    "content": [{"type": "output_text", "text": text}]
                                }));
                            }
                        }
                    }

                    // Chat Completions tool_calls → Responses API function_call 项
                    if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                        for tc in tool_calls {
                            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                            let name = tc.get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str()).unwrap_or("").to_string();
                            let arguments = tc.get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str()).unwrap_or("{}").to_string();
                            input.push(serde_json::json!({
                                "type": "function_call",
                                "id": id,
                                "call_id": id,
                                "name": name,
                                "arguments": arguments
                            }));
                        }
                    }
                } else {
                    // 纯文本 assistant 消息
                    let content = msg.get("content");
                    let converted = convert_chat_content_to_responses(content);
                    input.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": converted
                    }));
                }
            }
            "tool" => {
                // Tool 消息 → {"type": "function_call_output", "call_id": "...", "output": "..."}
                let call_id = msg.get("tool_call_id")
                    .and_then(|c| c.as_str()).unwrap_or("").to_string();
                let output = msg.get("content")
                    .and_then(|c| c.as_str()).unwrap_or("").to_string();
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output
                }));
            }
            _ => {
                // 未知角色 → 作为 user 消息处理
                let content = msg.get("content");
                let converted = convert_chat_content_to_responses(content);
                input.push(serde_json::json!({
                    "type": "message",
                    "role": "user",
                    "content": converted
                }));
            }
        }
    }

    if !input.is_empty() {
        obj.insert("input".to_string(), serde_json::Value::Array(input));
    } else {
        // 增量模式且无新增消息：空 input 数组（上下文由 previous_response_id 携带）
        obj.insert("input".to_string(), serde_json::Value::Array(Vec::new()));
    }

    // 3. 设置 instructions（仅非增量模式）
    if !incremental {
        if let Some(instructions) = instructions {
            obj.insert("instructions".to_string(), serde_json::Value::String(instructions));
        }
    }

    // 3.5 注入 previous_response_id（多轮续接关键）
    if let Some(prev_id) = &ctx.previous_response_id {
        if !prev_id.is_empty() {
            obj.insert("previous_response_id".to_string(), serde_json::Value::String(prev_id.clone()));
        }
    }

    // 4. 转换 max_tokens → max_output_tokens
    if let Some(max_tokens) = obj.remove("max_tokens") {
        if !obj.contains_key("max_output_tokens") {
            obj.insert("max_output_tokens".to_string(), max_tokens);
        }
    }

    // 5. 转换 tools 格式：Chat Completions 嵌套 function → Responses API 扁平
    if let Some(tools) = obj.get_mut("tools") {
        if let Some(tools_array) = tools.as_array_mut() {
            let converted: Vec<serde_json::Value> = tools_array.drain(..).filter_map(|tool| {
                let tool_type = tool.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
                if tool_type == "function" {
                    // Chat Completions: {"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}
                    // Responses API: {"type": "function", "name": "...", "description": "...", "parameters": {...}}
                    if let Some(fn_obj) = tool.get("function").and_then(|f| f.as_object()) {
                        let mut new_tool = serde_json::Map::new();
                        new_tool.insert("type".to_string(), serde_json::Value::String("function".to_string()));
                        if let Some(name) = fn_obj.get("name") {
                            new_tool.insert("name".to_string(), name.clone());
                        }
                        if let Some(desc) = fn_obj.get("description") {
                            new_tool.insert("description".to_string(), desc.clone());
                        }
                        if let Some(params) = fn_obj.get("parameters") {
                            new_tool.insert("parameters".to_string(), params.clone());
                        }
                        if let Some(strict) = fn_obj.get("strict") {
                            new_tool.insert("strict".to_string(), strict.clone());
                        }
                        Some(serde_json::Value::Object(new_tool))
                    } else {
                        Some(tool) // 保留原样
                    }
                } else {
                    // 非 function 类型（file_search, web_search 等）直接保留
                    Some(tool)
                }
            }).collect();
            *tools = serde_json::Value::Array(converted);
        }
    }

    // 6. 转换 tool_choice 格式
    if let Some(tool_choice) = obj.get_mut("tool_choice") {
        // Chat Completions: {"type": "function", "function": {"name": "..."}}
        // Responses API: {"type": "function", "name": "..."}
        let name_to_extract = tool_choice
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        if let Some(name) = name_to_extract {
            if let Some(tc_map) = tool_choice.as_object_mut() {
                tc_map.insert("name".to_string(), serde_json::Value::String(name));
                tc_map.remove("function");
            }
        }
    }

    // 7. 转换 response_format → text.format
    // Chat Completions: {"response_format": {"type": "json_schema", "json_schema": {...}}}
    // Responses API: {"text": {"format": {"type": "json_schema", "name": "...", "schema": {...}}}}
    if let Some(response_format) = obj.remove("response_format") {
        let mut text_format = serde_json::Map::new();
        if let Some(rf_obj) = response_format.as_object() {
            let fmt_type = rf_obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match fmt_type {
                "json_schema" => {
                    // 从 json_schema 嵌套中提取 name/schema
                    if let Some(js) = rf_obj.get("json_schema").and_then(|j| j.as_object()) {
                        let mut fmt = serde_json::Map::new();
                        fmt.insert("type".to_string(), serde_json::Value::String("json_schema".to_string()));
                        if let Some(name) = js.get("name") {
                            fmt.insert("name".to_string(), name.clone());
                        }
                        if let Some(schema) = js.get("schema") {
                            fmt.insert("schema".to_string(), schema.clone());
                        }
                        if let Some(strict) = js.get("strict") {
                            fmt.insert("strict".to_string(), strict.clone());
                        }
                        text_format.insert("format".to_string(), serde_json::Value::Object(fmt));
                    } else {
                        text_format.insert("format".to_string(), response_format);
                    }
                }
                _ => {
                    text_format.insert("format".to_string(), response_format);
                }
            }
        } else {
            text_format.insert("format".to_string(), response_format);
        }
        if !text_format.is_empty() {
            obj.insert("text".to_string(), serde_json::Value::Object(text_format));
        }
    }

    // 8. 保留通用参数（透传）
    // 这些参数在两种协议中名称相同：temperature, top_p, stop, presence_penalty,
    // frequency_penalty, logit_bias, user, metadata, seed, n
    // stream 字段同样透传（Responses API 原生支持 stream: true）

    Some(serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec()))
}

/// 将 Chat Completions 的 content 字段转换为 Responses API 的 content 数组格式。
///
/// Chat Completions content 可以是：
///   - 字符串: "Hello" → [{"type": "input_text", "text": "Hello"}]
///   - 数组: [{"type": "text", "text": "Hello"}, {"type": "image_url", "image_url": {"url": "..."}}]
///     → [{"type": "input_text", "text": "Hello"}, {"type": "input_image", "image_url": {"url": "..."}}]
///   - null → [{"type": "input_text", "text": ""}]
fn convert_chat_content_to_responses(content: Option<&serde_json::Value>) -> serde_json::Value {
    match content {
        None | Some(serde_json::Value::Null) => {
            serde_json::json!([{"type": "input_text", "text": ""}])
        }
        Some(serde_json::Value::String(s)) => {
            if s.is_empty() {
                serde_json::json!([{"type": "input_text", "text": ""}])
            } else {
                serde_json::json!([{"type": "input_text", "text": s}])
            }
        }
        Some(serde_json::Value::Array(parts)) => {
            let converted: Vec<serde_json::Value> = parts.iter().map(|part| {
                let mut p = part.clone();
                if let Some(obj) = p.as_object_mut() {
                    let part_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("text").to_string();
                    match part_type.as_str() {
                        "text" => {
                            obj.insert("type".to_string(), serde_json::Value::String("input_text".to_string()));
                        }
                        "image_url" => {
                            obj.insert("type".to_string(), serde_json::Value::String("input_image".to_string()));
                        }
                        "image" => {
                            obj.insert("type".to_string(), serde_json::Value::String("input_image".to_string()));
                        }
                        "file" => {
                            obj.insert("type".to_string(), serde_json::Value::String("input_file".to_string()));
                        }
                        "audio" => {
                            obj.insert("type".to_string(), serde_json::Value::String("input_audio".to_string()));
                        }
                        "input_text" | "input_image" | "input_file" | "input_audio" => {
                            // 已经是 Responses API 格式，保留
                        }
                        _ => {
                            // 未知类型，默认转为 text
                            obj.insert("type".to_string(), serde_json::Value::String("input_text".to_string()));
                        }
                    }
                }
                p
            }).collect();
            serde_json::Value::Array(converted)
        }
        Some(other) => {
            // 其他类型 → 转为字符串
            serde_json::json!([{"type": "input_text", "text": other.to_string()}])
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 2. 流式响应转换：Responses API SSE → Chat Completions delta
// ────────────────────────────────────────────────────────────────────────────

/// 流结束钩子：上游流结束时由转换器回调（供服务层回写 response_id 上下文）
pub struct StreamHooks {
    /// 参数：(response_id, has_tool_calls, has_text_output, duration_ms)
    pub on_complete: Option<Arc<dyn Fn(Option<String>, bool, bool, u128) + Send + Sync>>,
}

impl Default for StreamHooks {
    fn default() -> Self {
        Self { on_complete: None }
    }
}

/// 流式转换器状态
struct StreamTranslatorState {
    /// 从 response.created 中提取的响应 ID
    response_id: String,
    /// 模型名称
    model_name: String,
    /// 创建时间戳
    created_at: i64,

    /// 是否已发送 role="assistant" 初始 chunk
    has_sent_role: bool,
    /// 聊天完成 chunk 计数器（用于 id 生成）
    chunk_counter: u64,

    // ── 文本输出跟踪 ──
    text_item_id: Option<String>,
    text_output_index: Option<u32>,
    has_sent_text_delta: bool,

    // ── 工具调用跟踪 ──
    /// 工具调用状态：以 item_id 为主键（多 response 会话不串号）
    tool_calls: HashMap<String, ToolCallState>,
    /// output_index → item_id 辅助映射（处理只带 output_index 的事件）
    output_index_to_item_id: HashMap<u32, String>,
    /// 下一个工具调用的 chat 索引（全流连续，不因 response 重置）
    next_tool_chat_index: u32,

    // ── 完成状态 ──
    /// 是否已下发 finish（流已终结，幂等）
    is_finished: bool,
    /// 整个流中是否出现过 function_call（决定最终 finish_reason）
    has_tool_calls: bool,
    /// 是否有文本输出
    has_text_output: bool,

    // ── 会话钩子 ──
    complete_hook: Option<Arc<dyn Fn(Option<String>, bool, bool, u128) + Send + Sync>>,
}

/// 单个工具调用在转换过程中的状态
#[derive(Debug, Clone)]
struct ToolCallState {
    /// 累积的参数片段（仅用于统计/校验，转发始终用原始 delta）
    arguments_buffer: String,
    /// Chat Completions 中的索引
    chat_index: u32,
    /// 是否已发送初始 delta（带 id 和 name）
    has_sent_initial: bool,
}

impl StreamTranslatorState {
    fn new(
        complete_hook: Option<Arc<dyn Fn(Option<String>, bool, bool, u128) + Send + Sync>>,
    ) -> Self {
        Self {
            response_id: String::new(),
            model_name: String::new(),
            created_at: chrono::Utc::now().timestamp(),
            has_sent_role: false,
            chunk_counter: 0,
            text_item_id: None,
            text_output_index: None,
            has_sent_text_delta: false,
            tool_calls: HashMap::new(),
            output_index_to_item_id: HashMap::new(),
            next_tool_chat_index: 0,
            is_finished: false,
            has_tool_calls: false,
            has_text_output: false,
            complete_hook,
        }
    }

    /// 生成聊天完成 chunk ID
    fn chunk_id(&self) -> String {
        if self.response_id.is_empty() {
            format!("chatcmpl-{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
        } else {
            format!("chatcmpl-{}", self.response_id.trim_start_matches("resp_"))
        }
    }

    /// 发送一个 Chat Completions delta 分片
    fn send_delta(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        delta: serde_json::Value,
        finish_reason: Option<&str>,
    ) {
        self.chunk_counter += 1;
        let mut choice = serde_json::json!({
            "index": 0,
            "delta": delta,
        });
        if let Some(reason) = finish_reason {
            choice["finish_reason"] = serde_json::Value::String(reason.to_string());
        } else {
            choice["finish_reason"] = serde_json::Value::Null;
        }

        let chunk = serde_json::json!({
            "id": self.chunk_id(),
            "object": "chat.completion.chunk",
            "created": self.created_at,
            "model": self.model_name,
            "choices": [choice]
        });

        let sse = format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap_or_default());
        let _ = tx.try_send(Ok(axum::body::Bytes::from(sse)));
    }

    /// 发送 role="assistant" 初始 chunk
    fn send_role_chunk(&mut self, tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>) {
        if self.has_sent_role {
            return;
        }
        self.has_sent_role = true;
        self.send_delta(tx, serde_json::json!({"role": "assistant", "content": ""}), None);
    }

    /// 发送文本内容 delta
    fn send_text_delta(&mut self, tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>, text: &str) {
        if text.is_empty() {
            return;
        }
        self.send_role_chunk(tx);
        self.has_sent_text_delta = true;
        self.has_text_output = true;
        self.send_delta(tx, serde_json::json!({"content": text}), None);
    }

    /// 发送工具调用初始 delta（带 id 和 name）
    fn send_tool_call_initial(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        chat_index: u32,
        call_id: &str,
        name: &str,
    ) {
        self.send_role_chunk(tx);
        self.has_tool_calls = true;
        self.send_delta(tx, serde_json::json!({
            "tool_calls": [{
                "index": chat_index,
                "id": call_id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": ""
                }
            }]
        }), None);
    }

    /// 发送工具调用参数 delta（原始分片原样转发，不合并不截断）
    fn send_tool_call_args_delta(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        chat_index: u32,
        arguments: &str,
    ) {
        if arguments.is_empty() {
            return;
        }
        self.send_delta(tx, serde_json::json!({
            "tool_calls": [{
                "index": chat_index,
                "function": {
                    "arguments": arguments
                }
            }]
        }), None);
    }

    /// 流终结：发送 finish chunk（幂等）。
    /// ⚠️ 规则：绝不主动注入 stop —— 只有整个上游流真正结束（EOF/超时/硬错误）
    ///    才由本方法统一下发 finish_reason。
    fn send_finish_chunk(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
    ) {
        if self.is_finished {
            return;
        }
        self.is_finished = true;

        // 流结束时统一判定：出现过多轮工具调用意图 → "tool_calls"，否则 "stop"
        let reason = if self.has_tool_calls {
            "tool_calls"
        } else {
            "stop"
        };

        info!(
            "responses_bridge: sending final finish_reason={} (has_text={}, has_tool_calls={})",
            reason, self.has_text_output, self.has_tool_calls
        );
        self.send_delta(tx, serde_json::json!({}), Some(reason));
    }

    /// 触发流结束钩子（回写 response_id 上下文）
    fn fire_complete_hook(&mut self, duration_ms: u128) {
        if let Some(hook) = self.complete_hook.take() {
            let rid = if self.response_id.is_empty() {
                None
            } else {
                Some(self.response_id.clone())
            };
            hook(rid, self.has_tool_calls, self.has_text_output, duration_ms);
        }
    }

    /// 处理 response.created 事件
    fn handle_response_created(&mut self, data: &serde_json::Value) {
        if let Some(response) = data.get("response") {
            if let Some(id) = response.get("id").and_then(|i| i.as_str()) {
                if !id.is_empty() {
                    self.response_id = id.to_string();
                }
            }
            if let Some(model) = response.get("model").and_then(|m| m.as_str()) {
                if !model.is_empty() {
                    self.model_name = model.to_string();
                }
            }
            if let Some(created) = response.get("created_at").and_then(|c| c.as_i64()) {
                self.created_at = created;
            }
        }
    }

    /// 处理 output_item.added 事件
    fn handle_output_item_added(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        data: &serde_json::Value,
    ) {
        let output_index = data.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
        let item = match data.get("item") {
            Some(i) => i,
            None => return,
        };
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match item_type {
            "message" => {
                // 文本消息项开始
                let item_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                self.text_item_id = Some(item_id);
                self.text_output_index = Some(output_index);
                self.has_sent_text_delta = false;
            }
            "function_call" => {
                // 工具调用项开始 —— 以 item_id 为主键，多 response 会话不覆盖
                let item_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                let call_id = item.get("call_id").and_then(|c| c.as_str()).unwrap_or(&item_id).to_string();
                let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();

                // chat 索引全流连续分配
                let chat_index = self.next_tool_chat_index;
                self.next_tool_chat_index += 1;

                let tc_state = ToolCallState {
                    arguments_buffer: String::new(),
                    chat_index,
                    has_sent_initial: false,
                };
                self.tool_calls.insert(item_id.clone(), tc_state);
                self.output_index_to_item_id.insert(output_index, item_id.clone());

                // 发送初始 tool_call delta（id + name + type 完整）
                self.send_tool_call_initial(tx, chat_index, &call_id, &name);
                if let Some(s) = self.tool_calls.get_mut(&item_id) {
                    s.has_sent_initial = true;
                }
            }
            "reasoning" => {
                // Reasoning 项 — Chat Completions 没有对应概念，跳过
            }
            _ => {
                warn!("responses_bridge: unknown output_item type '{}'", item_type);
            }
        }
    }

    /// 处理 output_text.delta 事件
    fn handle_output_text_delta(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        data: &serde_json::Value,
    ) {
        let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");
        if delta.is_empty() {
            return;
        }
        self.send_text_delta(tx, delta);
    }

    /// 处理 function_call_arguments.delta 事件
    ///
    /// 参数分片按「原始 delta」即时转发，不合并、不截断 —— 保证客户端
    /// 收到的 tool_calls[].function.arguments 分片顺序与上游一致。
    fn handle_function_call_args_delta(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        data: &serde_json::Value,
    ) {
        let output_index = data.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
        let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");

        if delta.is_empty() {
            return;
        }

        // 定位工具调用：优先 item_id，其次 output_index → item_id，最后回退 output_index
        let item_id_owned = data.get("item_id").and_then(|i| i.as_str()).map(|s| s.to_string());
        let found = if let Some(ref iid) = item_id_owned {
            self.tool_calls.get(iid).map(|s| (s.chat_index, s.has_sent_initial, iid.clone()))
        } else if let Some(iid) = self.output_index_to_item_id.get(&output_index) {
            self.tool_calls.get(iid).map(|s| (s.chat_index, s.has_sent_initial, iid.clone()))
        } else {
            None
        };

        match found {
            Some((chat_index, has_sent_initial, iid)) => {
                if !has_sent_initial {
                    // 防御性处理：output_item.added 未到（异常上游），先补发初始 delta
                    self.send_tool_call_initial(tx, chat_index, "", "");
                    if let Some(s) = self.tool_calls.get_mut(&iid) {
                        s.has_sent_initial = true;
                    }
                }
                if let Some(s) = self.tool_calls.get_mut(&iid) {
                    s.arguments_buffer.push_str(delta);
                }
                self.send_tool_call_args_delta(tx, chat_index, delta);
            }
            None => {
                // 未知工具调用的参数块：容错 —— 跳过该块，不中断整条流
                warn!(
                    "responses_bridge: function_call_arguments.delta for unknown output_index {} (item_id={:?}), skipping",
                    output_index, item_id_owned
                );
            }
        }
    }

    /// 处理 response.completed 事件
    ///
    /// ⚠️ 修复：只更新状态（收集 output、判断是否含 function_call、回写 response_id），
    ///    绝不在此时下发 finish_reason —— 会话是否结束由「上游流 EOF」决定，
    ///    避免模型输出阶段性文本后被误判为会话结束、客户端提前终止。
    fn handle_response_completed(&mut self, data: &serde_json::Value) {
        if let Some(response) = data.get("response") {
            if let Some(id) = response.get("id").and_then(|i| i.as_str()) {
                if !id.is_empty() {
                    self.response_id = id.to_string();
                }
            }
            // 从 output 中检查是否包含 function_call（兜底判定）
            if let Some(output) = response.get("output").and_then(|o| o.as_array()) {
                for item in output {
                    if let Some(item_type) = item.get("type").and_then(|t| t.as_str()) {
                        if item_type == "function_call" {
                            self.has_tool_calls = true;
                        }
                    }
                }
            }
        }
    }

    /// 处理 response.failed 事件（上游明确失败 → 硬错误终结）
    fn handle_response_failed(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        data: &serde_json::Value,
    ) {
        if self.is_finished {
            return;
        }
        self.is_finished = true;

        // 提取错误信息
        let error_msg = data.pointer("/response/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown upstream error");
        let error_code = data.pointer("/response/error/code")
            .and_then(|c| c.as_str())
            .unwrap_or("unknown");

        warn!("responses_bridge: upstream error: [{}] {}", error_code, error_msg);

        // 发送错误 chunk
        let error_chunk = serde_json::json!({
            "error": {
                "message": format!("Upstream error: {} (code: {})", error_msg, error_code),
                "code": error_code
            }
        });

        let chunk = serde_json::json!({
            "id": self.chunk_id(),
            "object": "chat.completion.chunk",
            "created": self.created_at,
            "model": self.model_name,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "error"
            }],
            "error": error_chunk
        });
        let sse = format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap_or_default());
        let _ = tx.try_send(Ok(axum::body::Bytes::from(sse)));
    }

    /// 处理 response.done 事件（非标准，部分上游发送）—— 同 completed，不终结
    fn handle_response_done(&mut self, data: &serde_json::Value) {
        self.handle_response_completed(data);
    }

    /// 处理 output_item.done 事件
    fn handle_output_item_done(&mut self, data: &serde_json::Value) {
        if let Some(item) = data.get("item") {
            if let Some(item_type) = item.get("type").and_then(|t| t.as_str()) {
                if item_type == "function_call" {
                    self.has_tool_calls = true;
                }
            }
        }
    }

    /// 处理 content_part.done 和 output_text.done
    fn handle_content_part_done(&mut self) {
        // 不需要特别处理，文本已通过 delta 发送
    }

    /// 处理协议级 error 事件（type="error"）—— 上游明确报错 → 终结
    fn handle_error_event(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, String>>,
        data: &serde_json::Value,
    ) {
        if self.is_finished {
            return;
        }
        self.is_finished = true;
        let error_msg = data.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown upstream error");
        warn!("responses_bridge: upstream error event: {}", error_msg);

        let chunk = serde_json::json!({
            "id": self.chunk_id(),
            "object": "chat.completion.chunk",
            "created": self.created_at,
            "model": self.model_name,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "error"
            }],
            "error": {"message": error_msg}
        });
        let sse = format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap_or_default());
        let _ = tx.try_send(Ok(axum::body::Bytes::from(sse)));
    }
}

/// 将 Responses API SSE 流转换为 Chat Completions delta 流。
///
/// # 输入
/// 上游推送的 Responses API SSE 事件流（文本/字节流）。
///
/// # 输出
/// 标准的 OpenAI Chat Completions delta 流（data: {...}\n\n 格式）。
///
/// # 关键特性（线上 BUG 修复）
/// - 边接收边转发，无大包缓存，分片即时转发
/// - **终结信号只在流真正结束时下发**：response.completed / [DONE] 均不注入 stop
/// - 多轮工具调用（function_call → tool_calls）跨多 response 会话不串号
/// - 工具调用参数分片原样转发（不合并、不截断）
/// - SSE 心跳 `: ping` 保活；心跳计时与空闲超时解耦
/// - 解析异常分片跳过，不中断整条流
pub fn transform_responses_stream_to_chat(
    upstream_stream: impl futures::stream::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static,
    model: &str,
    config: BridgeConfig,
    hooks: Option<StreamHooks>,
) -> impl futures::stream::Stream<Item = Result<axum::body::Bytes, String>> + Send + 'static {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, String>>(128);
    let model_owned = model.to_string();
    let complete_hook = hooks.and_then(|h| h.on_complete);

    tokio::spawn(async move {
        let started = Instant::now();
        let mut st = StreamTranslatorState::new(complete_hook);
        if !model_owned.is_empty() {
            st.model_name = model_owned;
        }

        // 用于 SSE 分片重组的缓冲区
        let mut pending_sse_fragments: Vec<String> = Vec::new();

        // 跨上游 chunk 的「行」字节缓冲区。TCP 可能在任意字节位置切分，
        // 包括 JSON 中间、`data:` 前缀中间、以及多字节 UTF-8 字符中间。
        // 若按 chunk 独立解码分行，会导致：
        //   * 半行被当作完整行解析失败后丢弃；
        //   * `data:` 被切成 `data` + `: {...}`，后半段以 ':' 开头
        //     会被误判为心跳注释行而直接丢弃（内容永久丢失）；
        //   * from_utf8_lossy 把被截断的多字节字符替换为 U+FFFD，不可恢复。
        // 只把「以换行结尾的完整行」交给解析器可从根源消除以上三种损坏。
        let mut line_buf: Vec<u8> = Vec::new();

        // 心跳间隔（至少 1s，避免 0 值死循环）
        let heartbeat_interval = std::time::Duration::from_secs(config.heartbeat_interval_secs.max(1));
        // last_activity：任何上游字节到达都更新（心跳计时）
        let mut last_activity = Instant::now();
        // last_data：任何可处理的 SSE 行更新（空闲超时判定，心跳不重置它）
        let mut last_data = Instant::now();
        // 是否见过显式 [DONE]（仅标记，不作为结束信号）
        let mut seen_done = false;
        // 终结原因（日志用）
        let mut end_reason: &str = "upstream_eof";

        futures::pin_mut!(upstream_stream);

        // 主循环：读取上游 SSE 事件 → 翻译为 Chat delta → 发送
        loop {
            // 检查总时长
            if started.elapsed().as_secs() > config.session_max_duration_secs {
                warn!(
                    "responses_bridge: session max duration reached ({}s), forcing completion",
                    config.session_max_duration_secs
                );
                end_reason = "session_max_duration";
                break;
            }

            // 心跳计时（基于 last_activity）
            let time_since_activity = last_activity.elapsed();
            let next_heartbeat = if time_since_activity < heartbeat_interval {
                heartbeat_interval - time_since_activity
            } else {
                std::time::Duration::from_secs(0)
            };

            // 空闲/首片超时（基于 last_data）
            let idle_timeout = if st.has_sent_role {
                std::time::Duration::from_secs(config.chunk_idle_timeout_secs.max(1))
            } else {
                std::time::Duration::from_secs(config.first_chunk_timeout_secs.max(1))
            };
            let time_since_data = last_data.elapsed();
            let data_idle_remaining = idle_timeout.saturating_sub(time_since_data);

            // 等待上游数据：取「下一次心跳」与「空闲超时剩余」中更早的
            let timeout = std::cmp::min(next_heartbeat, data_idle_remaining);
            let chunk_result = tokio::time::timeout(timeout, upstream_stream.next()).await;

            match chunk_result {
                Ok(Some(Ok(chunk))) => {
                    last_activity = Instant::now();

                    // 追加原始字节，只处理到最后一个换行为止；其后的不完整行
                    // 留在缓冲区，等下一个 chunk 补全。
                    line_buf.extend_from_slice(&chunk);
                    let split_at = match line_buf.iter().rposition(|&b| b == b'\n') {
                        Some(pos) => pos + 1,
                        None => {
                            // 尚无完整行 —— 继续等待更多字节。
                            if line_buf.len() > SSE_LINE_BUFFER_LIMIT {
                                warn!(
                                    "responses_bridge: SSE 行缓冲超过 {} 字节仍无换行，丢弃",
                                    SSE_LINE_BUFFER_LIMIT
                                );
                                line_buf.clear();
                            }
                            continue;
                        }
                    };
                    let complete_bytes: Vec<u8> = line_buf.drain(..split_at).collect();

                    // 此处切片以换行结尾，不会有多字节字符跨越边界，可安全解码。
                    let chunk_str = String::from_utf8_lossy(&complete_bytes);
                    // 用 split_terminator 避免行尾换行产生「幽灵空行」——
                    // 那会被当作 SSE 事件边界，把仍在累积的多行 fragment 丢掉。
                    let lines: Vec<&str> = chunk_str.split_terminator('\n').collect();

                    for line in &lines {
                        let line = line.trim();
                        if line.is_empty() {
                            // SSE 空行表示事件结束，丢弃未完成的 fragment
                            if !pending_sse_fragments.is_empty() {
                                pending_sse_fragments.clear();
                            }
                            continue;
                        }

                        // 注释行（上游自身心跳）：视为存活信号，仅刷新数据时间
                        if line.starts_with(':') {
                            last_data = Instant::now();
                            continue;
                        }

                        // 提取 data: 部分
                        let data = if let Some(d) = line.strip_prefix("data:") {
                            d.trim()
                        } else if line.starts_with("event:") {
                            // event: 行 — 跳过，下一个 data: 行会携带事件数据
                            continue;
                        } else {
                            // 非 data: 行 — 尝试作为 JSON 解析（部分上游发送裸 JSON）
                            line.trim()
                        };

                        if data == "[DONE]" {
                            pending_sse_fragments.clear();
                            seen_done = true;
                            // ⚠️ 修复：不在这里终结流！某些上游在每段输出后发 [DONE]，
                            // 若立即下发 finish_reason，客户端会提前终止多轮工具调用。
                            // 继续读取，直到上游连接关闭（EOF）才统一收尾。
                            continue;
                        }

                        // 尝试解析 JSON（容错：坏块跳过，不中断流）
                        let json = match parse_sse_line(&mut pending_sse_fragments, data) {
                            Some(j) => j,
                            None => {
                                last_data = Instant::now();
                                continue;
                            }
                        };
                        last_data = Instant::now();

                        // 提取事件类型
                        let event_type = json.get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("");

                        match event_type {
                            "response.created" => {
                                st.handle_response_created(&json);
                            }
                            "response.in_progress" => {
                                // 忽略，已处理
                            }
                            "response.output_item.added" => {
                                st.handle_output_item_added(&tx, &json);
                            }
                            "response.output_text.delta" => {
                                st.handle_output_text_delta(&tx, &json);
                            }
                            "response.function_call_arguments.delta" => {
                                st.handle_function_call_args_delta(&tx, &json);
                            }
                            "response.output_text.done" => {
                                st.handle_content_part_done();
                            }
                            "response.content_part.done" => {
                                st.handle_content_part_done();
                            }
                            "response.content_part.added" => {
                                // 忽略，delta 会携带内容
                            }
                            "response.output_item.done" => {
                                st.handle_output_item_done(&json);
                            }
                            "response.completed" => {
                                // ⚠️ 修复：仅更新状态，不发送 finish
                                st.handle_response_completed(&json);
                            }
                            "response.failed" => {
                                // 硬错误 → 终结
                                st.handle_response_failed(&tx, &json);
                                end_reason = "response_failed";
                                break;
                            }
                            "response.done" => {
                                // 非标准，同 completed：仅更新状态
                                st.handle_response_done(&json);
                            }
                            "response.function_call_arguments.done" => {
                                // 参数累积完成，不需要额外操作（分片已原样转发）
                            }
                            "error" => {
                                // 协议级错误 → 终结
                                st.handle_error_event(&tx, &json);
                                end_reason = "error_event";
                                break;
                            }
                            "" => {
                                // 无 type 字段 — 可能是裸输出文本或未知格式
                                // 尝试作为标准 chat completion chunk 透传
                                if json.get("choices").is_some() {
                                    // 已经是 chat completion 格式，直接透传
                                    let sse = format!("data: {}\n\n", serde_json::to_string(&json).unwrap_or_default());
                                    let _ = tx.try_send(Ok(axum::body::Bytes::from(sse)));
                                }
                            }
                            _ => {
                                // 未知事件类型 — 可能包含我们需要的字段（容错提取）
                                if let Some(delta) = json.get("delta").and_then(|d| d.as_str()) {
                                    if !delta.is_empty() {
                                        st.send_text_delta(&tx, delta);
                                    }
                                }
                                if let Some(text) = json.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        st.send_text_delta(&tx, text);
                                    }
                                }
                            }
                        }
                    }

                    // 若因硬错误 break 出事件匹配，退出主循环
                    if st.is_finished && (end_reason == "response_failed" || end_reason == "error_event") {
                        break;
                    }
                }
                Ok(Some(Err(e))) => {
                    warn!("responses_bridge: upstream stream error: {}", e);
                    end_reason = "upstream_stream_error";
                    break;
                }
                Ok(None) => {
                    // 规范的 SSE 流以空行结束，此处行缓冲应为空。若仍有残留，
                    // 说明上游在一行中间切断了连接 —— 记录下来而非静默丢弃。
                    if !line_buf.is_empty() {
                        let leftover = String::from_utf8_lossy(&line_buf);
                        let preview: String = leftover.chars().take(400).collect();
                        warn!(
                            "responses_bridge: EOF 时行缓冲仍有 {} 字节未以换行结尾（上游在行中间断开）: {}",
                            line_buf.len(),
                            preview
                        );
                        line_buf.clear();
                    }
                    // 上游连接关闭 → 这才是「会话结束」的权威信号
                    end_reason = if seen_done { "upstream_eof" } else { "upstream_eof_no_done" };
                    break;
                }
                Err(_elapsed) => {
                    // 超时分支
                    if st.is_finished {
                        break;
                    }

                    // 心跳：超过心跳周期且没有新数据 → 下发注释心跳（不重置 last_data）
                    if last_activity.elapsed() >= heartbeat_interval {
                        let heartbeat = ": ping\n\n";
                        let _ = tx.try_send(Ok(axum::body::Bytes::from(heartbeat)));
                        last_activity = Instant::now();
                        continue;
                    }

                    // 空闲/首片超时：真正的死流 → 强制收尾
                    if last_data.elapsed() >= idle_timeout {
                        warn!(
                            "responses_bridge: idle timeout ({}s), forcing completion",
                            idle_timeout.as_secs()
                        );
                        end_reason = "idle_timeout";
                        break;
                    }
                }
            }
        }

        // 统一收尾：finish chunk + [DONE]（幂等）
        st.send_finish_chunk(&tx);
        let _ = tx.try_send(Ok(axum::body::Bytes::from("data: [DONE]\n\n")));

        let duration_ms = started.elapsed().as_millis();
        st.fire_complete_hook(duration_ms);

        info!(
            "responses_bridge: stream finished (reason={}, duration={:?}, has_text={}, has_tool_calls={})",
            end_reason,
            started.elapsed(),
            st.has_text_output,
            st.has_tool_calls
        );
    });

    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
}

// ────────────────────────────────────────────────────────────────────────────
// 3. 非流式响应转换：Responses API → Chat Completions
// ────────────────────────────────────────────────────────────────────────────

/// 将 Responses API 非流式响应体转换为 Chat Completions 格式。
///
/// # 输入（Responses API）
/// ```json
/// {
///   "id": "resp_xxx",
///   "object": "response",
///   "status": "completed",
///   "output": [
///     {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hello"}]},
///     {"type": "function_call", "id": "call_1", "call_id": "call_1", "name": "tool", "arguments": "{}"}
///   ],
///   "usage": {"input_tokens": 10, "output_tokens": 20}
/// }
/// ```
///
/// # 输出（Chat Completions）
/// ```json
/// {
///   "id": "chatcmpl-xxx",
///   "object": "chat.completion",
///   "choices": [{
///     "index": 0,
///     "message": {
///       "role": "assistant",
///       "content": "Hello",
///       "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "tool", "arguments": "{}"}}]
///     },
///     "finish_reason": "tool_calls"
///   }],
///   "usage": {"prompt_tokens": 10, "completion_tokens": 20}
/// }
/// ```
pub fn transform_responses_to_chat_completions(body_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut json: serde_json::Value = serde_json::from_slice(body_bytes).ok()?;
    let obj = json.as_object_mut()?;

    // 转换 id 格式
    if let Some(id) = obj.get("id").and_then(|i| i.as_str()) {
        if id.starts_with("resp_") || id.starts_with("resp-") {
            let chat_id = format!("chatcmpl-{}", id.trim_start_matches("resp_").trim_start_matches("resp-"));
            obj.insert("id".to_string(), serde_json::Value::String(chat_id));
        }
    }

    // 转换 object
    if let Some(object) = obj.get("object").and_then(|o| o.as_str()) {
        if object == "response" {
            obj.insert("object".to_string(), serde_json::Value::String("chat.completion".to_string()));
        }
    }

    // 转换 output → choices
    let output = obj.remove("output");
    let status = obj.get("status").and_then(|s| s.as_str()).unwrap_or("completed").to_string();

    if let Some(output) = output {
        if let Some(output_array) = output.as_array() {
            let mut content_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<serde_json::Value> = Vec::new();
            let mut has_tool_calls = false;

            for item in output_array {
                let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

                match item_type {
                    "message" => {
                        // 提取文本内容
                        if let Some(content) = item.get("content") {
                            extract_text_from_responses_content(content, &mut content_parts);
                        }
                        // 提取文本内容（直接字符串格式）
                        if let Some(text) = item.get("content").and_then(|c| c.as_str()) {
                            if !text.is_empty() {
                                content_parts.push(text.to_string());
                            }
                        }
                    }
                    "function_call" => {
                        has_tool_calls = true;
                        let id = item.get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                        let arguments = item.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}").to_string();

                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments
                            }
                        }));
                    }
                    "reasoning" => {
                        // Reasoning 在 Chat Completions 中没有标准位置，跳过
                    }
                    "output_text" | "output_image" | "output_file" | "output_audio" => {
                        // 这些是 content part 类型，不是 output item 类型
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                content_parts.push(text.to_string());
                            }
                        }
                    }
                    _ => {
                        // 未知类型，尝试提取文本
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                content_parts.push(text.to_string());
                            }
                        }
                    }
                }
            }

            // 构建 content
            let content = if content_parts.is_empty() {
                if has_tool_calls {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(String::new())
                }
            } else {
                serde_json::Value::String(content_parts.join(""))
            };

            // 构建 message
            let mut message = serde_json::json!({
                "role": "assistant",
                "content": content
            });

            if has_tool_calls && !tool_calls.is_empty() {
                message["tool_calls"] = serde_json::Value::Array(tool_calls);
            }

            // 确定 finish_reason（非流式：响应完整，直接判定）
            // 修复：incomplete（max_tokens 截断）→ "length"，不再误报 "error"
            let finish_reason = if status == "failed" {
                "error"
            } else if status == "incomplete" {
                "length"
            } else if has_tool_calls {
                // 如果输出中有 function_call，finish_reason = "tool_calls"
                "tool_calls"
            } else {
                "stop"
            };

            let choices = vec![serde_json::json!({
                "index": 0,
                "message": message,
                "finish_reason": finish_reason
            })];

            obj.insert("choices".to_string(), serde_json::Value::Array(choices));
        }
    }

    // 转换 usage
    if let Some(usage) = obj.get_mut("usage") {
        if let Some(usage_obj) = usage.as_object() {
            let mut new_usage = serde_json::Map::new();
            if let Some(input) = usage_obj.get("input_tokens") {
                new_usage.insert("prompt_tokens".to_string(), input.clone());
            }
            if let Some(output) = usage_obj.get("output_tokens") {
                new_usage.insert("completion_tokens".to_string(), output.clone());
            }
            if let Some(total) = usage_obj.get("total_tokens") {
                new_usage.insert("total_tokens".to_string(), total.clone());
            }
            if !new_usage.is_empty() {
                *usage = serde_json::Value::Object(new_usage);
            }
        }
    }

    // 移除 Responses API 特有字段
    obj.remove("status");
    obj.remove("output");
    obj.remove("object");
    // 重新设置 object（上面设置了但可能被 remove 影响）
    obj.insert("object".to_string(), serde_json::Value::String("chat.completion".to_string()));

    Some(serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec()))
}

/// 从 Responses API 的 content 数组中提取文本
fn extract_text_from_responses_content(content: &serde_json::Value, parts: &mut Vec<String>) {
    match content {
        serde_json::Value::Array(arr) => {
            for part in arr {
                let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match part_type {
                    "output_text" | "input_text" | "text" => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                parts.push(text.to_string());
                            }
                        }
                    }
                    "refusal" => {
                        if let Some(text) = part.get("refusal").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                parts.push(format!("[refusal: {}]", text));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        serde_json::Value::String(s) => {
            if !s.is_empty() {
                parts.push(s.clone());
            }
        }
        _ => {}
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 4. SSE 辅助工具
// ────────────────────────────────────────────────────────────────────────────

/// 最大累积的未解析 SSE 片段大小（字节）
const SSE_FRAGMENT_BUFFER_LIMIT: usize = 1024 * 1024;

/// 跨 chunk 保存「未以换行结尾的行尾字节」的缓冲上限。单条 SSE 行通常仅几 KB，
/// 该上限只用于防御「上游完全不发换行」的异常情况。
const SSE_LINE_BUFFER_LIMIT: usize = 8 * 1024 * 1024;

/// 解析一行 SSE 数据为 JSON 文档。
/// 处理分片重组：当一行数据不完整时，累积到缓冲区等待后续行补全。
/// 解析失败返回 None，调用方跳过该块（容错，不中断流）。
fn parse_sse_line(pending: &mut Vec<String>, data: &str) -> Option<serde_json::Value> {
    // 尝试直接解析
    let standalone = serde_json::from_str::<serde_json::Value>(data).ok();

    match standalone {
        Some(j) if pending.is_empty() => Some(j),
        Some(j) => {
            // 缓冲区有未完成的片段，但这一行可以独立解析
            // 尝试追加后合并解析
            pending.push(data.to_string());
            match try_parse_sse_fragments(pending) {
                Some(combined) => {
                    pending.clear();
                    Some(combined)
                }
                None => {
                    // 缓冲区无法合并，丢弃缓冲区使用当前行
                    pending.clear();
                    Some(j)
                }
            }
        }
        None => {
            // 无法独立解析，累积到缓冲区
            pending.push(data.to_string());
            match try_parse_sse_fragments(pending) {
                Some(combined) => {
                    pending.clear();
                    Some(combined)
                }
                None => {
                    // 仍然不完整
                    let total: usize = pending.iter().map(|s| s.len()).sum();
                    if total > SSE_FRAGMENT_BUFFER_LIMIT {
                        warn!("responses_bridge: SSE fragment buffer exceeded {} bytes, discarding", SSE_FRAGMENT_BUFFER_LIMIT);
                        pending.clear();
                    }
                    None
                }
            }
        }
    }
}

/// 尝试从累积的 SSE 片段中解析 JSON 文档。
/// 支持拼接和换行连接两种方式。
fn try_parse_sse_fragments(fragments: &[String]) -> Option<serde_json::Value> {
    // 直接拼接
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&fragments.concat()) {
        return Some(v);
    }
    // 换行连接
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&fragments.join("\n")) {
        return Some(v);
    }
    None
}

// ────────────────────────────────────────────────────────────────────────────
// 5. 测试
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── 请求转换测试 ──

    #[test]
    fn test_chat_to_responses_simple_text() {
        // 基本文本消息转换
        let body = br#"{
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 100
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        // 验证 instructions
        assert_eq!(v["instructions"], "You are helpful.");

        // 验证 input
        let input = v["input"].as_array().expect("input array");
        assert_eq!(input.len(), 1); // system 被提取为 instructions，只剩 user
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "Hello");

        // 验证 max_output_tokens
        assert_eq!(v["max_output_tokens"], 100);
        assert!(v.get("max_tokens").is_none(), "max_tokens should be removed");
    }

    #[test]
    fn test_chat_to_responses_tool_calls() {
        // 多轮工具调用场景
        let body = br#"{
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "List files"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "list_files", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "file1.txt\nfile2.txt"},
                {"role": "assistant", "content": "Here are the files: file1.txt, file2.txt"}
            ],
            "tools": [{"type": "function", "function": {"name": "list_files", "description": "List files", "parameters": {"type": "object"}}}]
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        let input = v["input"].as_array().expect("input array");
        // user(0) + function_call(1) + function_call_output(2) + assistant(3) = 4
        assert_eq!(input.len(), 4, "expected 4 input items");

        // 验证 user 消息
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");

        // 验证 function_call
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[1]["name"], "list_files");

        // 验证 function_call_output
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_1");
        assert_eq!(input[2]["output"], "file1.txt\nfile2.txt");

        // 验证 assistant 回复
        assert_eq!(input[3]["type"], "message");
        assert_eq!(input[3]["role"], "assistant");

        // 验证 tools 格式
        let tools = v["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 1);
        // Responses API 格式：扁平结构
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "list_files");
        assert!(tools[0].get("function").is_none(), "function should not be nested");
    }

    #[test]
    fn test_chat_to_responses_multi_tool_rounds() {
        // 连续多轮工具调用 — 模拟 Agent 场景
        let body = br#"{
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Search for weather and then send email"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_weather", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\": \"Beijing\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_weather", "content": "25C, Sunny"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_email", "type": "function", "function": {"name": "send_email", "arguments": "{\"to\": \"user@example.com\", \"body\": \"Weather is 25 C and Sunny\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_email", "content": "Email sent successfully"}
            ]
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        let input = v["input"].as_array().expect("input array");
        // user(0) + fc_weather(1) + fco_weather(2) + fc_email(3) + fco_email(4) = 5
        assert_eq!(input.len(), 5, "expected 5 input items");

        // 验证第一轮工具调用
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_weather");
        assert_eq!(input[1]["name"], "get_weather");

        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_weather");

        // 验证第二轮工具调用
        assert_eq!(input[3]["type"], "function_call");
        assert_eq!(input[3]["call_id"], "call_email");
        assert_eq!(input[3]["name"], "send_email");

        assert_eq!(input[4]["type"], "function_call_output");
        assert_eq!(input[4]["call_id"], "call_email");
    }

    #[test]
    fn test_chat_to_responses_response_format() {
        // response_format → text.format 转换
        let body = br#"{
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Generate JSON"}],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "answer",
                    "schema": {"type": "object", "properties": {"ok": {"type": "boolean"}}},
                    "strict": true
                }
            }
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        // 验证 text.format
        let text = v.get("text").expect("text field should exist");
        let format = text.get("format").expect("format should exist");
        assert_eq!(format["type"], "json_schema");
        assert_eq!(format["name"], "answer");
        assert_eq!(format["schema"]["type"], "object");
        assert_eq!(format["strict"], true);
        assert!(v.get("response_format").is_none(), "response_format should be removed");
    }

    // ── 请求转换：会话上下文（previous_response_id）测试 ──

    #[test]
    fn test_chat_to_responses_with_previous_response_id() {
        // 多轮续接：注入 previous_response_id
        let body = br#"{
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Search weather"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "search", "arguments": "{\"q\":\"weather\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "sunny"}
            ]
        }"#;

        let ctx = RequestContext {
            previous_response_id: Some("resp_abc123".to_string()),
            incremental_from: None,
        };
        let out = transform_chat_to_responses_request_ctx(body, &ctx).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        // 验证 previous_response_id
        assert_eq!(v["previous_response_id"], "resp_abc123");

        // 完整模式：input 包含所有消息
        let input = v["input"].as_array().expect("input array");
        assert_eq!(input.len(), 3);
    }

    #[test]
    fn test_chat_to_responses_incremental_mode() {
        // 增量模式：只转换 messages[2..]（最后一条 user 之后的新增消息）
        let body = br#"{
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Search weather"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "search", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "sunny"}
            ]
        }"#;

        let ctx = RequestContext {
            previous_response_id: Some("resp_abc123".to_string()),
            incremental_from: Some(2),
        };
        let out = transform_chat_to_responses_request_ctx(body, &ctx).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        // 验证 previous_response_id
        assert_eq!(v["previous_response_id"], "resp_abc123");

        // 增量模式：input 只包含 index=2 的 tool 结果
        let input = v["input"].as_array().expect("input array");
        assert_eq!(input.len(), 1, "incremental input should contain only new messages");
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["output"], "sunny");

        // 增量模式不应设置 instructions
        assert!(v.get("instructions").is_none(), "no instructions in incremental mode");
    }

    #[test]
    fn test_session_store_context() {
        // SessionStore 会话上下文：key 稳定 + previous_response_id 往返
        let store = SessionStore::new();

        let messages: Vec<serde_json::Value> = serde_json::from_str(r#"[
            {"role": "user", "content": "帮我查一下北京的天气"},
            {"role": "assistant", "content": null, "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "weather", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "c1", "content": "25C"}
        ]"#).unwrap();

        let key1 = SessionStore::key_for_request("gpt-4", &messages);
        // 同样的首条 user 消息 → 相同 key
        let key2 = SessionStore::key_for_request("gpt-4", &messages);
        assert_eq!(key1, key2, "session key should be stable");

        // 空 store：返回默认
        let ctx = store.get(&key1);
        assert!(ctx.previous_response_id.is_none());
        assert_eq!(ctx.processed_msg_len, 0);

        // 写入后读取
        store.update(&key1, SessionContext {
            previous_response_id: Some("resp_round1".to_string()),
            processed_msg_len: 3,
        });
        let ctx = store.get(&key1);
        assert_eq!(ctx.previous_response_id.as_deref(), Some("resp_round1"));
        assert_eq!(ctx.processed_msg_len, 3);

        // 清除
        store.clear(&key1);
        assert!(store.get(&key1).previous_response_id.is_none());
    }

    #[test]
    fn test_session_store_eviction() {
        // LRU 淘汰
        let store = SessionStore::with_capacity(2);
        store.update("k1", SessionContext { previous_response_id: Some("r1".into()), processed_msg_len: 1 });
        store.update("k2", SessionContext { previous_response_id: Some("r2".into()), processed_msg_len: 1 });
        store.update("k3", SessionContext { previous_response_id: Some("r3".into()), processed_msg_len: 1 });
        assert_eq!(store.len(), 2, "should evict oldest entry");
        assert!(store.get("k1").previous_response_id.is_none(), "k1 should be evicted");
        assert!(store.get("k3").previous_response_id.is_some());
    }

    // ── 非流式响应转换测试 ──

    #[test]
    fn test_responses_to_chat_simple() {
        let body = br#"{
            "id": "resp_abc123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4",
            "output": [
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hello!"}]}
            ],
            "usage": {"input_tokens": 5, "output_tokens": 3}
        }"#;

        let out = transform_responses_to_chat_completions(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        assert_eq!(v["object"], "chat.completion");
        assert!(v["id"].as_str().unwrap().starts_with("chatcmpl-"));
        assert_eq!(v["choices"][0]["message"]["role"], "assistant");
        assert_eq!(v["choices"][0]["message"]["content"], "Hello!");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["prompt_tokens"], 5);
        assert_eq!(v["usage"]["completion_tokens"], 3);
    }

    #[test]
    fn test_responses_to_chat_with_tool_calls() {
        let body = br#"{
            "id": "resp_abc123",
            "object": "response",
            "status": "completed",
            "output": [
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Let me check."}]},
                {"type": "function_call", "id": "fc_1", "call_id": "call_abc", "name": "get_weather", "arguments": "{\"city\": \"Beijing\"}"}
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        }"#;

        let out = transform_responses_to_chat_completions(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        let msg = &v["choices"][0]["message"];
        assert_eq!(msg["content"], "Let me check.");
        assert_eq!(msg["role"], "assistant");

        let tool_calls = msg["tool_calls"].as_array().expect("tool_calls array");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "call_abc");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
        assert_eq!(tool_calls[0]["function"]["arguments"], "{\"city\": \"Beijing\"}");

        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn test_responses_to_chat_incomplete_status() {
        // incomplete（max_tokens 截断）应映射为 length，而不是 error
        let body = br#"{
            "id": "resp_incomplete",
            "object": "response",
            "status": "incomplete",
            "output": [
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Partial..."}]}
            ]
        }"#;

        let out = transform_responses_to_chat_completions(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert_eq!(v["choices"][0]["finish_reason"], "length");
    }

    // ── 流式转换测试 ──

    /// 辅助：创建模拟上游流
    fn mock_upstream_stream(
        events: Vec<&str>,
    ) -> impl futures::stream::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static {
        let chunks: Vec<Result<axum::body::Bytes, reqwest::Error>> = events
            .into_iter()
            .map(|s| Ok(axum::body::Bytes::from(s.to_string())))
            .collect();
        futures::stream::iter(chunks)
    }

    /// 收集流式输出到字符串列表
    async fn collect_stream(
        stream: impl futures::stream::Stream<Item = Result<axum::body::Bytes, String>> + Send + 'static,
    ) -> Vec<String> {
        futures::pin_mut!(stream);
        let mut results = Vec::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    let s = String::from_utf8_lossy(&bytes).to_string();
                    results.push(s);
                }
                Err(e) => {
                    results.push(format!("ERROR: {}", e));
                }
            }
        }
        results
    }

    #[tokio::test]
    async fn test_stream_simple_text() {
        // 简单的文本输出流
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_abc\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" World\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_abc\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello World\"}]}]}}\n\n",
            "data: [DONE]\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
        let results = collect_stream(stream).await;

        // 验证输出
        let output: String = results.iter().map(|s| s.as_str()).collect();
        assert!(output.contains("data: "), "should contain SSE data");
        assert!(output.contains("[DONE]"), "should end with [DONE]");

        // 验证包含 content delta
        assert!(output.contains("Hello"), "should contain 'Hello'");
        assert!(output.contains("World"), "should contain 'World'");

        // 验证有 role 初始 chunk
        assert!(output.contains("\"role\":\"assistant\""), "should set role");

        // 验证 finish_reason 出现在流末尾（而非 completed 事件处）
        let stop_pos = output.find("\"finish_reason\":\"stop\"");
        let done_pos = output.find("[DONE]");
        assert!(stop_pos.is_some(), "should end with stop");
        assert!(done_pos.is_some());
        assert!(stop_pos.unwrap() < done_pos.unwrap(), "finish should come before [DONE]");
    }

    #[tokio::test]
    async fn test_stream_tool_call() {
        // 工具调用场景
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_abc\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"I will check the weather.\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_weather\",\"name\":\"get_weather\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"city\\\":\",\"item_id\":\"fc_1\",\"output_index\":1}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\" \\\"Beijing\\\"}\",\"item_id\":\"fc_1\",\"output_index\":1}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_abc\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"I will check the weather.\"}]},{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_weather\",\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\": \\\"Beijing\\\"}\"}]}}\n\n",
            "data: [DONE]\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
        let results = collect_stream(stream).await;

        let output: String = results.iter().map(|s| s.as_str()).collect();

        // 验证包含文本
        assert!(output.contains("I will check the weather."));

        // 验证工具调用
        assert!(output.contains("tool_calls"), "should contain tool_calls");
        assert!(output.contains("call_weather"), "should contain tool call id");
        assert!(output.contains("get_weather"), "should contain tool name");
        assert!(output.contains("Beijing"), "should contain argument");

        // 验证 finish_reason 为 tool_calls（流末尾）
        assert!(output.contains("\"finish_reason\":\"tool_calls\""), "finish_reason should be tool_calls");
    }

    #[tokio::test]
    async fn test_stream_multi_round_tool_calls() {
        // 连续多轮工具调用 — Agent 场景（单响应内多个工具调用交错）
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_multi\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            // 第一个工具调用
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_search\",\"name\":\"search_tool\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"query\\\":\",\"item_id\":\"fc_1\",\"output_index\":0}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\" \\\"weather\\\"}\",\"item_id\":\"fc_1\",\"output_index\":0}\n\n",
            // 第二个工具调用
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_email\",\"name\":\"send_email\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"to\\\":\\\"a@b.com\\\",\",\"item_id\":\"fc_2\",\"output_index\":1}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"\\\"body\\\":\\\"Done\\\"}\",\"item_id\":\"fc_2\",\"output_index\":1}\n\n",
            // 完成
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_multi\",\"status\":\"completed\",\"output\":[{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_search\",\"name\":\"search_tool\",\"arguments\":\"{\\\"query\\\": \\\"weather\\\"}\"},{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_email\",\"name\":\"send_email\",\"arguments\":\"{\\\"to\\\":\\\"a@b.com\\\",\\\"body\\\":\\\"Done\\\"}\"}]}}\n\n",
            "data: [DONE]\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
        let results = collect_stream(stream).await;

        let output: String = results.iter().map(|s| s.as_str()).collect();

        // 验证两个工具调用都在
        assert!(output.contains("call_search"), "should contain first call id");
        assert!(output.contains("call_email"), "should contain second call id");
        assert!(output.contains("search_tool"), "should contain first tool name");
        assert!(output.contains("send_email"), "should contain second tool name");

        // 验证参数
        assert!(output.contains("weather"), "should contain search query");
        assert!(output.contains("a@b.com"), "should contain email recipient");

        // 验证索引正确
        assert!(output.contains("\"index\":0"), "first tool call should have index 0");
        assert!(output.contains("\"index\":1"), "second tool call should have index 1");

        // 验证 finish_reason（流末尾，tool_calls）
        assert!(output.contains("\"finish_reason\":\"tool_calls\""));
    }

    // ════════════════════════════════════════════════════════════════════════
    // 线上 BUG 回归测试：多轮工具调用会话不被提前终止
    // ════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_stream_multi_response_no_premature_stop() {
        // 核心回归：上游在同一个连接里连续推送两个 response——
        //   response1：纯文本（模型阶段性回复）
        //   response2：function_call（模型要继续调用工具）
        // 旧实现会在 response1.completed 时下发 finish_reason=stop，
        // 客户端收到后终止会话，模型无法发起下一轮工具调用。
        // 修复后：整个流结束时才下发 finish_reason=tool_calls，且绝不出现 stop。
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"我已经搜索到第一轮结果，\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"接下来继续查找更多资料。\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"我已经搜索到第一轮结果，接下来继续查找更多资料。\"}]}]}}\n\n",
            // 关键：response.completed 之后，上游继续推送第二个 response（function_call）
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_2\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_search_2\",\"name\":\"web_search\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"query\\\":\",\"item_id\":\"fc_2\",\"output_index\":0}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"\\\"deep research\\\"}\",\"item_id\":\"fc_2\",\"output_index\":0}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"status\":\"completed\",\"output\":[{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_search_2\",\"name\":\"web_search\",\"arguments\":\"{\\\"query\\\":\\\"deep research\\\"}\"}]}}\n\n",
            "data: [DONE]\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
        let results = collect_stream(stream).await;

        let output: String = results.iter().map(|s| s.as_str()).collect();

        // 1. 两轮内容都在
        assert!(output.contains("我已经搜索到第一轮结果"), "first response text must be present");
        assert!(output.contains("call_search_2"), "second response tool call must be present");
        assert!(output.contains("web_search"), "second response tool name must be present");

        // 2. 关键断言：整个流中绝不出现 finish_reason=stop
        assert!(
            !output.contains("\"finish_reason\":\"stop\""),
            "must NOT inject stop mid-stream: {}",
            output
        );

        // 3. 流末尾唯一一次 finish_reason = tool_calls
        //    （每个普通 chunk 都携带 "finish_reason":null，因此只统计带值出现次数）
        assert!(output.contains("\"finish_reason\":\"tool_calls\""), "final finish should be tool_calls");
        assert_eq!(output.matches("\"finish_reason\":\"tool_calls\"").count(), 1, "only one valued finish_reason");
        assert_eq!(output.matches("\"finish_reason\":\"stop\"").count(), 0, "no stop ever");
    }

    #[tokio::test]
    async fn test_stream_done_midstream_not_terminate() {
        // 回归：上游在每段输出之间发送 [DONE]（非标准但存在），
        // 旧实现收到 [DONE] 即下发 finish，客户端提前终止。
        // 修复后：[DONE] 仅标记，流继续，直到 EOF 才收尾。
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_done\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_a\",\"name\":\"tool_a\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"a\\\":1}\",\"item_id\":\"fc_1\",\"output_index\":0}\n\n",
            // 中途 [DONE]
            "data: [DONE]\n\n",
            // [DONE] 之后仍然有新的 function_call（另一段输出）
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_b\",\"name\":\"tool_b\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"b\\\":2}\",\"item_id\":\"fc_2\",\"output_index\":1}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_done\",\"status\":\"completed\",\"output\":[{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_a\",\"name\":\"tool_a\",\"arguments\":\"{\\\"a\\\":1}\"},{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_b\",\"name\":\"tool_b\",\"arguments\":\"{\\\"b\\\":2}\"}]}}\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
        let results = collect_stream(stream).await;

        let output: String = results.iter().map(|s| s.as_str()).collect();

        // [DONE] 之后的内容仍在
        assert!(output.contains("call_b"), "tool call after mid-stream [DONE] must be delivered");
        assert!(output.contains("tool_b"), "tool name after [DONE] must be delivered");

        // 无 stop；最终恰好一次 tool_calls（普通 chunk 携带 finish_reason:null，不计入）
        assert_eq!(output.matches("\"finish_reason\":\"stop\"").count(), 0, "must not inject stop");
        assert_eq!(output.matches("\"finish_reason\":\"tool_calls\"").count(), 1, "only one valued finish_reason");
    }

    #[tokio::test]
    async fn test_stream_arg_deltas_forwarded_as_is() {
        // 规范 3/5：参数分片原样即时转发（不合并、不截断），时序一致
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_frag\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_x\",\"name\":\"search\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"q\\\":\",\"item_id\":\"fc_1\",\"output_index\":0}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\" \\\"x\\\",\\\"n\\\":\",\"item_id\":\"fc_1\",\"output_index\":0}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"1}\",\"item_id\":\"fc_1\",\"output_index\":0}\n\n",
            "data: [DONE]\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
        let results = collect_stream(stream).await;

        let output: String = results.iter().map(|s| s.as_str()).collect();

        // 三段参数 delta 各自独立出现在输出中（未被合并成一个大块）
        assert_eq!(output.matches("\\\"q\\\":").count(), 1, "first fragment");
        assert!(output.contains(" \\\"x\\\",\\\"n\\\":"), "second fragment must be forwarded as-is");
        assert!(output.contains("1}"), "third fragment");

        // 4 个 arguments 字段 = 初始 delta（空串）+ 3 个参数分片，全部原样
        assert_eq!(output.matches("\"arguments\":\"").count(), 4, "initial(1) + three arg deltas(3)");
    }

    #[tokio::test]
    async fn test_stream_malformed_chunk_skipped() {
        // 规范 6：解析异常分片被跳过，流不中断
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_bad\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {this is not valid json\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"still alive\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_bad\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"still alive\"}]}]}}\n\n",
            "data: [DONE]\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
        let results = collect_stream(stream).await;

        let output: String = results.iter().map(|s| s.as_str()).collect();
        assert!(output.contains("still alive"), "stream must survive malformed chunk");
        assert!(output.contains("[DONE]"), "stream must end normally");
        assert!(output.contains("\"finish_reason\":\"stop\""));
    }

    #[tokio::test]
    async fn test_stream_keepalive_heartbeat() {
        // 规范 4：验证心跳机制：长时间无数据时发送 : ping
        let config = BridgeConfig {
            heartbeat_interval_secs: 1,  // 1 秒心跳
            first_chunk_timeout_secs: 60,
            chunk_idle_timeout_secs: 60,
            session_max_duration_secs: 4,  // 4 秒后自动结束
            ..Default::default()
        };

        // 使用 channel 创建可控流：先发送一些事件，然后保持打开
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, reqwest::Error>>(64);

        // 发送初始事件
        let init_events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_heartbeat\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
        ];
        for event in init_events {
            let _ = tx.send(Ok(axum::body::Bytes::from(event.to_string()))).await;
        }
        // 保持 tx 不关闭，流保持打开状态

        // 创建一个从 channel 读取的流
        let upstream = futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });

        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", config, None);
        let results = collect_stream(stream).await;

        let output: String = results.iter().map(|s| s.as_str()).collect();

        // 验证心跳（: ping 注释行）
        assert!(output.contains(": ping"), "should contain ping heartbeats");

        // 验证文本内容
        assert!(output.contains("Hi"), "should contain text content");

        // 验证最终有 [DONE]
        assert!(output.contains("[DONE]"), "should end with [DONE]");
    }

    #[tokio::test]
    async fn test_stream_idle_timeout() {
        // 验证空闲超时：长时间无数据后强制完成（心跳不应无限续命死流）
        let config = BridgeConfig {
            heartbeat_interval_secs: 1,   // 1 秒心跳（会持续发）
            first_chunk_timeout_secs: 5,  // 5 秒首次超时
            chunk_idle_timeout_secs: 5,   // 5 秒空闲超时
            session_max_duration_secs: 30,
            ..Default::default()
        };

        // 只发送 response.created 然后停止
        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_timeout\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", config, None);
        let started = Instant::now();
        let results = collect_stream(stream).await;
        let elapsed = started.elapsed();

        let output: String = results.iter().map(|s| s.as_str()).collect();

        // 死流必须在 idle 超时（5s）附近被清理，而不是被心跳无限续命
        assert!(elapsed.as_secs() < 15, "idle timeout must fire despite heartbeats (elapsed={}s)", elapsed.as_secs());

        // 最终有完成标记
        assert!(output.contains("finish_reason"), "should have finish_reason");
        assert!(output.contains("[DONE]"), "should end with [DONE]");
    }

    #[tokio::test]
    async fn test_stream_complete_hook_fires() {
        // 流结束时钩子被触发并回传 response_id
        let hook_response_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let hook_calls: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
        let sink = hook_response_id.clone();
        let calls = hook_calls.clone();
        let hooks = StreamHooks {
            on_complete: Some(Arc::new(move |rid, _ht, _htx, _dur| {
                *sink.lock().unwrap() = rid;
                *calls.lock().unwrap() += 1;
            })),
        };

        let events = vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_hook\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"done\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_hook\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\"}]}]}}\n\n",
            "data: [DONE]\n\n",
        ];

        let upstream = mock_upstream_stream(events);
        let stream = transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), Some(hooks));
        let results = collect_stream(stream).await;
        assert!(!results.is_empty());

        assert_eq!(*hook_calls.lock().unwrap(), 1, "hook should fire exactly once");
        assert_eq!(hook_response_id.lock().unwrap().as_deref(), Some("resp_hook"));
    }

    #[test]
    fn test_chat_to_responses_stream_flag() {
        // stream=true 应该透传
        let body = br#"{
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stream": true
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert_eq!(v["stream"], true, "stream flag should be preserved");
    }

    #[test]
    fn test_chat_to_responses_tool_choice_auto() {
        let body = br#"{
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "auto"
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert_eq!(v["tool_choice"], "auto", "string tool_choice should pass through");
    }

    #[test]
    fn test_chat_to_responses_tool_choice_function() {
        // Chat Completions: {"type": "function", "function": {"name": "my_tool"}}
        // Responses API: {"type": "function", "name": "my_tool"}
        let body = br#"{
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {"type": "function", "function": {"name": "my_tool"}}
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let tc = v.get("tool_choice").expect("tool_choice should exist");
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["name"], "my_tool", "name should be at top level");
        assert!(tc.get("function").is_none(), "function nesting should be removed");
    }

    #[test]
    fn test_responses_to_chat_no_output() {
        // 空输出
        let body = br#"{
            "id": "resp_empty",
            "object": "response",
            "status": "completed",
            "output": []
        }"#;

        let out = transform_responses_to_chat_completions(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert_eq!(v["choices"][0]["message"]["content"], "");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn test_chat_to_responses_image_content() {
        // 多模态内容转换
        let body = br#"{
            "model": "gpt-4",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is in this image?"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/img.jpg"}}
                ]
            }]
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        let content = &v["input"][0]["content"];
        assert_eq!(content[0]["type"], "input_text", "text should become input_text");
        assert_eq!(content[0]["text"], "What is in this image?");
        assert_eq!(content[1]["type"], "input_image", "image_url should become input_image");
        assert_eq!(content[1]["image_url"]["url"], "https://example.com/img.jpg");
    }

    #[test]
    fn test_chat_to_responses_assistant_text_before_tool_calls() {
        // Assistant 消息既有文本又有 tool_calls
        let body = br#"{
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Check weather in Beijing"},
                {"role": "assistant", "content": "I'll check the weather for you.", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Beijing\"}"}}]}
            ]
        }"#;

        let out = transform_chat_to_responses_request(body).expect("should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");

        let input = v["input"].as_array().expect("input array");
        // user + assistant_text + function_call = 3
        assert_eq!(input.len(), 3);

        // assistant 文本消息
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
        assert_eq!(input[1]["content"][0]["text"], "I'll check the weather for you.");

        // function_call
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["name"], "get_weather");
    }

    /// 把整段 SSE 文本按固定字节长度切片投喂，模拟真实 TCP 任意位置分包。
    fn mock_upstream_split_by_bytes(
        transcript: &str,
        chunk_size: usize,
    ) -> impl futures::stream::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static
    {
        let chunks: Vec<Result<axum::body::Bytes, reqwest::Error>> = transcript
            .as_bytes()
            .chunks(chunk_size)
            .map(|c| Ok(axum::body::Bytes::copy_from_slice(c)))
            .collect();
        futures::stream::iter(chunks)
    }

    #[tokio::test]
    async fn test_stream_survives_arbitrary_byte_chunking() {
        // 回归：TCP 在任意字节位置分包时，按 chunk 独立解码分行会导致
        //   * 半行被当完整行解析失败后丢弃；
        //   * `data:` 被切成 `data` + `: {...}`，后半段以 ':' 开头被误判为
        //     心跳注释行而丢弃 —— 内容永久丢失；
        //   * 多字节 UTF-8 被截断成 U+FFFD。
        // 结果是客户端收到空回复（表现为「对话中断 / 没有回复」）。
        let transcript = [
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_split\",\"model\":\"gpt-4\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"页面能访问，状态码 200。\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"让我看看内容。\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_split\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"页面能访问，状态码 200。让我看看内容。\"}]}]}}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat();

        for chunk_size in [1usize, 2, 3, 5, 7, 13, 64, 137] {
            let upstream = mock_upstream_split_by_bytes(&transcript, chunk_size);
            let stream =
                transform_responses_stream_to_chat(upstream, "gpt-4", BridgeConfig::default(), None);
            let output: String = collect_stream(stream).await.concat();

            assert!(
                output.contains("状态码 200"),
                "{} 字节分包下多字节文本不得损坏；实际: {}",
                chunk_size,
                output
            );
            assert!(
                output.contains("让我看看内容"),
                "{} 字节分包下后续 delta 不得丢失；实际: {}",
                chunk_size,
                output
            );
            assert!(
                !output.contains('\u{FFFD}'),
                "{} 字节分包下不得出现 U+FFFD 替换字符；实际: {}",
                chunk_size,
                output
            );
        }
    }
}
