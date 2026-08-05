use futures::StreamExt;
use tracing::{info, warn, error};

// ── Responses API ↔ Chat Completions API translation ──
// Codex CLI uses the OpenAI Responses API (/v1/responses), but most
// upstream providers only support Chat Completions (/v1/chat/completions).
// These functions translate between the two formats transparently.

/// Translate a Responses API request body to Chat Completions API format.
///
/// Responses API format:  {"model":"...","input":"...","max_output_tokens":...}
/// Chat Completions format: {"model":"...","messages":[...],"max_tokens":...}
pub fn transform_responses_to_chat_completions(body_bytes: &[u8]) -> Option<Vec<u8>> {
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
            let mut arr_clone = arr.clone();
            for mut msg in arr_clone.drain(..) {
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

    // ── Translate `reasoning` (Responses API) → `reasoning_effort` (Chat Completions) ──
    // Codex CLI sends `{"reasoning": {"effort": "medium"}}`, but OpenAI-compatible
    // reasoning providers (DeepSeek, Qwen, Kimi, etc.) expect a top-level string
    // `reasoning_effort`. Passing the `reasoning` object through makes strict
    // upstreams reject the whole request with HTTP 400.
    if let Some(reasoning) = obj.remove("reasoning") {
        let effort = if let Some(e) = reasoning.get("effort").and_then(|e| e.as_str()) {
            Some(e.to_string())
        } else if let Some(e) = reasoning.as_str() {
            // Some clients send a bare string instead of an object
            Some(e.to_string())
        } else {
            None
        };
        if let Some(effort) = effort {
            if !obj.contains_key("reasoning_effort") {
                obj.insert("reasoning_effort".to_string(), serde_json::Value::String(effort));
            }
        }
        // The `reasoning` object itself is dropped — chat completions upstreams
        // (other than OpenAI) do not understand it.
    }

    // ── Convert `tool_choice` from Responses API format to Chat Completions format ──
    // Responses API:   {"type": "function", "name": "..."}
    // Chat Completions: {"type": "function", "function": {"name": "..."}}
    // String values like "auto" / "none" / "required" pass through unchanged.
    if let Some(tool_choice) = obj.get_mut("tool_choice") {
        if let Some(tc) = tool_choice.as_object_mut() {
            if tc.contains_key("name") && !tc.contains_key("function") {
                if let Some(name) = tc.remove("name") {
                    tc.insert("function".to_string(), serde_json::json!({ "name": name }));
                }
            }
        }
    }

    // ── Translate Responses API `text.format` (structured output) → `response_format` ──
    // The `text` param is Responses-only; strict upstreams reject it.
    if let Some(text) = obj.remove("text") {
        if let Some(fmt) = text.get("format") {
            if !obj.contains_key("response_format") {
                obj.insert("response_format".to_string(), fmt.clone());
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

    // Remove Responses API-specific fields that don't exist in Chat Completions.
    // Strict third-party upstreams reject unknown fields with HTTP 400, so drop
    // every Responses-only param instead of passing it through.
    obj.remove("previous_response_id");
    obj.remove("store");
    obj.remove("include");
    obj.remove("truncation");

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
pub fn transform_chat_completions_to_responses(body_bytes: &[u8]) -> Option<Vec<u8>> {
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
                    // Some providers (like Agnes) return thinking content inline in the
                    // text with a >think prefix. We detect and split it here, emitting
                    // a separate reasoning output item so Codex CLI can display it as
                    // a collapsible thinking block instead of raw text.
                    let has_text = message.get("content").and_then(|c| c.as_str()).map_or(false, |s| !s.is_empty());
                    if has_text {
                        if let Some(content_str) = message.get("content").and_then(|c| c.as_str()) {
                            let (reasoning_text, actual_text) = split_thinking_content(content_str);

                            // Emit reasoning output item if thinking content was found
                            if let Some(rt) = reasoning_text {
                                if !rt.is_empty() {
                                    output.push(serde_json::json!({
                                        "type": "reasoning",
                                        "reasoning": rt
                                    }));
                                }
                            }

                            // Emit text output item for the remaining (actual) content
                            let text_to_emit = actual_text.as_deref().unwrap_or(content_str);
                            if !text_to_emit.is_empty() {
                                output.push(serde_json::json!({
                                    "type": "message",
                                    "role": role,
                                    "content": [
                                        {"type": "output_text", "text": text_to_emit}
                                    ]
                                }));
                            }
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
                    // Support multiple field names: reasoning_content, reasoning, thinking, thinking_content
                    if let Some(reasoning) = extract_reasoning_content(message) {
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

    // ── Complete the Responses API contract ──
    // Upstream chat completions responses never carry `status` / `created_at`,
    // but Codex CLI (and other Responses API clients) expect them. Without
    // `status` the client may treat the response as incomplete and stall.
    if obj.contains_key("error") {
        obj.insert("status".to_string(), serde_json::json!("failed"));
    } else if !obj.contains_key("status") {
        obj.insert("status".to_string(), serde_json::json!("completed"));
    }
    // Map `created` (unix seconds) → `created_at` (Responses API field name).
    if let Some(created) = obj.remove("created") {
        if !obj.contains_key("created_at") {
            obj.insert("created_at".to_string(), created);
        }
    }
    // Always carry a non-empty response id.
    if obj.get("id").and_then(|v| v.as_str()).map_or(true, |s| s.is_empty()) {
        obj.insert(
            "id".to_string(),
            serde_json::json!(format!("resp_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))),
        );
    }

    // Remove Chat Completions-specific fields
    obj.remove("choices");

    Some(serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec()))
}

/// Maximum time (seconds) to wait for the first valid SSE chunk before
/// forcing stream completion. Prevents hung streams when a non-standard
/// upstream sends chunks that don't match the expected OpenAI SSE format.
/// Must be generous enough to cover slow first tokens from reasoning models.
const STREAM_FIRST_CHUNK_TIMEOUT_SECS: u64 = 30;

/// Maximum idle time (seconds) allowed between consecutive SSE chunks.
/// Reasoning models (DeepSeek, Agnes, etc.) can pause for a long time between
/// chunks while "thinking" (or when the relay buffers output), so this must be
/// generous — an aggressive idle timeout truncates the stream mid-conversation
/// and makes Codex end the turn with no output. The reqwest client's 300s
/// total timeout still caps the overall stream length.
const STREAM_CHUNK_IDLE_TIMEOUT_SECS: u64 = 120;

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
pub fn transform_stream_to_responses(
    upstream_stream: impl futures::stream::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static,
    model: &str,
) -> impl futures::stream::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, reqwest::Error>>(64);
    let model_owned = model.to_string();

    tokio::spawn(async move {
        info!("transform_stream_to_responses: stream processing started");

        let now = chrono::Utc::now().timestamp();
        let mut st = StreamState {
            // Generate the response id upfront so every emitted event has a
            // valid id even when the upstream stream turns out to be empty
            // (previously an empty upstream stream produced a "response.completed"
            // with id="" which Codex could not associate with any response).
            response_id: format!("resp_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
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
            reasoning_buffer: String::new(),
            reasoning_item_id: String::new(),
            has_sent_reasoning: false,
            reasoning_output_index: 0,
            is_thinking_block: false,
            tool_call_ids: std::collections::HashMap::new(),
            tool_call_names: std::collections::HashMap::new(),
            tool_call_args: std::collections::HashMap::new(),
            tool_call_output_index: 1,
            now,
        };

        // ── Main stream processing loop with timeout ──
        futures::pin_mut!(upstream_stream);

        loop {
            // Apply an idle timeout: if the stream hasn't produced a valid
            // "choices" chunk within the expected window, force completion.
            // The per-chunk idle timeout must be generous — reasoning models
            // can pause for a long time between chunks.
            let timeout_duration = if st.has_sent_created {
                std::time::Duration::from_secs(STREAM_CHUNK_IDLE_TIMEOUT_SECS)
            } else {
                std::time::Duration::from_secs(STREAM_FIRST_CHUNK_TIMEOUT_SECS)
            };

            let chunk_result = tokio::time::timeout(timeout_duration, upstream_stream.next()).await;

            let chunk = match chunk_result {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    warn!("transform_stream_to_responses: upstream stream error: {}", e);
                    // Emit error as a Responses API error event if we haven't completed
                    if !st.is_completed {
                        st.send_error_response(&tx, &format!("Upstream stream error: {}", e)).await;
                    }
                    let _ = tx.send(Err(e)).await;
                    break;
                }
                Ok(None) => {
                    // Stream ended naturally
                    if st.has_sent_created && !st.is_completed {
                        info!("transform_stream: upstream stream ended naturally, flushing remaining events");
                        st.flush_done_events(&tx).await;
                        st.send_completed(&tx).await;
                    } else if !st.is_completed {
                        // Stream ended before any valid chunk — this can happen
                        // if the upstream returns an empty body (e.g., 204 No Content
                        // disguised as a 200), a non-SSE response, or a body whose
                        // format we couldn't parse. Surface this as a FAILED
                        // response instead of a silent empty "completed" event
                        // (which previously made Codex end the turn with no output).
                        warn!("transform_stream: upstream stream ended without any valid data");
                        st.send_error_response(&tx, "Upstream returned an empty or unparseable streaming response. Check the upstream provider / relay for errors.").await;
                    }
                    break;
                }
                Err(_elapsed) => {
                    // Timeout: no chunk received within the expected window.
                    // This can happen if the upstream sends SSE in a non-standard
                    // format that gets silently skipped by our parser, causing
                    // the stream to "hang" from the client's perspective.
                    if st.is_completed {
                        break;
                    }
                    if !st.has_sent_created {
                        warn!("transform_stream: timeout waiting for first valid chunk ({}s). Upstream may be using non-standard SSE format.", STREAM_FIRST_CHUNK_TIMEOUT_SECS);
                        // Send a minimal error response so Codex CLI doesn't hang
                        st.send_error_response(&tx, &format!(
                            "Upstream did not return a valid streaming response within {} seconds. The provider may use an unsupported SSE format.", STREAM_FIRST_CHUNK_TIMEOUT_SECS
                        )).await;
                    } else {
                        warn!("transform_stream: timeout waiting for next chunk ({}s), forcing completion", STREAM_CHUNK_IDLE_TIMEOUT_SECS);
                        st.flush_done_events(&tx).await;
                        st.send_completed(&tx).await;
                    }
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
                let data = if let Some(d) = line.strip_prefix("data:") {
                    // Standard "data: {...}" — also handles relays/providers that
                    // emit "data:{...}" without a space after the colon.
                    d.trim()
                } else {
                    // Non-"data:" line — some relays/providers emit raw JSON
                    // lines (NDJSON) without the SSE prefix. Try parsing it as a
                    // chunk below; non-JSON lines (event:/comment lines) are
                    // skipped with a warning.
                    line.trim()
                };

                if data == "[DONE]" {
                    // Guard against duplicate terminal events: some relays emit
                    // multiple [DONE] lines, or an error may already have been
                    // emitted for this stream.
                    if st.is_completed {
                        continue;
                    }
                    if st.has_sent_created {
                        st.flush_done_events(&tx).await;
                        st.send_completed(&tx).await;
                    } else {
                        // [DONE] arrived with no valid chunk before it — the
                        // upstream sent an empty stream. Surface an error rather
                        // than a silent empty completion.
                        warn!("transform_stream: [DONE] received before any valid chunk (empty upstream stream)");
                        st.send_error_response(&tx, "Upstream returned no streaming data ([DONE] with empty body).").await;
                    }
                    continue;
                }

                let json: serde_json::Value = match serde_json::from_str(data) {
                    Ok(j) => j,
                    Err(_) => {
                        warn!("transform_stream: skipping non-JSON SSE data: {:.80}", data);
                        continue;
                    }
                };

                // Prefer the upstream chunk id for continuity; keep the
                // pre-generated UUID when the upstream provides none.
                if let Some(id) = json.get("id").and_then(|i| i.as_str()) {
                    if !id.is_empty() {
                        st.response_id = format!("resp_{}", id.trim_start_matches("chatcmpl-"));
                    }
                }

                if st.model_name.is_empty() {
                    if let Some(m) = json.get("model").and_then(|m| m.as_str()) {
                        st.model_name = m.to_string();
                    }
                }

                // ── Handle error chunks ──
                // Some upstream providers send error responses in the SSE stream
                // (e.g., {"error": {"message": "...", "code": ...}}) instead of
                // standard choices. We translate these into a Responses API error.
                let has_error = json.get("error").is_some();
                if has_error {
                    let error_msg = json.pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown upstream error")
                        .to_string();
                    let error_code = json.pointer("/error/code")
                        .and_then(|c| c.as_str())
                        .unwrap_or("unknown");
                    warn!("transform_stream: upstream error in SSE: [{}] {}", error_code, error_msg);
                    if !st.is_completed {
                        st.send_error_response(&tx, &format!("Upstream error: {} (code: {})", error_msg, error_code)).await;
                    }
                    continue;
                }

                let choices = match json.get("choices").and_then(|c| c.as_array()) {
                    Some(arr) => arr,
                    None => {
                        warn!("transform_stream: skipping SSE chunk without 'choices' or 'error' field: {:.120}", data);
                        continue;
                    }
                };

                for choice in choices {
                    let delta = match choice.get("delta") {
                        Some(d) => d,
                        None => {
                            warn!("transform_stream: skipping choice without 'delta' field");
                            continue;
                        }
                    };
                    let finish_reason = choice.get("finish_reason").and_then(|f| f.as_str());

                    // ── Send initial events on first chunk ──
                    if !st.has_sent_created {
                        st.has_sent_created = true;
                        sse_send(&tx, &serde_json::json!({
                            "type": "response.created",
                            "response": {
                                "id": st.response_id,
                                "object": "response",
                                "created_at": st.now,
                                "model": st.model_name,
                                "status": "in_progress",
                                "output": []
                            }
                        })).await;
                    }

                    if !st.has_sent_in_progress {
                        st.has_sent_in_progress = true;
                        sse_send(&tx, &serde_json::json!({
                            "type": "response.in_progress",
                            "response": {
                                "id": st.response_id,
                                "object": "response",
                                "created_at": st.now,
                                "model": st.model_name,
                                "status": "in_progress",
                                "output": []
                            }
                        })).await;
                    }

                    // ── Text content (with >think thinking block detection) ──
                    // Some providers (like Agnes) return thinking content inline in the
                    // text with a >think prefix, instead of in a separate
                    // reasoning_content field. We detect this at the start of the text
                    // stream and route it to the reasoning buffer, so Codex CLI can
                    // display it as a collapsible thinking block.
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            // Detect an inline thinking block at the start of the
                            // text stream (markers vary by provider: >think, <think>, ...)
                            if !st.has_sent_output_item && !st.has_sent_reasoning && !st.is_thinking_block {
                                if is_thinking_marker(content) {
                                    st.is_thinking_block = true;
                                }
                            }

                            if st.is_thinking_block {
                                // ── Thinking block: route to reasoning_buffer ──
                                // Accumulate text in reasoning_buffer and check if the
                                // thinking block ends with a double-newline separator.
                                let combined = format!("{}{}", st.reasoning_buffer, content);
                                // End of the thinking block (shared logic with
                                // split_thinking_content — handles both prefix
                                // style `>think` and tag style `<think>` blocks).
                                let delim = find_thinking_delimiter(&combined);
                                if let Some((delim_pos, delim_len)) = delim {
                                    // End of thinking block found — split at the delimiter
                                    let thinking_part = combined[..delim_pos].to_string();
                                    let text_after = combined[delim_pos + delim_len..].trim_start().to_string();

                                    // Emit reasoning output item for the thinking part
                                    if !st.has_sent_reasoning {
                                        st.has_sent_reasoning = true;
                                        st.reasoning_output_index = 0;
                                        st.reasoning_item_id = format!("reason_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
                                        sse_send(&tx, &serde_json::json!({
                                            "type": "response.output_item.added",
                                            "output_index": st.reasoning_output_index,
                                            "item": {
                                                "id": st.reasoning_item_id,
                                                "type": "reasoning",
                                                "reasoning": ""
                                            }
                                        })).await;
                                    }
                                    st.reasoning_buffer = thinking_part;
                                    st.is_thinking_block = false;

                                    // Reasoning is at output_index 0, so text should be at 1
                                    st.output_index = 1;
                                    // Tool calls start after text (index 2)
                                    st.tool_call_output_index = 2;

                                    // Now send the remaining text as normal text
                                    if !text_after.is_empty() {
                                        if !st.has_sent_output_item {
                                            st.has_sent_output_item = true;
                                            st.item_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
                                            sse_send(&tx, &serde_json::json!({
                                                "type": "response.output_item.added",
                                                "output_index": st.output_index,
                                                "item": {
                                                    "id": st.item_id,
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "content": []
                                                }
                                            })).await;
                                        }
                                        if !st.has_sent_content_part {
                                            st.has_sent_content_part = true;
                                            sse_send(&tx, &serde_json::json!({
                                                "type": "response.content_part.added",
                                                "item_id": st.item_id,
                                                "output_index": st.output_index,
                                                "content_index": st.content_index,
                                                "part": {"type": "output_text", "text": ""}
                                            })).await;
                                        }
                                        st.text_buffer.push_str(&text_after);
                                        sse_send(&tx, &serde_json::json!({
                                            "type": "response.output_text.delta",
                                            "delta": &text_after,
                                            "item_id": st.item_id,
                                            "output_index": st.output_index,
                                            "content_index": st.content_index
                                        })).await;
                                    }
                                } else {
                                    // Still in thinking block — emit reasoning events
                                    if !st.has_sent_reasoning {
                                        st.has_sent_reasoning = true;
                                        st.reasoning_output_index = 0;
                                        st.reasoning_item_id = format!("reason_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
                                        sse_send(&tx, &serde_json::json!({
                                            "type": "response.output_item.added",
                                            "output_index": st.reasoning_output_index,
                                            "item": {
                                                "id": st.reasoning_item_id,
                                                "type": "reasoning",
                                                "reasoning": ""
                                            }
                                        })).await;
                                    }
                                    st.reasoning_buffer = combined;
                                }
                            } else {
                                // ── Normal text path (existing behavior) ──
                                if !st.has_sent_output_item {
                                    st.has_sent_output_item = true;
                                    st.item_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
                                    sse_send(&tx, &serde_json::json!({
                                        "type": "response.output_item.added",
                                        "output_index": st.output_index,
                                        "item": {
                                            "id": st.item_id,
                                            "type": "message",
                                            "role": "assistant",
                                            "content": []
                                        }
                                    })).await;
                                }
                                if !st.has_sent_content_part {
                                    st.has_sent_content_part = true;
                                    sse_send(&tx, &serde_json::json!({
                                        "type": "response.content_part.added",
                                        "item_id": st.item_id,
                                        "output_index": st.output_index,
                                        "content_index": st.content_index,
                                        "part": {"type": "output_text", "text": ""}
                                    })).await;
                                }
                                st.text_buffer.push_str(content);
                                sse_send(&tx, &serde_json::json!({
                                    "type": "response.output_text.delta",
                                    "delta": content,
                                    "item_id": st.item_id,
                                    "output_index": st.output_index,
                                    "content_index": st.content_index
                                })).await;
                            }
                        }
                    }

                    // ── Reasoning / thinking content ──
                    // Some providers (DeepSeek, Qwen, etc.) send reasoning_content
                    // BEFORE the actual content. We handle reasoning separately from
                    // the regular text output — it is emitted as a reasoning output
                    // item with type "reasoning", NOT mixed into output_text.
                    // Support multiple field names: reasoning_content, reasoning, thinking, thinking_content
                    // Check at both delta level and choice level (some providers differ)
                    let reasoning = extract_reasoning_content(delta)
                        .or_else(|| extract_reasoning_content(choice));
                    if let Some(reasoning) = reasoning {
                        if !reasoning.is_empty() {
                            // Emit reasoning output item on first reasoning chunk
                            if !st.has_sent_reasoning {
                                st.has_sent_reasoning = true;
                                // Reasoning output_index goes after the text output item
                                // (which has output_index=0) and before tool calls
                                st.reasoning_output_index = if st.has_sent_output_item { 1 } else { 0 };
                                st.reasoning_item_id = format!("reason_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
                                sse_send(&tx, &serde_json::json!({
                                    "type": "response.output_item.added",
                                    "output_index": st.reasoning_output_index,
                                    "item": {
                                        "id": st.reasoning_item_id,
                                        "type": "reasoning",
                                        "reasoning": ""
                                    }
                                })).await;
                            }
                            st.reasoning_buffer.push_str(reasoning);
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
                                sse_send(&tx, &serde_json::json!({
                                    "type": "response.output_item.added",
                                    "output_index": st.tool_call_output_index,
                                    "item": {
                                        "id": tc_id, "type": "function_call",
                                        "call_id": tc_id, "name": tc_name, "arguments": ""
                                    }
                                })).await;
                            }

                            if let Some(args_buf) = st.tool_call_args.get_mut(&tc_index) {
                                if !tc_args.is_empty() {
                                    args_buf.push_str(&tc_args);
                                    let tc_item_id = st.tool_call_ids.get(&tc_index).cloned().unwrap_or_default();
                                    sse_send(&tx, &serde_json::json!({
                                        "type": "response.function_call_arguments.delta",
                                        "delta": tc_args, "item_id": tc_item_id,
                                        "output_index": st.tool_call_output_index
                                    })).await;
                                }
                            }
                        }
                    }

                    // ── Finish reason ──
                    if let Some(reason) = finish_reason {
                        if reason == "stop" || reason == "length" || reason == "tool_calls" || reason == "content_filter" {
                            st.flush_done_events(&tx).await;
                            st.send_completed(&tx).await;
                        }
                    }
                }
            }
        }

        // ── Final safety net: ensure stream completion ──
        if !st.is_completed {
            if st.has_sent_created {
                info!("transform_stream: final safety net — forcing stream completion");
                st.flush_done_events(&tx).await;
                st.send_completed(&tx).await;
            } else {
                // Stream ended without ever sending "response.created".
                // This means no valid chunks were recognized at all.
                error!("transform_stream: stream ended without any valid SSE data (provider may use incompatible format)");
                st.send_error_response(&tx, "Upstream returned no valid streaming data. The provider may use an incompatible SSE format.").await;
            }
        }
    });

    // Convert the mpsc receiver into a futures::stream::Stream
    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
}

/// Internal state for the stream translator
pub struct StreamState {
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
    /// Reasoning/thinking content stored separately from text_buffer
    reasoning_buffer: String,
    reasoning_item_id: String,
    has_sent_reasoning: bool,
    reasoning_output_index: u32,
    /// Whether we are currently inside a >think thinking block (inline thinking
    /// content from providers that don't use the reasoning_content field).
    /// When true, text deltas are routed to reasoning_buffer instead of text_buffer.
    is_thinking_block: bool,
    tool_call_ids: std::collections::HashMap<u32, String>,
    tool_call_names: std::collections::HashMap<u32, String>,
    tool_call_args: std::collections::HashMap<u32, String>,
    tool_call_output_index: u32,
    now: i64,
}

impl StreamState {
    async fn send_error_response(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, reqwest::Error>>,
        error_message: &str,
    ) {
        // Mark as completed first so no subsequent send_completed fires
        self.is_completed = true;

        // If we already sent response.created, don't send another one.
        // Just emit a terminal error event to avoid confusing the client.
        if self.has_sent_created {
            sse_send(tx, &serde_json::json!({
                "type": "response.failed",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "created_at": self.now,
                    "model": self.model_name,
                    "status": "failed",
                    "error": {
                        "code": "proxy_upstream_error",
                        "message": error_message
                    }
                }
            })).await;
            return;
        }
        let response_id = if self.response_id.is_empty() {
            format!("resp_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
        } else {
            self.response_id.clone()
        };
        // No response.created was sent yet — send one with error status
        sse_send(tx, &serde_json::json!({
            "type": "response.created",
            "response": {
                "id": response_id,
                "object": "response",
                "created_at": self.now,
                "model": self.model_name,
                "status": "failed",
                "error": {
                    "code": "proxy_upstream_error",
                    "message": error_message
                },
                "output": []
            }
        })).await;
    }

    async fn flush_done_events(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, reqwest::Error>>,
    ) {
        // Correct event order per Responses API spec:
        // 1. output_text.done → 2. content_part.done → 3. output_item.done
        if self.has_sent_content_part {
            sse_send(tx, &serde_json::json!({
                "type": "response.output_text.done",
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": self.content_index,
                "text": self.text_buffer
            })).await;
            sse_send(tx, &serde_json::json!({
                "type": "response.content_part.done",
                "item_id": self.item_id,
                "output_index": self.output_index,
                "content_index": self.content_index,
                "part": {"type": "output_text", "text": self.text_buffer}
            })).await;
        }
        if self.has_sent_output_item && self.has_sent_content_part {
            sse_send(tx, &serde_json::json!({
                "type": "response.output_item.done",
                "output_index": self.output_index,
                "item": {
                    "id": self.item_id, "type": "message", "role": "assistant",
                    "content": [{"type": "output_text", "text": self.text_buffer}]
                }
            })).await;
        }
        // ── Reasoning output item done ──
        if self.has_sent_reasoning && !self.reasoning_buffer.is_empty() {
            sse_send(tx, &serde_json::json!({
                "type": "response.output_item.done",
                "output_index": self.reasoning_output_index,
                "item": {
                    "id": self.reasoning_item_id,
                    "type": "reasoning",
                    "reasoning": self.reasoning_buffer
                }
            })).await;
        }
        let mut tc_indices: Vec<u32> = self.tool_call_ids.keys().copied().collect();
        tc_indices.sort();
        let mut tc_idx_counter = 0u32;
        for tc_idx in &tc_indices {
            if let Some(tc_id) = self.tool_call_ids.get(tc_idx) {
                let tc_output_idx = if self.has_sent_output_item {
                    if self.has_sent_reasoning { 2 + tc_idx_counter } else { 1 + tc_idx_counter }
                } else {
                    tc_idx_counter
                };
                let tc_name = self.tool_call_names.get(tc_idx).cloned().unwrap_or_default();
                let tc_args = self.tool_call_args.get(tc_idx).cloned().unwrap_or_default();
                sse_send(tx, &serde_json::json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": tc_id, "output_index": tc_output_idx, "arguments": tc_args
                })).await;
                sse_send(tx, &serde_json::json!({
                    "type": "response.output_item.done",
                    "output_index": tc_output_idx,
                    "item": {
                        "id": tc_id, "type": "function_call",
                        "call_id": tc_id, "name": tc_name, "arguments": tc_args
                    }
                })).await;
                tc_idx_counter += 1;
            }
        }
    }

    async fn send_completed(
        &mut self,
        tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, reqwest::Error>>,
    ) {
        if self.is_completed {
            return;
        }
        self.is_completed = true;
        // Defensive: never emit a response.completed with an empty id.
        if self.response_id.is_empty() {
            self.response_id = format!("resp_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        }
        let mut output: Vec<serde_json::Value> = Vec::new();

        // ── Reasoning output ──
        // Reasoning/thinking content comes FIRST in the output array (before
        // the message text), matching the non-streaming response format.
        if self.has_sent_reasoning && !self.reasoning_buffer.is_empty() {
            output.push(serde_json::json!({
                "id": self.reasoning_item_id,
                "type": "reasoning",
                "reasoning": self.reasoning_buffer
            }));
        }

        // ── Text message output ──
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
        sse_send(tx, &serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": self.response_id, "object": "response",
                "created_at": self.now, "model": self.model_name,
                "status": "completed", "output": output
            }
        })).await;
    }
}

/// Extract reasoning/thinking content from a JSON object, checking multiple
/// field names that different providers use: reasoning_content, reasoning,
/// thinking, thinking_content.
fn extract_reasoning_content<'a>(obj: &'a serde_json::Value) -> Option<&'a str> {
    // Reasoning field names vary across providers/models: DeepSeek/Qwen/Kimi
    // use `reasoning_content`; others use `thinking`, `reasoning`,
    // `thinking_content`, `reasoning_text`, `thought`, `thoughts`.
    for key in &[
        "reasoning_content",
        "reasoning",
        "thinking",
        "thinking_content",
        "reasoning_text",
        "thought",
        "thoughts",
    ] {
        if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// Check if text starts with a known inline thinking-block marker, and if so,
/// split the text into (thinking_content, remaining_text).
///
/// Many providers return thinking/reasoning content inline in the text with a
/// marker instead of in a separate `reasoning_content` field. Markers vary by
/// platform/model — Agnes uses `>think`, Qwen3/GLM/Kimi use `<think>` tags,
/// others use `[think]` / `[reasoning]`. This function detects and extracts it.
///
/// Returns `(Some(thinking), None)` when the entire text is thinking,
/// `(Some(thinking), Some(remaining))` when there's actual content after
/// the thinking block, or `(None, Some(original))` when no thinking marker
/// is detected.
fn split_thinking_content(text: &str) -> (Option<String>, Option<String>) {
    if !is_thinking_marker(text) {
        return (None, Some(text.to_string()));
    }

    // Find the delimiter separating thinking from actual content (shared logic
    // with the streaming path — see find_thinking_delimiter).
    let delim = find_thinking_delimiter(text);

    if let Some((pos, len)) = delim {
        let thinking = text[..pos].trim_end().to_string();
        let remaining = text[pos + len..].trim_start().to_string();
        let remaining = if remaining.is_empty() { None } else { Some(remaining) };
        (Some(thinking), remaining)
    } else {
        // No closing delimiter found — entire text is thinking content
        (Some(text.to_string()), None)
    }
}

/// Check whether text starts with a known inline "thinking block" marker.
/// Markers vary across providers/models:
///   - Agnes: `>think ...` (no closing tag)
///   - Qwen3 / GLM / Kimi: `<think>...</think>` / `<thinking>...</thinking>`
///   - Others: `>thinking`, `>reasoning`, `[think]`, `[thinking]`, `[reasoning]`
fn is_thinking_marker(text: &str) -> bool {
    let lower = text.trim_start().to_lowercase();
    const MARKERS: [&str; 9] = [
        ">think",
        ">thinking",
        ">reasoning",
        "<think",
        "<thinking",
        "<reasoning",
        "[think",
        "[thinking",
        "[reasoning",
    ];
    MARKERS.iter().any(|m| lower.starts_with(m))
}

/// Find the delimiter that ends an inline thinking block, returning its
/// position and length. Shared by the streaming and non-streaming paths.
///
/// - Tag-style blocks (`<think>`, `<thinking>`, `<reasoning>`) end at their
///   closing tag. A `\n\n` inside multi-paragraph reasoning is content, NOT a
///   terminator — so it is only used as a fallback when no closing tag exists
///   (malformed / unclosed block).
/// - Prefix-style blocks (`>think`, `[think]`, ...) end at the first `\n\n`.
///
/// Closing tags are checked longest-first so that `</thinking>` wins over its
/// prefix `</think>` at the same position (otherwise the leftover `"ing>"`
/// would leak into the visible answer).
fn find_thinking_delimiter(text: &str) -> Option<(usize, usize)> {
    let lower = text.trim_start().to_lowercase();
    let tag_style = lower.starts_with('<');

    if tag_style {
        let mut best: Option<(usize, usize)> = None;
        for d in ["</thinking>", "</reasoning>", "</think>"] {
            if let Some(pos) = text.find(d) {
                if best.map_or(true, |(bp, _)| pos < bp) {
                    best = Some((pos, d.len()));
                }
            }
        }
        best.or_else(|| text.find("\n\n").map(|pos| (pos, 2)))
    } else {
        text.find("\n\n").map(|pos| (pos, 2))
    }
}

/// Helper: send a JSON SSE event through the channel (async, waits for capacity).
async fn sse_send(
    tx: &tokio::sync::mpsc::Sender<Result<axum::body::Bytes, reqwest::Error>>,
    data: &serde_json::Value,
) {
    let sse = format!(
        "data: {}\n\n",
        serde_json::to_string(data).unwrap_or_default()
    );
    if let Err(e) = tx.send(Ok(axum::body::Bytes::from(sse))).await {
        warn!("sse_send: send failed: {:?}", e);
    }
}
