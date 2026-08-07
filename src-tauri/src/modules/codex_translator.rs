use futures::StreamExt;
use tracing::{info, warn, error};

// ── Responses API ↔ Chat Completions API translation ──
// Codex CLI uses the OpenAI Responses API (/v1/responses), but most
// upstream providers only support Chat Completions (/v1/chat/completions).
// These functions translate between the two formats transparently.

/// Sanitize tool call `arguments` so upstream providers never reject them.
///
/// OpenAI-compatible upstreams require `function.arguments` to be a valid JSON
/// string (the tool-call arguments object). Some platforms/models emit:
///   - empty strings / whitespace-only
///   - truncated JSON (missing closing braces)
///   - prose wrapped around JSON (e.g. "The file is: {"path": "/tmp/x"}")
///   - markdown code fences around JSON
///
/// Any of these cause upstream HTTP 400 "Assistant tool call arguments must be
/// valid JSON", which aborts the whole conversation. This function repairs
/// what it can and falls back to `{}` (empty arguments object) as a safe
/// default so the request still goes through.
fn sanitize_tool_arguments(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "{}".to_string();
    }

    // Already valid JSON → use as-is (the common case).
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return trimmed.to_string();
    }

    // JSON embedded in prose or wrapped in markdown code fences:
    // extract the first balanced {...} object.
    if let Some(extracted) = extract_json_object(trimmed) {
        if serde_json::from_str::<serde_json::Value>(&extracted).is_ok() {
            return extracted;
        }
    }

    // Truncated JSON: attempt brace repair (append missing closing braces).
    if let Some(repaired) = repair_truncated_json(trimmed) {
        return repaired;
    }

    warn!(
        "sanitize_tool_arguments: unrepairable arguments, falling back to {{}} (preview={:.120})",
        raw
    );
    "{}".to_string()
}

/// Extract the first balanced `{...}` JSON object from a string that may
/// contain prose or markdown code fences around it.
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + ch.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Attempt to repair truncated JSON by appending the missing closing braces.
/// E.g. `{"file": "/tmp/x"` → `{"file": "/tmp/x"}`.
fn repair_truncated_json(text: &str) -> Option<String> {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    if depth <= 0 {
        return None;
    }
    let mut candidate = text.to_string();
    for _ in 0..depth {
        candidate.push('}');
    }
    if serde_json::from_str::<serde_json::Value>(&candidate).is_ok() {
        Some(candidate)
    } else {
        None
    }
}

/// Sanitize model output text by stripping control tokens that should never
/// appear as literal text in the client.
///
/// Some models emit EOS (end-of-sequence) tokens or format control markers
/// as literal text, especially when the tokenizer is misconfigured or the
/// model has not been fine-tuned to suppress these tokens. These are
/// machine-only control sequences that are meaningless to the end user and
/// are silently removed from the output stream.
///
/// This function is designed to be extremely conservative for compatibility —
/// it only strips tokens that are unambiguously machine-only control
/// sequences, and never touches legitimate user-facing content:
///
/// 1. Synthetic tokens (pipe-delimited / underscore-marked special sequences
///    such as `<|endoftext|>`, `<|eot_id|>`, `|im_end|`, `<end_of_turn>`) are
///    stripped unconditionally — none of these can appear in legitimate text.
/// 2. `</s>` is BOTH a common EOS token AND a valid HTML/XML closing tag. To
///    avoid breaking legitimate markup (e.g. `<s>opening</s>`), it is only
///    stripped when it appears as a RUN of 2+ consecutive occurrences (the
///    classic EOS-leak signature, e.g. `</s></s>`). A single `</s>` is kept.
///
/// It does NOT attempt to fix semantic issues (hallucination loops,
/// repetition, malformed content).
///
/// Returns the original string unchanged (zero-copy via Cow) when no tokens
/// were removed, avoiding unnecessary allocation on every chunk.
fn sanitize_output_text<'a>(text: &'a str) -> std::borrow::Cow<'a, str> {
    if text.is_empty() {
        return std::borrow::Cow::Borrowed(text);
    }

    // Step 1: strip synthetic control tokens — these are unambiguous machine
    // markers (pipe-delimited or underscore-marked) that never appear in
    // legitimate user-facing text.
    const SYNTHETIC_TOKENS: &[&str] = &[
        "<|endoftext|>",      // GPT-2/GPT-3 EOS token
        "<|eot_id|>",         // Llama 3 EOS token
        "|im_end|",           // ChatML format boundary
        "|im_start|",         // ChatML format boundary
        "<|end|>",            // Some models' EOS (e.g., Yi, DeepSeek)
        "<end_of_turn>",      // Gemma turn separator
        "<|END_OF_TURN_TOKEN|>", // Some models' turn separator
    ];

    let mut result = text.to_string();
    for token in SYNTHETIC_TOKENS {
        if result.contains(token) {
            result = result.replace(token, "");
        }
    }

    // Step 2: strip `</s>` ONLY as runs of 2+ consecutive occurrences. A single
    // `</s>` is preserved because it is a valid HTML/XML closing tag. Scan
    // byte-by-byte (all tokens here are ASCII, so UTF-8 safety is preserved).
    const EOS: &str = "</s>";
    let bytes = result.as_bytes();
    let mut cleaned = String::with_capacity(result.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(EOS.as_bytes()) {
            // Count consecutive `</s>` occurrences starting at i.
            let mut j = i;
            let mut count = 0;
            while j + EOS.len() <= bytes.len() && &bytes[j..j + EOS.len()] == EOS.as_bytes() {
                count += 1;
                j += EOS.len();
            }
            if count >= 2 {
                // Run of 2+ EOS tokens → skip the entire run (leak signature).
                i = j;
            } else {
                // Single `</s>` → keep (likely legitimate HTML).
                cleaned.push_str(&result[i..j]);
                i = j;
            }
        } else {
            // Copy one full UTF-8 character.
            let ch = result[i..].chars().next().unwrap_or_default();
            cleaned.push(ch);
            i += ch.len_utf8();
        }
    }

    if cleaned == text {
        std::borrow::Cow::Borrowed(text)
    } else {
        std::borrow::Cow::Owned(cleaned)
    }
}

/// Translate a Responses API request body to Chat Completions API format.
///
/// Responses API format:  {"model":"...","input":"...","max_output_tokens":...}
/// Chat Completions format: {"model":"...","messages":[...],"max_tokens":...}
pub fn transform_responses_to_chat_completions(body_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut json: serde_json::Value = serde_json::from_slice(body_bytes).ok()?;
    let obj = json.as_object_mut()?;

    // Helper: convert a pending function_call item into a Chat Completions tool_call object
    let fn_call_to_tool_call = |fc: &serde_json::Value| -> serde_json::Value {
        // Responses API function_call items carry BOTH `id` (the item id, e.g.
        // "fc_...") and `call_id` (the identifier the matching
        // function_call_output references). Chat Completions requires the
        // assistant tool_calls[].id to EXACTLY equal the tool message's
        // tool_call_id; strict upstreams (Mistral) reject a mismatch with
        // HTTP 400 code 3230 "Unexpected tool call id ... in tool results".
        // Prefer `call_id` (the value tool results reference), falling back
        // to `id` for clients that only send one.
        let id = fc.get("call_id")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| fc.get("id").and_then(|i| i.as_str()))
            .unwrap_or("")
            .to_string();
        let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        // Sanitize arguments so the upstream never rejects the request with
        // 400 "arguments must be valid JSON". Codex echoes back the arguments
        // we emitted, so an unvalidated value would break the NEXT request too.
        let arguments = sanitize_tool_arguments(
            fc.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}")
        );
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
                Some("output_image") => {
                    // output_image → image_url (Chat Completions part type)
                    obj["type"] = serde_json::Value::String("image_url".to_string());
                    if let Some(image_url) = obj.get("image_url") {
                        if image_url.is_string() {
                            let url = image_url.as_str().unwrap_or("").to_string();
                            obj["image_url"] = serde_json::json!({"url": url});
                        }
                    }
                }
                Some("output_file") | Some("output_audio") => {
                    // Strip "output_" prefix: output_file → file, output_audio → audio
                    if let Some(rest) = part_type.as_deref().and_then(|t| t.strip_prefix("output_")) {
                        obj["type"] = serde_json::Value::String(rest.to_string());
                    }
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

    // Helper: sanitize a history message from Responses API format to Chat
    // Completions format before forwarding upstream.
    //
    // Responses API history messages carry fields that strict Chat Completions
    // upstreams (e.g. Mistral's Pydantic schema) reject with HTTP 422
    // extra_forbidden:
    //   - `id` (msg_...), `status` ("completed"), `metadata` (client telemetry)
    //   - assistant replies under `output` instead of `content`
    // Strip / fold these so the upstream accepts the request.
    let sanitize_message_for_chat = |msg: &mut serde_json::Value| {
        if let Some(obj) = msg.as_object_mut() {
            // Assistant messages in Responses API history carry their reply
            // under `output` (array of output_text / function_call / reasoning
            // items) instead of `content`. Fold it into Chat Completions fields.
            // Only fold when the message has no `content` yet — if Codex sends
            // both, keep the original content (never overwrite it).
            if let Some(output) = obj.remove("output") {
                if !obj.contains_key("content") {
                    if let Some(items) = output.as_array() {
                        let mut content_parts: Vec<serde_json::Value> = Vec::new();
                        let mut tool_calls: Vec<serde_json::Value> = Vec::new();
                        for item in items {
                            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match item_type {
                                "function_call" => tool_calls.push(fn_call_to_tool_call(item)),
                                "message" => {
                                    // Nested assistant message item → take its
                                    // content parts (flatten, don't nest arrays).
                                    if let Some(content) = item.get("content") {
                                        if let Some(arr) = content.as_array() {
                                            content_parts.extend(arr.iter().cloned());
                                        } else {
                                            content_parts.push(content.clone());
                                        }
                                    }
                                }
                                "reasoning" | "function_call_output" | "computer_call"
                                | "file_search_call" | "web_search_call"
                                | "refusal" | "output_refusal" => {
                                    // Non-text output items are Responses API-only;
                                    // dropping them (rather than pushing them as
                                    // content parts) prevents strict upstreams from
                                    // rejecting unknown content part types.
                                }
                                _ => content_parts.push(item.clone()), // output_text & others
                            }
                        }
                        // Always produce an array for multi-part content, and wrap a
                        // single non-string part in an array so convert_message_content
                        // below can convert part types (output_text → text). A single
                        // plain string stays a string.
                        if content_parts.len() == 1 {
                            if content_parts[0].is_string() {
                                obj.insert("content".to_string(), content_parts.remove(0));
                            } else {
                                obj.insert("content".to_string(), serde_json::Value::Array(content_parts));
                            }
                        } else if !content_parts.is_empty() {
                            obj.insert("content".to_string(), serde_json::Value::Array(content_parts));
                        }
                        if !tool_calls.is_empty() {
                            obj.insert("tool_calls".to_string(), serde_json::Value::Array(tool_calls));
                        }
                        // An assistant message whose output contained only tool calls
                        // (or only dropped reasoning items) must still carry an
                        // explicit `content: null` — matching flush_fn_calls below —
                        // so strict upstreams don't reject a content-less message.
                        if !obj.contains_key("content") {
                            obj.insert("content".to_string(), serde_json::Value::Null);
                        }
                    } else if output.is_string() {
                        obj.insert("content".to_string(), output);
                    }
                }
            }
            // Responses API-only fields that strict upstreams reject.
            obj.remove("id");
            obj.remove("status");
            obj.remove("metadata");
            obj.remove("type");
            obj.remove("input");
            obj.remove("from_messages");
            obj.remove("call_id");
            obj.remove("reasoning");
            obj.remove("annotations");
            obj.remove("sender");
        }
        convert_message_content(msg);
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
                                let mut msg = serde_json::Value::Object(m);
                                sanitize_message_for_chat(&mut msg);
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
                sanitize_message_for_chat(&mut msg);
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
                            // Move name, description, parameters, strict into a nested "function" object
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
                            // `strict` belongs INSIDE the function object in
                            // OpenAI-compatible Chat Completions format. Left at
                            // the tool level it makes strict upstreams (Mistral's
                            // Pydantic union of WebSearchTool/CodeInterpreterTool/
                            // Tool) reject the request with HTTP 422
                            // extra_forbidden.
                            if let Some(strict) = tool_obj.remove("strict") {
                                fn_obj.insert("strict".to_string(), strict);
                            }
                            if !fn_obj.is_empty() {
                                tool_obj.insert("function".to_string(), serde_json::Value::Object(fn_obj));
                            }
                        } else if has_function_nesting {
                            // Already nested (Chat Completions style) — sweep
                            // any stray top-level function fields into the nested
                            // object (covers `strict`, and the rare mixed format
                            // that carries name/description/parameters at BOTH
                            // levels) so they don't 422 upstream.
                            let stray_strict = tool_obj.remove("strict");
                            let stray_name = tool_obj.remove("name");
                            let stray_desc = tool_obj.remove("description");
                            let stray_params = tool_obj.remove("parameters");
                            if let Some(fn_obj) = tool_obj.get_mut("function").and_then(|f| f.as_object_mut()) {
                                if let Some(strict) = stray_strict {
                                    fn_obj.insert("strict".to_string(), strict);
                                }
                                if let Some(name) = stray_name {
                                    fn_obj.insert("name".to_string(), name);
                                }
                                if let Some(desc) = stray_desc {
                                    fn_obj.insert("description".to_string(), desc);
                                }
                                if let Some(params) = stray_params {
                                    fn_obj.insert("parameters".to_string(), params);
                                }
                            }
                        }
                        // Whitelist the tool-level keys. The Responses API
                        // function tool schema also accepts `metadata` (and Codex
                        // may send other per-tool fields in future versions);
                        // strict upstreams reject ANY unknown tool-level key with
                        // 422 extra_forbidden, so drop everything except `type`
                        // and `function` rather than whack-a-mole each field.
                        let stray_keys: Vec<String> = tool_obj
                            .keys()
                            .filter(|k| k.as_str() != "type" && k.as_str() != "function")
                            .cloned()
                            .collect();
                        for key in stray_keys {
                            info!("Removing Responses API-only tool field '{}'", key);
                            tool_obj.remove(&key);
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
    //
    // CRITICAL: unsupported tool types (namespace, web_search, computer_use,
    // etc.) were removed from `tools` above. Any `tool_choice` that references
    // a removed tool — or any `tool_choice` left behind when `tools` became
    // empty — makes strict upstreams reject the whole request with HTTP 400
    // ("'tool_choice' is only allowed when 'tools' are specified"), which
    // aborts the conversation. Reconcile tool_choice against the surviving
    // tools so the request still goes through.
    let surviving_tool_names: std::collections::HashSet<String> = obj
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tool| {
                    tool.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    // Decide whether to drop tool_choice (borrow-check friendly: decide first,
    // mutate after).
    let drop_tool_choice: bool = if !obj.contains_key("tools") {
        // `tools` was removed entirely (all types were unsupported) — a
        // tool_choice is now invalid and strict upstreams reject it.
        warn!("transform: removing tool_choice (request has no tools after translation)");
        obj.get("tool_choice").is_some()
    } else {
        match obj.get("tool_choice") {
            None => false,
            Some(v) => {
                let choice_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("function");
                if choice_type != "function" {
                    // tool_choice references a removed non-function tool type
                    // (e.g. web_search_preview) — drop it.
                    warn!("transform: removing tool_choice of unsupported type '{}'", choice_type);
                    true
                } else {
                    // If tool_choice names a specific function, ensure it still
                    // exists among the surviving tools (Borrow<str> avoids a
                    // String allocation per request).
                    match v.get("name").and_then(|n| n.as_str()) {
                        Some(name) if !surviving_tool_names.contains(name) => {
                            warn!("transform: tool_choice references removed tool '{}', dropping it", name);
                            true
                        }
                        _ => false,
                    }
                }
            }
        }
    };
    if drop_tool_choice {
        obj.remove("tool_choice");
    }

    // Convert surviving tool_choice from Responses API format to Chat Completions format.
    if let Some(tool_choice) = obj.get_mut("tool_choice") {
        if let Some(tc) = tool_choice.as_object_mut() {
            if tc.contains_key("name") && !tc.contains_key("function") {
                if let Some(name) = tc.remove("name") {
                    tc.insert("function".to_string(), serde_json::json!({ "name": name }));
                }
            }
            // Whitelist tool_choice object keys the same way as tools: strict
            // upstreams (Mistral) reject unknown keys (e.g. `strict`) in the
            // ToolChoice union member with HTTP 422 extra_forbidden. Only
            // `type` and `function` are valid Chat Completions keys.
            let stray_keys: Vec<String> = tc
                .keys()
                .filter(|k| k.as_str() != "type" && k.as_str() != "function")
                .cloned()
                .collect();
            for key in stray_keys {
                info!("Removing Responses API-only tool_choice field '{}'", key);
                tc.remove(&key);
            }
        }
    }

    // ── Translate Responses API `text.format` (structured output) → `response_format` ──
    // The `text` param is Responses-only; strict upstreams reject it.
    //
    // Shape conversion: Responses API puts the JSON schema at the top level
    // ({"type":"json_schema","name":...,"schema":...}), while Chat
    // Completions nests it under a `json_schema` key
    // ({"type":"json_schema","json_schema":{"name":...,"schema":...}}).
    // Passing the Responses API shape through makes strict upstreams reject the
    // request with HTTP 400 "response_format: missing field json_schema", which
    // aborts the conversation — so wrap the schema fields into the nested
    // `json_schema` object.
    if let Some(text) = obj.remove("text") {
        if let Some(fmt) = text.get("format") {
            if !obj.contains_key("response_format") {
                let mut response_format = fmt.clone();
                let mut keep_response_format = true;
                if let Some(fmt_obj) = response_format.as_object_mut() {
                    let fmt_type = fmt_obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match fmt_type {
                        "json_schema" => {
                            let mut inner = serde_json::Map::new();
                            if let Some(name) = fmt_obj.remove("name") {
                                inner.insert("name".to_string(), name);
                            }
                            if let Some(schema) = fmt_obj.remove("schema") {
                                inner.insert("schema".to_string(), schema);
                            }
                            if let Some(strict) = fmt_obj.remove("strict") {
                                inner.insert("strict".to_string(), strict);
                            }
                            if let Some(desc) = fmt_obj.remove("description") {
                                inner.insert("description".to_string(), desc);
                            }
                            fmt_obj.insert("json_schema".to_string(), serde_json::Value::Object(inner));
                        }
                        "text" => {
                            // `{"type":"text"}` is the default output format;
                            // some strict relays reject an explicit
                            // response_format, so omit it entirely.
                            keep_response_format = false;
                        }
                        _ => {} // "json_object" and anything else pass through
                    }
                }
                if keep_response_format {
                    obj.insert("response_format".to_string(), response_format);
                }
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

    // ── Post-processing: sanitize ALL tool_call arguments ──
    // OpenAI-compatible upstreams reject assistant tool_calls whose
    // `function.arguments` is not valid JSON (HTTP 400 "Assistant tool call
    // arguments must be valid JSON"). Run a unified pass over every message
    // so every input path is covered — including the `from_messages`
    // passthrough and role-bearing messages with embedded tool_calls, in
    // addition to the function_call items converted above.
    if let Some(messages) = obj.get_mut("messages") {
        if let Some(arr) = messages.as_array_mut() {
            for msg in arr.iter_mut() {
                if let Some(tool_calls) = msg.get_mut("tool_calls").and_then(|t| t.as_array_mut()) {
                    for tc in tool_calls.iter_mut() {
                        if let Some(fn_obj) = tc.get_mut("function").and_then(|f| f.as_object_mut()) {
                            if let Some(serde_json::Value::String(s)) = fn_obj.get_mut("arguments") {
                                *s = sanitize_tool_arguments(s);
                            } else {
                                // Missing or non-string arguments → replace with
                                // an empty arguments object so strict upstreams
                                // don't reject the whole request.
                                fn_obj.insert("arguments".to_string(), serde_json::json!("{}"));
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Post-processing: drop orphan tool messages ──
    // Strict upstreams (Mistral) require every `tool` message's
    // `tool_call_id` to match an assistant message's `tool_calls[].id` in the
    // SAME request; a tool result whose assistant tool_call is absent (e.g.
    // history truncated between the call and its result, or a
    // function_call_output that arrived without its function_call) is
    // rejected with HTTP 400 code 3230 "Unexpected tool call id ... in tool
    // results". Collect all assistant tool_call ids first, then drop orphan
    // tool messages so the request still goes through.
    if let Some(messages) = obj.get_mut("messages") {
        if let Some(arr) = messages.as_array_mut() {
            // Collect all assistant tool_call ids first, then drop tool
            // messages whose tool_call_id doesn't match. NOTE: this pass must
            // run even when the set is EMPTY — with zero assistant tool_calls
            // (e.g. history truncated between a call and its result, or a
            // function_call_output that arrived without its function_call)
            // ANY tool message is by definition orphan and must be dropped,
            // or the strict upstream (Mistral 3230) rejects the whole
            // conversation.
            let tool_call_ids: std::collections::HashSet<String> = arr.iter()
                .filter_map(|m| m.get("tool_calls").and_then(|t| t.as_array()))
                .flatten()
                .filter_map(|tc| tc.get("id").and_then(|i| i.as_str()))
                .map(|s| s.to_string())
                .collect();
            arr.retain(|m| {
                let is_tool = m.get("role").and_then(|r| r.as_str()) == Some("tool");
                // A tool message whose tool_call_id is missing or doesn't
                // match any assistant tool_call is orphan (truncated history
                // or an unpaired function_call_output). Drop it.
                let is_orphan = is_tool
                    && !m.get("tool_call_id")
                        .and_then(|c| c.as_str())
                        .map_or(false, |cid| tool_call_ids.contains(cid));
                if is_orphan {
                    warn!("transform: dropping orphan tool message (tool_call_id not found in assistant tool_calls)");
                }
                !is_orphan
            });
        }
    }

    // Remove Responses API-specific fields that don't exist in Chat Completions.
    // Strict third-party upstreams reject unknown fields with HTTP 400/422, so
    // drop every Responses-only param instead of passing it through.
    obj.remove("previous_response_id");
    obj.remove("store");
    obj.remove("include");
    obj.remove("truncation");
    // Codex sends cache/telemetry metadata on every request; strict upstreams
    // (e.g. Mistral's Pydantic schema) reject unknown top-level fields with
    // HTTP 422 extra_forbidden, so strip them like the other Responses-only
    // params.
    obj.remove("prompt_cache_key");
    obj.remove("client_metadata");
    obj.remove("x-codex-turn-metadata");
    obj.remove("turn_id");

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

/// Strip `reasoning_effort` for models that do not support the parameter.
///
/// Codex CLI sends Responses API `reasoning.effort` (low/medium/high), which
/// the request translation above maps to Chat Completions `reasoning_effort`.
/// Most OpenAI-compatible providers accept this parameter, but some do NOT
/// support it at all — sending ANY value (including the schema-valid
/// `none`/`high`) makes the upstream reject the whole request with HTTP 400,
/// aborting the conversation mid-turn:
///   - Mistral model families (codestral, mistral-small, open-mistral-nemo,
///     pixtral, etc.): "reasoning_effort is not enabled for this model"
///     (code 3051) — the model-level capability check rejects the field.
///   - Google Gemini OpenAI-compat layer (generativelanguage.googleapis.com/
///     v1beta/openai): does not implement the `reasoning_effort` parameter.
///
/// The correct fix is to REMOVE the field entirely for these models so the
/// request goes through (the model uses its default reasoning behavior).
/// Other models (and bodies without `reasoning_effort`) are left unchanged
/// and None is returned.
pub fn sanitize_reasoning_effort_for_model(body_bytes: &[u8], model_name: &str) -> Option<Vec<u8>> {
    // Models that reject the reasoning_effort field entirely:
    //   - All Mistral family models: codestral, mistral-*, open-mistral-*,
    //     pixtral-* (Mistral error code 3051)
    //   - Google Gemini OpenAI-compat layer
    let lower = model_name.to_lowercase();
    if !lower.contains("mistral")
        && !lower.contains("codestral")
        && !lower.contains("pixtral")
        && !lower.contains("gemini")
    {
        return None;
    }
    let mut json: serde_json::Value = serde_json::from_slice(body_bytes).ok()?;
    let obj = json.as_object_mut()?;
    if obj.remove("reasoning_effort").is_none() {
        return None;
    }
    info!(
        "sanitize_reasoning_effort: model '{}': removed reasoning_effort (model does not support the parameter)",
        model_name
    );
    Some(serde_json::to_vec(&json).ok()?)
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
                            let text_to_emit = sanitize_output_text(actual_text.as_deref().unwrap_or(content_str));
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
                            // Sanitize arguments: some providers emit empty,
                            // truncated or prose-wrapped arguments. Codex stores
                            // this value and echoes it back in the next request,
                            // so it must be valid JSON or the upstream 400s.
                            let arguments = sanitize_tool_arguments(
                                tc.get("function")
                                    .and_then(|f| f.get("arguments"))
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("{}")
                            );
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
/// Must be generous enough to cover slow first tokens from reasoning models
/// and relays that buffer the whole response before streaming it (Agnes
/// buffers; a long reasoning + web_search turn can exceed 30s before the
/// first chunk). 120s still catches genuinely dead streams, while the reqwest
/// client's 3600s total timeout remains the last-resort backstop.
const STREAM_FIRST_CHUNK_TIMEOUT_SECS: u64 = 120;

/// Maximum idle time (seconds) allowed between consecutive SSE chunks.
/// Reasoning models (DeepSeek, Agnes, etc.) can pause for a long time between
/// chunks while "thinking" (or when the relay buffers output), so this must be
/// generous — an aggressive idle timeout truncates the stream mid-conversation
/// and makes Codex end the turn with no output. The reqwest client's 3600s
/// total timeout still caps the overall stream length as a backstop.
const STREAM_CHUNK_IDLE_TIMEOUT_SECS: u64 = 300;

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
        // Wall-clock timer for termination diagnostics: every exit path below
        // logs how long the stream ran and why it ended, so a "conversation got
        // interrupted" report can be traced to a specific cause (upstream error,
        // idle timeout, empty stream, client disconnect, natural end).
        let started = std::time::Instant::now();

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

        // Buffer for reassembling SSE payloads that some relays split across
        // multiple physical lines (see try_parse_sse_fragments). Declared
        // outside the chunk loop so a fragment cut off at a chunk boundary can
        // be completed by the first line of the next chunk.
        let mut pending_sse_fragments: Vec<String> = Vec::new();

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
                    warn!("transform_stream_to_responses: upstream stream error: {} (after {:?})", e, started.elapsed());
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
                        info!("transform_stream: upstream stream ended naturally (after {:?}), flushing remaining events", started.elapsed());
                        st.flush_done_events(&tx).await;
                        st.send_completed(&tx).await;
                    } else if !st.is_completed {
                        // Stream ended before any valid chunk — this can happen
                        // if the upstream returns an empty body (e.g., 204 No Content
                        // disguised as a 200), a non-SSE response, or a body whose
                        // format we couldn't parse. Surface this as a FAILED
                        // response instead of a silent empty "completed" event
                        // (which previously made Codex end the turn with no output).
                        warn!("transform_stream: upstream stream ended without any valid data (after {:?})", started.elapsed());
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
                        warn!("transform_stream: timeout waiting for first valid chunk ({}s, stream ran {:?}). Upstream may be using non-standard SSE format.", STREAM_FIRST_CHUNK_TIMEOUT_SECS, started.elapsed());
                        // Send a minimal error response so Codex CLI doesn't hang
                        st.send_error_response(&tx, &format!(
                            "Upstream did not return a valid streaming response within {} seconds. The provider may use an unsupported SSE format.", STREAM_FIRST_CHUNK_TIMEOUT_SECS
                        )).await;
                    } else {
                        warn!("transform_stream: timeout waiting for next chunk ({}s, stream ran {:?}), forcing completion", STREAM_CHUNK_IDLE_TIMEOUT_SECS, started.elapsed());
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
                    // An SSE blank line terminates the current event. If a
                    // multi-line payload never completed, discard the fragment.
                    if !pending_sse_fragments.is_empty() {
                        warn!("transform_stream: discarding incomplete SSE fragment at event boundary");
                        pending_sse_fragments.clear();
                    }
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
                    // A complete payload may be followed by [DONE]; drop any
                    // leftover unparsed fragment.
                    pending_sse_fragments.clear();
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

                // Try to parse this line as a complete JSON document. Some
                // relays (Agnes et al.) wrap a single payload across multiple
                // physical lines — a truncated `data:` line followed by bare
                // continuation lines, or a pretty-printed multi-line error
                // body. Accumulate fragments and parse once the combined text
                // forms valid JSON, so no chunk is silently dropped (dropped
                // fragments previously truncated tool-call arguments and made
                // the whole stream look empty, aborting the conversation).
                let json = match parse_sse_line(&mut pending_sse_fragments, data) {
                    Some(j) => j,
                    None => {
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
                    // ── Output sanitization ──
                    // Strip leaked control tokens (EOS markers like </s>, ChatML
                    // boundaries like |im_end|, etc.) from the model's text stream
                    // before it reaches the client. See sanitize_output_text().
                    if let Some(raw_content) = delta.get("content").and_then(|c| c.as_str()) {
                        if !raw_content.is_empty() {
                            let content = sanitize_output_text(raw_content);
                            if !content.is_empty() {
                            // Detect an inline thinking block at the start of the
                            // text stream (markers vary by provider: >think, <think>, ...)
                            if !st.has_sent_output_item && !st.has_sent_reasoning && !st.is_thinking_block {
                                if is_thinking_marker(content.as_ref()) {
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
                                st.text_buffer.push_str(content.as_ref());
                                sse_send(&tx, &serde_json::json!({
                                    "type": "response.output_text.delta",
                                    "delta": content,
                                    "item_id": st.item_id,
                                    "output_index": st.output_index,
                                    "content_index": st.content_index
                                })).await;
                            }
                            // Close the inner if !content.is_empty() (sanitization guard)
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
                info!("transform_stream: final safety net — forcing stream completion (after {:?})", started.elapsed());
                st.flush_done_events(&tx).await;
                st.send_completed(&tx).await;
            } else {
                // Stream ended without ever sending "response.created".
                // This means no valid chunks were recognized at all.
                error!("transform_stream: stream ended without any valid SSE data after {:?} (provider may use incompatible format)", started.elapsed());
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
                // Sanitize the accumulated arguments before emitting them: the
                // upstream model may stream empty/truncated/prose-wrapped JSON
                // fragments. Codex stores this value and echoes it back in the
                // next request, so it must be valid JSON or the upstream 400s.
                let tc_args = sanitize_tool_arguments(
                    &self.tool_call_args.get(tc_idx).cloned().unwrap_or_default()
                );
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
                // Sanitize before including in the final response payload —
                // must be valid JSON for Codex to echo back on the next turn.
                let tc_args = sanitize_tool_arguments(
                    &self.tool_call_args.get(tc_idx).cloned().unwrap_or_default()
                );
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

/// Maximum accumulated size of unparseable SSE fragments before discarding.
/// Prevents unbounded memory growth when an upstream emits garbage that never
/// forms valid JSON.
const SSE_FRAGMENT_BUFFER_LIMIT: usize = 1024 * 1024;

/// Parse one SSE line into a JSON document, transparently reassembling
/// multi-line fragments left by relays that wrap a single payload across
/// multiple physical lines (Agnes et al.).
///
/// - A line that parses standalone with an empty buffer → used as-is.
/// - A line that parses standalone while the buffer holds a stale fragment →
///   the stale fragment is discarded (it never completed) and the standalone
///   document is used.
/// - A line that does not parse standalone → appended to the buffer and the
///   combined text is retried; the buffer is cleared once it parses.
///
/// The buffer is cleared on every success. On failure the buffer is retained
/// for the next line, up to `SSE_FRAGMENT_BUFFER_LIMIT`.
fn parse_sse_line(pending: &mut Vec<String>, data: &str) -> Option<serde_json::Value> {
    let standalone = serde_json::from_str::<serde_json::Value>(data).ok();
    match standalone {
        Some(j) if pending.is_empty() => Some(j),
        Some(j) => {
            // The pending buffer holds an earlier fragment. If appending this
            // line completes it, use the combined document; otherwise the
            // buffer was garbage — drop it and use this complete line alone.
            pending.push(data.to_string());
            match try_parse_sse_fragments(pending) {
                Some(combined) => {
                    pending.clear();
                    Some(combined)
                }
                None => {
                    pending.clear();
                    Some(j)
                }
            }
        }
        None => {
            // Not valid JSON on its own — accumulate and retry once the
            // combined text parses.
            pending.push(data.to_string());
            match try_parse_sse_fragments(pending) {
                Some(combined) => {
                    pending.clear();
                    Some(combined)
                }
                None => {
                    // Still incomplete — guard against unbounded growth.
                    let total: usize = pending.iter().map(|s| s.len()).sum();
                    if total > SSE_FRAGMENT_BUFFER_LIMIT {
                        warn!(
                            "transform_stream: SSE fragment buffer exceeded {} bytes, discarding",
                            SSE_FRAGMENT_BUFFER_LIMIT
                        );
                        pending.clear();
                    }
                    None
                }
            }
        }
    }
}

/// Try to parse a JSON document from accumulated SSE line fragments.
///
/// Some relays (e.g. Agnes) wrap a single JSON payload across multiple
/// physical lines — a truncated `data:` line followed by bare continuation
/// lines, or a pretty-printed multi-line error body. Dropping the fragments
/// truncates tool-call arguments and makes the whole stream look empty,
/// which aborts the conversation. Returns the parsed value when the combined
/// fragments form valid JSON.
fn try_parse_sse_fragments(fragments: &[String]) -> Option<serde_json::Value> {
    // 1. Direct concatenation — relays that wrap mid-token (e.g.
    //    `...reasoning_cont` + `ent":" should"}}]}`) need no separator.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&fragments.concat()) {
        return Some(v);
    }
    // 2. Join with a newline — pretty-printed multi-line error bodies.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&fragments.join("\n")) {
        return Some(v);
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
    const MARKERS: [&str; 10] = [
        ">think",
        ">thinking",
        ">reasoning",
        "<think",
        "<thinking",
        "<reasoning",
        "<odk",
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
        for d in ["</thinking>", "</reasoning>", "</odk>", " response"] {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_history_messages_strips_responses_api_fields() {
        // Regression test for Mistral HTTP 422 extra_forbidden: Codex history
        // messages carry `id`/`status`/`metadata`, and assistant messages carry
        // their reply under `output` instead of `content`. All of these must be
        // stripped / folded before forwarding to strict chat-completions
        // upstreams (Mistral rejects them with 422 extra_forbidden).
        let body = br#"{
            "model": "codestral-latest",
            "input": [
                {"type": "message", "role": "system", "id": "msg_sys_1", "status": "completed",
                 "content": [{"type": "input_text", "text": "You are helpful."}]},
                {"type": "message", "role": "user", "id": "msg_user_1", "status": "completed",
                 "content": [{"type": "input_text", "text": "hi"}],
                 "metadata": {"thread_id": "t1"}},
                {"type": "message", "role": "assistant", "id": "msg_asst_1", "status": "completed",
                 "output": [
                    {"type": "output_text", "text": "Let me look."},
                    {"type": "function_call", "id": "call_1", "name": "shell",
                     "arguments": "{\"command\": \"ls\"}"}
                 ]}
            ],
            "max_output_tokens": 16
        }"#;

        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let json: serde_json::Value = serde_json::from_slice(&out).expect("output should be valid JSON");

        let messages = json.get("messages").and_then(|m| m.as_array()).expect("messages array");
        assert_eq!(messages.len(), 3);

        // Helper: extract text from a content array of converted parts
        let content_text = |msg: &serde_json::Value| -> String {
            msg.get("content")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")  .to_string()
        };

        // System message: id/status stripped, content preserved (input_text→text)
        let sys = &messages[0];
        assert!(sys.get("id").is_none(), "system message must not carry id");
        assert!(sys.get("status").is_none(), "system message must not carry status");
        assert_eq!(content_text(sys), "You are helpful.");
        assert_eq!(sys["content"][0]["type"], "text");

        // User message: id/status/metadata stripped, content preserved
        let user = &messages[1];
        assert!(user.get("id").is_none());
        assert!(user.get("status").is_none());
        assert!(user.get("metadata").is_none(), "user message must not carry metadata");
        assert_eq!(content_text(user), "hi");

        // Assistant message: output folded into content + tool_calls, no id/status/output
        let asst = &messages[2];
        assert!(asst.get("id").is_none());
        assert!(asst.get("status").is_none());
        assert!(asst.get("output").is_none(), "assistant message must not carry output");
        assert_eq!(content_text(asst), "Let me look.");
        assert_eq!(asst["content"][0]["type"], "text", "output_text part must be converted to text");
        let tool_calls = asst.get("tool_calls").and_then(|t| t.as_array()).expect("tool_calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["function"]["name"], "shell");
        assert_eq!(tool_calls[0]["function"]["arguments"], "{\"command\": \"ls\"}");
    }

    #[test]
    fn sanitize_passes_through_valid_json() {
        assert_eq!(sanitize_tool_arguments("{\"path\": \"/tmp/x\"}"), "{\"path\": \"/tmp/x\"}");
        assert_eq!(sanitize_tool_arguments("  {\"a\": 1}  "), "{\"a\": 1}");
    }

    #[test]
    fn sanitize_handles_empty_and_whitespace() {
        assert_eq!(sanitize_tool_arguments(""), "{}");
        assert_eq!(sanitize_tool_arguments("   "), "{}");
        assert_eq!(sanitize_tool_arguments("\n\t"), "{}");
    }

    #[test]
    fn sanitize_extracts_json_from_prose() {
        assert_eq!(
            sanitize_tool_arguments("The file is at {\"path\": \"/tmp/x\"} please check"),
            "{\"path\": \"/tmp/x\"}"
        );
    }

    #[test]
    fn sanitize_extracts_json_from_code_fence() {
        assert_eq!(
            sanitize_tool_arguments("```json\n{\"path\": \"/tmp/x\"}\n```"),
            "{\"path\": \"/tmp/x\"}"
        );
    }

    #[test]
    fn sanitize_repairs_truncated_json() {
        assert_eq!(
            sanitize_tool_arguments("{\"path\": \"/tmp/x\""),
            "{\"path\": \"/tmp/x\"}"
        );
    }

    #[test]
    fn sanitize_falls_back_to_empty_object() {
        // Unrepairable garbage → empty object so the upstream accepts it
        assert_eq!(sanitize_tool_arguments("not json at all"), "{}");
        assert_eq!(sanitize_tool_arguments("hello world"), "{}");
    }

    #[test]
    fn extract_json_object_handles_braces_in_strings() {
        // The closing brace inside the string must NOT end the object
        let s = "{\"a\": \"}\", \"b\": 2}";
        assert_eq!(extract_json_object(s).as_deref(), Some(s));
    }

    #[test]
    fn repair_truncated_json_handles_nested_braces() {
        assert_eq!(
            repair_truncated_json("{\"a\": {\"b\": 1").as_deref(),
            Some("{\"a\": {\"b\": 1}}")
        );
    }

    #[test]
    fn sse_fragment_reassembles_mid_token_wrap() {
        // A relay splits `{"a": 1, "b": 2}` mid-token: `{"a": 1, "b` + `": 2}`
        let frags = vec!["{\"a\": 1, \"b\"".to_string(), ": 2}".to_string()];
        let v = try_parse_sse_fragments(&frags).expect("fragments should reassemble");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn sse_fragment_reassembles_pretty_printed_error_body() {
        // A pretty-printed multi-line error body without the data: prefix
        let frags = vec![
            "{\"error\": {".to_string(),
            "  \"message\": \"boom\"".to_string(),
            "}}".to_string(),
        ];
        let v = try_parse_sse_fragments(&frags).expect("fragments should reassemble");
        assert_eq!(v.pointer("/error/message").and_then(|m| m.as_str()), Some("boom"));
    }

    #[test]
    fn sse_fragment_single_complete_document() {
        let frags = vec!["{\"ok\": true}".to_string()];
        let v = try_parse_sse_fragments(&frags).expect("single fragment should parse");
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn sse_fragment_garbage_returns_none() {
        let frags = vec!["not json".to_string(), "at all".to_string()];
        assert!(try_parse_sse_fragments(&frags).is_none());
    }

    #[test]
    fn sse_line_uses_complete_doc_when_buffer_empty() {
        let mut pending: Vec<String> = Vec::new();
        let v = parse_sse_line(&mut pending, "{\"ok\": true}").expect("complete doc should parse");
        assert_eq!(v["ok"], true);
        assert!(pending.is_empty(), "buffer must stay empty");
    }

    #[test]
    fn sse_line_discards_stale_fragment_for_complete_doc() {
        // Buffer holds a stale fragment from a previous (malformed) event;
        // a new complete document arrives — the stale fragment must be
        // discarded and the standalone document used.
        let mut pending = vec!["{\"a\": 1,".to_string()];
        let v = parse_sse_line(&mut pending, "{\"b\": 2}").expect("complete doc should parse");
        assert_eq!(v["b"], 2);
        assert!(pending.is_empty(), "stale fragment must be cleared");
    }

    #[test]
    fn sse_line_accumulates_then_reassembles() {
        let mut pending: Vec<String> = Vec::new();
        // First line: truncated fragment, still incomplete
        assert!(parse_sse_line(&mut pending, "{\"a\": 1, \"b\"").is_none());
        assert_eq!(pending.len(), 1);
        // Second line: continuation completes the document
        let v = parse_sse_line(&mut pending, ": 2}").expect("combined doc should parse");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], 2);
        assert!(pending.is_empty());
    }

    #[test]
    fn tool_choice_dropped_when_no_tools_remain() {
        // All tool types are unsupported (namespace/web_search) → `tools` is
        // removed entirely. A leftover tool_choice would make the upstream 400
        // with "tool_choice is only allowed when 'tools' are specified".
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {"type": "namespace", "name": "my_namespace"},
                {"type": "web_search"}
            ],
            "tool_choice": {"type": "function", "name": "my_namespace"}
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert!(v.get("tools").is_none(), "tools must be removed");
        assert!(
            v.get("tool_choice").is_none(),
            "tool_choice must be dropped when no tools remain"
        );
    }

    #[test]
    fn function_tool_strict_moved_into_function_object() {
        // Codex CLI sends Responses API tools with a top-level `strict` field:
        //   {"type": "function", "name": "...", "description": "...", "parameters": {...}, "strict": false}
        // The Chat Completions translation must move `strict` INSIDE the nested
        // `function` object (alongside name/description/parameters). Left at the
        // tool level, strict upstreams (Mistral's Pydantic union of
        // WebSearchTool/CodeInterpreterTool/Tool) reject it with HTTP 422
        // extra_forbidden.
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {
                    "type": "function",
                    "name": "shell_command",
                    "description": "Runs a command",
                    "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}},
                    "strict": false
                }
            ]
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let tools = v.get("tools").and_then(|t| t.as_array()).expect("tools must remain");
        let tool = &tools[0];
        assert_eq!(tool["type"], "function");
        assert!(
            tool.get("strict").is_none(),
            "top-level strict must be removed from the tool object"
        );
        let fn_obj = tool.get("function").expect("function must be nested");
        assert_eq!(fn_obj["name"], "shell_command");
        assert_eq!(fn_obj["description"], "Runs a command");
        assert_eq!(fn_obj["strict"], false, "strict must live inside function");
        assert_eq!(fn_obj["parameters"]["type"], "object");
    }

    #[test]
    fn already_nested_tool_strict_swept_into_function() {
        // A tool that already uses Chat Completions nesting but still carries a
        // stray top-level `strict` must have it swept into `function`.
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {
                    "type": "function",
                    "strict": true,
                    "function": {"name": "get_weather", "parameters": {"type": "object"}}
                }
            ]
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let tools = v.get("tools").and_then(|t| t.as_array()).expect("tools must remain");
        let tool = &tools[0];
        assert!(tool.get("strict").is_none(), "top-level strict must be swept");
        assert_eq!(tool["function"]["strict"], true);
    }

    #[test]
    fn tool_metadata_and_choice_strict_whitelisted() {
        // The Responses API function tool schema also accepts `metadata`;
        // strict upstreams (Mistral) reject ANY unknown tool-level key with
        // 422 extra_forbidden, so metadata must be dropped. tool_choice must
        // likewise have unknown keys (e.g. strict) stripped.
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {
                    "type": "function",
                    "name": "shell_command",
                    "description": "Runs a command",
                    "parameters": {"type": "object"},
                    "strict": false,
                    "metadata": {"some": "value"}
                }
            ],
            "tool_choice": {"type": "function", "name": "shell_command", "strict": true}
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let tools = v.get("tools").and_then(|t| t.as_array()).expect("tools must remain");
        let tool = &tools[0];
        // Order-agnostic whitelist assertions (serde_json Map key order is
        // not guaranteed across the preserve_order feature).
        assert!(tool.get("metadata").is_none(), "tool metadata must be stripped");
        assert!(tool.get("strict").is_none(), "top-level tool strict must be stripped");
        assert!(tool.get("name").is_none(), "top-level tool name must be moved into function");
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["strict"], false);
        let tc = v.get("tool_choice").expect("tool_choice must remain");
        assert!(tc.get("strict").is_none(), "tool_choice strict must be stripped");
        assert_eq!(tc["function"]["name"], "shell_command");
    }

    #[test]
    fn reasoning_effort_stripped_for_mistral_and_gemini() {
        // Regression test for Mistral HTTP 400 "reasoning_effort is not
        // enabled for this model" (code 3051): Mistral family models
        // (codestral, mistral-small, open-mistral-nemo, pixtral, ...) do NOT
        // support the reasoning_effort parameter at all — even schema-valid
        // none/high are rejected (the "low is not supported, must be one of
        // none/high" error was only the stateless enum check). The field must
        // be REMOVED, not remapped. Google Gemini's OpenAI-compat layer does
        // not implement the parameter either.

        // low → removed (codestral)
        let body = br#"{
            "model": "codestral-latest",
            "input": "hi",
            "reasoning": {"effort": "low"}
        }"#;
        let translated = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let out = sanitize_reasoning_effort_for_model(&translated, "codestral-latest").expect("sanitize should apply");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert!(
            v.get("reasoning_effort").is_none(),
            "reasoning_effort must be removed for codestral"
        );

        // medium / high / none → removed too (field unsupported regardless of value)
        for effort in ["medium", "high", "none", "maximum"] {
            let body = format!(
                r#"{{"model": "codestral-latest", "input": "hi", "reasoning": {{"effort": "{}"}}}}"#,
                effort
            );
            let translated = transform_responses_to_chat_completions(body.as_bytes()).expect("transform should succeed");
            let out = sanitize_reasoning_effort_for_model(&translated, "codestral-latest").expect("sanitize should apply");
            let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
            assert!(
                v.get("reasoning_effort").is_none(),
                "reasoning_effort '{}' must be removed for codestral",
                effort
            );
        }

        // Other Mistral family models (mistral-small, open-mistral-nemo,
        // pixtral) reject the field too — must be stripped as well
        for model in ["mistral-small-latest", "open-mistral-nemo", "pixtral-12b", "mistral-large-latest"] {
            let body = format!(
                r#"{{"model": "{}", "input": "hi", "reasoning": {{"effort": "low"}}}}"#,
                model
            );
            let translated = transform_responses_to_chat_completions(body.as_bytes()).expect("transform should succeed");
            let out = sanitize_reasoning_effort_for_model(&translated, model).expect("sanitize should apply");
            let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
            assert!(
                v.get("reasoning_effort").is_none(),
                "reasoning_effort must be removed for '{}'",
                model
            );
        }

        // Gemini OpenAI-compat layer strips it too
        let body = br#"{
            "model": "gemini-2.0-flash",
            "input": "hi",
            "reasoning": {"effort": "low"}
        }"#;
        let translated = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let out = sanitize_reasoning_effort_for_model(&translated, "gemini-2.0-flash").expect("sanitize should apply");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert!(
            v.get("reasoning_effort").is_none(),
            "reasoning_effort must be removed for gemini"
        );

        // Non-Mistral / non-Gemini models are never touched
        for model in ["gpt-5.6", "claude-sonnet-4-5", "deepseek-v4-flash", "qwen3-coder"] {
            let body = format!(
                r#"{{"model": "{}", "input": "hi", "reasoning": {{"effort": "low"}}}}"#,
                model
            );
            let translated = transform_responses_to_chat_completions(body.as_bytes()).expect("transform should succeed");
            assert!(
                sanitize_reasoning_effort_for_model(&translated, model).is_none(),
                "model '{}' must pass through unchanged",
                model
            );
        }

        // No reasoning_effort in body → no change
        let body = br#"{"model": "codestral-latest", "input": "hi"}"#;
        let translated = transform_responses_to_chat_completions(body).expect("transform should succeed");
        assert!(
            sanitize_reasoning_effort_for_model(&translated, "codestral-latest").is_none(),
            "body without reasoning_effort must be unchanged"
        );
    }

    #[test]
    fn tool_choice_dropped_when_referencing_removed_tool() {
        // tools survives with only function tools, but tool_choice references a
        // tool type that was filtered out — the choice must be dropped.
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {"type": "function", "name": "keep_me", "description": "d", "parameters": {"type": "object"}},
                {"type": "web_search"}
            ],
            "tool_choice": {"type": "function", "name": "web_search"}
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert!(v.get("tools").is_some(), "tools must remain");
        assert!(
            v.get("tool_choice").is_none(),
            "tool_choice referencing a removed tool must be dropped"
        );
    }

    #[test]
    fn tool_choice_kept_and_converted_for_surviving_tool() {
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {"type": "function", "name": "my_tool", "description": "d", "parameters": {"type": "object"}}
            ],
            "tool_choice": {"type": "function", "name": "my_tool"}
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let tc = v.get("tool_choice").expect("tool_choice must be kept");
        assert_eq!(tc["type"], "function");
        assert_eq!(
            tc["function"]["name"],
            "my_tool",
            "must be converted to chat completions format"
        );
    }

    #[test]
    fn tool_call_id_uses_call_id_not_item_id() {
        // Regression test for Mistral HTTP 400 code 3230
        // (invalid_request_message_order "Unexpected tool call id ... in tool
        // results"): Responses API function_call items carry BOTH `id` (item
        // id, e.g. fc_xxx) and `call_id` (the identifier the matching
        // function_call_output references). The translated assistant
        // tool_calls[].id must equal the tool message's tool_call_id, so the
        // assistant side must use `call_id` (falling back to `id`).
        let body = br#"{
            "model": "codestral-latest",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "list files"}]},
                {"type": "function_call", "id": "fc_item_1", "call_id": "call_abc123", "name": "shell_command", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_abc123", "output": "file1\nfile2"}
            ]
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let messages = v["messages"].as_array().expect("messages array");

        let assistant = messages.iter()
            .find(|m| m["role"] == "assistant" && m.get("tool_calls").is_some())
            .expect("assistant tool_calls message");
        assert_eq!(
            assistant["tool_calls"][0]["id"], "call_abc123",
            "assistant tool_call id must use call_id (not item id fc_item_1)"
        );

        let tool_msg = messages.iter()
            .find(|m| m["role"] == "tool")
            .expect("tool message");
        assert_eq!(
            tool_msg["tool_call_id"], "call_abc123",
            "tool message tool_call_id must match assistant tool_call id"
        );
    }

    #[test]
    fn orphan_tool_message_is_dropped() {
        // Regression test for Mistral 3230: a tool result whose call_id has
        // no matching assistant tool_call in the same request (e.g. truncated
        // history) must be dropped so the upstream doesn't reject the whole
        // conversation.
        let body = br#"{
            "model": "codestral-latest",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "continue"}]},
                {"type": "function_call_output", "call_id": "call_orphan_1", "output": "result text"}
            ]
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let messages = v["messages"].as_array().expect("messages array");
        assert!(
            messages.iter().all(|m| m["role"] != "tool"),
            "orphan tool message must be dropped (no matching assistant tool_call)"
        );
    }

    #[test]
    fn tool_choice_dropped_for_non_function_type() {
        // tool_choice referencing a removed non-function tool type (e.g.
        // web_search_preview) must be dropped, not forwarded to the upstream.
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {"type": "function", "name": "keep_me", "description": "d", "parameters": {"type": "object"}}
            ],
            "tool_choice": {"type": "web_search_preview"}
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert!(v.get("tools").is_some(), "tools must remain");
        assert!(v.get("tool_choice").is_none(), "non-function tool_choice must be dropped");
    }

    #[test]
    fn response_format_text_is_omitted() {
        // `{"type":"text"}` is the default output — strict relays may reject
        // an explicit response_format, so it must not be forwarded.
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "text": {"format": {"type": "text"}}
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert!(v.get("response_format").is_none(), "text format must be omitted");
    }

    #[test]
    fn tool_choice_string_passes_through_when_tools_remain() {
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "tools": [
                {"type": "function", "name": "my_tool", "parameters": {"type": "object"}}
            ],
            "tool_choice": "auto"
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert_eq!(v["tool_choice"], "auto");
    }

    #[test]
    fn response_format_json_schema_is_nested() {
        // Responses API text.format shape → Chat Completions response_format with
        // the schema wrapped under `json_schema` (upstream 400s otherwise with
        // "response_format: missing field json_schema").
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "answer",
                    "schema": {"type": "object", "properties": {"ok": {"type": "boolean"}}},
                    "strict": true
                }
            }
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        let rf = v.get("response_format").expect("response_format must be present");
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(
            rf["json_schema"]["name"], "answer",
            "schema fields must be nested under json_schema"
        );
        assert_eq!(rf["json_schema"]["schema"]["type"], "object");
        assert_eq!(rf["json_schema"]["strict"], true);
    }

    #[test]
    fn response_format_json_object_passes_through() {
        let body = br#"{
            "model": "gpt-5",
            "input": "hi",
            "text": {"format": {"type": "json_object"}}
        }"#;
        let out = transform_responses_to_chat_completions(body).expect("transform should succeed");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert_eq!(v["response_format"]["type"], "json_object");
    }

    #[test]
    fn sanitize_output_text_strips_control_tokens() {
        // Synthetic tokens (pipe-delimited) are stripped unconditionally;
        // repeated `</s>` runs (the EOS-leak signature) are stripped.
        let input = "Hello</s></s> world<|endoftext|>! |im_end| <|eot_id|> <|end|> <end_of_turn> done";
        let out = sanitize_output_text(input);
        // The `</s></s>` run and all synthetic tokens are removed; surrounding
        // whitespace is left in place.
        assert_eq!(out, "Hello world!     done");
    }

    #[test]
    fn sanitize_output_text_strips_repeated_eos_runs() {
        // Repeated `</s>` runs (2+ consecutive) are the EOS-leak signature and
        // are stripped entirely — including leading and mid-text runs.
        assert_eq!(sanitize_output_text("</s></s>"), "");
        assert_eq!(sanitize_output_text("</s></s></s>"), "");
        assert_eq!(sanitize_output_text("a</s></s>b"), "ab");
        assert_eq!(sanitize_output_text("</s></s> response"), " response");
    }

    #[test]
    fn sanitize_output_text_returns_borrowed_when_clean() {
        // No tokens to strip → zero-copy (Cow::Borrowed).
        let input = "This is normal text, no control tokens here.";
        let out = sanitize_output_text(input);
        assert_eq!(out, input);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn sanitize_output_text_handles_empty_and_owned() {
        // Empty input → empty borrowed.
        assert_eq!(sanitize_output_text(""), "");
        // Token that IS stripped → owned Cow + empty result.
        let out = sanitize_output_text("<|eot_id|>");
        assert!(matches!(out, std::borrow::Cow::Owned(_)));
        assert_eq!(out, "");
        // Multiple consecutive tokens removed fully.
        assert_eq!(sanitize_output_text("<|eot_id|><|end|>"), "");
    }

    #[test]
    fn sanitize_output_text_does_not_break_legitimate_content() {
        // A single `</s>` is a valid HTML/XML closing tag and is PRESERVED.
        assert_eq!(sanitize_output_text("<s>opening</s>"), "<s>opening</s>");
        // Short/common words that merely CONTAIN token substrings are NOT touched.
        assert_eq!(sanitize_output_text("endoftext is a word"), "endoftext is a word");
        assert_eq!(sanitize_output_text("eos"), "eos");
        // A single `</s>` embedded mid-sentence (legit HTML) is preserved.
        assert_eq!(sanitize_output_text("a</s>b"), "a</s>b");
    }
    #[test]
    fn odk_thought_routed_to_reasoning_channel() {
        // Codex Desktop's ODK reasoning format (<odk>_..._</odk>) must be
        // recognized and routed to the reasoning channel, not shown as text.
        let input = "<odk>_I need to read the pet files first._</odk>\nThen upgrade the pet.";
        assert!(is_thinking_marker(input), "ODK marker must be detected");
        let (reasoning, remaining) = split_thinking_content(input);
        assert!(reasoning.is_some(), "ODK block must yield reasoning");
        let reason = reasoning.unwrap();
        assert!(reason.contains("read the pet files"), "reasoning should contain ODK content");
        let remain = remaining.expect("there should be text after the ODK block");
        assert_eq!(remain, "Then upgrade the pet.");
    }

    #[test]
    fn odm_delimiter_closes_odk_block() {
        // The </odk> closing tag must terminate the thinking block.
        let input = "<odk>_think_</odk>answer";
        let (reasoning, remaining) = split_thinking_content(input);
        assert!(reasoning.is_some());
        assert_eq!(remaining.unwrap(), "answer");
    }

}
