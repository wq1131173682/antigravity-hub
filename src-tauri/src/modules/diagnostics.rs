//! # 流式终止诊断模块（Codex / OpenAI 兼容 API）
//!
//! 本模块用于把一段「模型 API 响应日志」解析成结构化诊断报告，区分两类
//! 模型停止场景：
//!
//! - **A 类（疑似工具调用被截断）**：流式在上游工具调用分片途中断开，或最后一个
//!   `choices[].delta` 仅有 `content` 文本（如「好的，我来…」）而**没有
//!   `tool_calls` 字段**，或 `finish_reason` 出现但工具参数不完整。
//! - **B 类（模型主动结束本轮）**：最后一个工具调用正常执行并返回结果，随后模型
//!   生成分析文本并自然停止（`finish_reason` 为 `stop` / `end_turn`）。
//!
//! 同时支持把「上游原始响应（Chat Completions）」与「代理转发给 Codex 的响应
//! （Responses API）」做配对分析，以定位故障层（第三方 API / 代理 / Codex / MCP）。
//!
//! ## 喂入数据的来源
//! 见仓库文档 / 本模块底部的 `how_to_capture_logs` 说明。简言之：
//! 开启 Codex 的 debug 日志（`CODEX_DEBUG=1` 或 ChatGPT Work 开发者控制台），
//! 把对应一轮的 SSE 文本（或导出的 JSON）保存为文件，传给 `diagnose` 二进制：
//!
//! ```text
//! diagnose upstream.log            # 单段分析
//! diagnose upstream.log proxy.log  # 上下游配对分析，定位代理层
//! cat upstream.log | diagnose      # 从 stdin 读取
//! ```

use serde::Serialize;
use serde_json::Value;

/// 最后一段 delta 的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LastDeltaType {
    /// 最后 delta 仅携带文本 content
    Content,
    /// 最后 delta 携带 tool_calls
    ToolCalls,
    /// 流以 finish_reason 收尾
    FinishReason,
    /// 无有效 delta
    #[default]
    Empty,
    /// 其他 / 无法判定
    Unknown,
}

/// 判定类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// 疑似工具调用被截断（A 类）
    A,
    /// 模型主动结束本轮（B 类）
    B,
    /// 其他 / 需人工判断（例如模型正常发出工具调用等待执行，并非终止）
    Other,
}

/// 疑似故障层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspectedLayer {
    /// 上游第三方 API 未发送 / 截断工具调用
    ThirdPartyApi,
    /// 代理在转发时丢失 / 截断工具调用
    Proxy,
    /// Codex 客户端收到工具调用但未执行
    Codex,
    /// 工具执行报错（MCP / 本地工具侧）
    Mcp,
    /// 无法仅凭日志定位
    Inconclusive,
}

/// 单个工具调用的累积记录。
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallRecord {
    pub index: usize,
    pub call_id: Option<String>,
    pub name: String,
    pub arguments: String,
    /// 是否在同段日志中观察到对应的工具返回结果（function_call_output / tool 消息）
    pub had_result: bool,
}

/// 工具执行链路中的返回结果记录。
#[derive(Debug, Clone, Serialize)]
pub struct ToolResultRecord {
    pub call_id: Option<String>,
    pub name: Option<String>,
    /// 返回内容（截断到前 200 字符以便阅读）
    pub output_preview: String,
    /// 返回是否为错误形态（含 "error" / 异常字样）
    pub is_error: bool,
}

/// 单段日志的诊断报告。
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticReport {
    /// 来源格式（chat_completions / responses_api / json_array / unknown）
    pub source_format: String,
    pub model: Option<String>,
    /// 解析出的 delta / 消息总数
    pub num_events: usize,
    /// 工具调用序列（按出现顺序累积）
    pub tool_calls: Vec<ToolCallRecord>,
    /// 观察到的工具返回结果数
    pub tool_results_observed: usize,
    /// 「已运行命令」类工具调用次数（即 tool_calls 中有 name 的）
    pub tool_invocations: usize,
    pub last_delta_type: LastDeltaType,
    pub finish_reason: Option<String>,
    /// 是否收到 [DONE] 或 finish_reason（流正常结束）
    pub ended_cleanly: bool,
    pub category: Category,
    pub suspected_layer: SuspectedLayer,
    /// 判定理由（人类可读）
    pub reasoning: String,
}

/// 上下游配对诊断报告。
#[derive(Debug, Clone, Serialize)]
pub struct PairReport {
    pub upstream: DiagnosticReport,
    pub proxy_emitted: DiagnosticReport,
    /// 配对后钉死的故障层
    pub pinned_layer: SuspectedLayer,
    pub reasoning: String,
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 从一段文本中提取所有 JSON 对象（兼容 SSE、JSON 数组、单 JSON 对象）。
fn extract_values(text: &str) -> Vec<Value> {
    let trimmed = text.trim();
    let mut values = Vec::new();

    // 1) SSE：按行扫描 `data: ...`
    if text.contains("data:") || text.contains("data:") {
        for line in text.lines() {
            let line = line.trim_end();
            let rest = match line.strip_prefix("data:") {
                Some(r) => r.strip_prefix(' ').unwrap_or(r),
                None => continue,
            };
            if rest.is_empty() || rest == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(rest) {
                values.push(v);
            }
        }
        if !values.is_empty() {
            return values;
        }
    }

    // 2) `[DONE]` 单独成行（无 `data:` 前缀的情况，部分客户端）
    if trimmed == "[DONE]" {
        return values;
    }

    // 3) JSON 数组
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(trimmed) {
        return arr;
    }

    // 4) 单个 JSON 对象
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(trimmed) {
        values.push(v);
        return values;
    }

    // 5) NDJSON（逐行 JSON，Responses API / Codex 日志常见）
    let mut ndjson = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(line) {
            ndjson.push(v);
        }
    }
    if !ndjson.is_empty() {
        return ndjson;
    }

    values
}

/// 判断并解析对象格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Format {
    ChatCompletions,
    ResponsesApi,
    #[default]
    Unknown,
}

fn detect_format(v: &Value) -> Format {
    if v.get("choices").is_some() {
        Format::ChatCompletions
    } else if v.get("type").is_some() || v.get("response").is_some() || v.get("output").is_some() {
        Format::ResponsesApi
    } else {
        Format::Unknown
    }
}

#[derive(Debug, Default)]
struct Accumulator {
    model: Option<String>,
    format: Format,
    last_delta_type: LastDeltaType,
    finish_reason: Option<String>,
    ended_with_done: bool,
    /// Responses API 收到 `response.completed` 事件（视为正常终止）
    responses_completed: bool,
    num_events: usize,
    /// 工具调用按 index 累积
    tool_calls: Vec<ToolCallRecord>,
    tool_results: Vec<ToolResultRecord>,
    /// 文本意图词命中（用于 A 类启发式）
    content_intent_hit: bool,
}

fn as_str_opt(v: &Value) -> Option<&str> {
    v.as_str()
}

fn tool_call_by_index(acc: &mut Accumulator, index: usize) -> &mut ToolCallRecord {
    if acc.tool_calls.len() <= index {
        acc.tool_calls.resize_with(index + 1, || ToolCallRecord {
            index,
            call_id: None,
            name: String::new(),
            arguments: String::new(),
            had_result: false,
        });
    }
    &mut acc.tool_calls[index]
}

const INTENT_WORDS: &[&str] = &["我来", "调用", "工具", "让我", "执行命令", "运行命令", "查一下", "尝试获取"];

fn note_intent(acc: &mut Accumulator, text: &str) {
    if INTENT_WORDS.iter().any(|w| text.contains(w)) {
        acc.content_intent_hit = true;
    }
}

/// 主解析：把一段日志喂入累加器。
fn parse_into(acc: &mut Accumulator, text: &str) {
    let values = extract_values(text);
    for v in &values {
        acc.num_events += 1;
        if acc.model.is_none() {
            acc.model = v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string());
        }
        let fmt = detect_format(v);
        if fmt != Format::Unknown {
            acc.format = fmt;
        }

        match fmt {
            Format::ChatCompletions => parse_chat_completions(acc, v),
            Format::ResponsesApi => parse_responses_api(acc, v),
            Format::Unknown => parse_message_like(acc, v),
        }
    }
}

fn parse_chat_completions(acc: &mut Accumulator, v: &Value) {
    // 工具结果消息（导出含 role:tool）
    if let Some(role) = v.get("role").and_then(|r| r.as_str()) {
        if role == "tool" || role == "function" {
            let call_id = v.get("tool_call_id").and_then(|c| c.as_str()).map(|s| s.to_string());
            let out = v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
            acc.tool_results.push(ToolResultRecord {
                call_id: call_id.clone(),
                name: v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()),
                output_preview: out.chars().take(200).collect(),
                is_error: out.to_lowercase().contains("error") || out.contains("错误"),
            });
            // 结果 ↔ 调用的关联在解析结束后统一对账（reconcile_results）
            return;
        }
    }

    let delta = match v.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta")) {
        Some(d) => d,
        None => return,
    };

    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            acc.last_delta_type = LastDeltaType::Content;
            note_intent(acc, content);
        }
    }

    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
        if !tool_calls.is_empty() {
            acc.last_delta_type = LastDeltaType::ToolCalls;
            for tc in tool_calls {
                let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                let rec = tool_call_by_index(acc, index);
                if let Some(name) = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()) {
                    if !name.is_empty() {
                        rec.name = name.to_string();
                    }
                }
                if let Some(args) = tc.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()) {
                    rec.arguments.push_str(args);
                }
                if let Some(cid) = tc.get("id").and_then(|i| i.as_str()) {
                    rec.call_id = Some(cid.to_string());
                }
            }
        }
    }

    if let Some(fr) = v.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("finish_reason")).and_then(|f| f.as_str()) {
        if !fr.is_empty() {
            acc.finish_reason = Some(fr.to_string());
            // 注意：finish_reason 是终止符，不覆盖 last_delta_type
            // （last_delta_type 应反映最后一个"数据" delta：content / tool_calls）
        }
    }
}

fn parse_responses_api(acc: &mut Accumulator, v: &Value) {
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match ty {
        "response.output_item.added" | "response.output_item.done" => {
            if let Some(item) = v.get("item") {
                let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if item_type == "function_call" {
                    acc.last_delta_type = LastDeltaType::ToolCalls;
                    let call_id = item.get("call_id").and_then(|c| c.as_str()).map(|s| s.to_string());
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    let args = item.get("arguments").and_then(|a| a.as_str()).unwrap_or("").to_string();
                    // added / done 可能重复出现同一调用，按 call_id（或名称）去重合并
                    upsert_tool_call(acc, call_id, name, args);
                } else if item_type == "function_call_output" {
                    let out = item.get("output").and_then(|o| o.as_str()).unwrap_or("").to_string();
                    let call_id = item.get("call_id").and_then(|c| c.as_str()).map(|s| s.to_string());
                    acc.tool_results.push(ToolResultRecord {
                        call_id,
                        name: None,
                        output_preview: out.chars().take(200).collect(),
                        is_error: out.to_lowercase().contains("error") || out.contains("错误"),
                    });
                }
            }
        }
        "response.output_item.delta" => {
            if let Some(item) = v.get("item") {
                if item.get("type").and_then(|t| t.as_str()).unwrap_or("") == "function_call" {
                    acc.last_delta_type = LastDeltaType::ToolCalls;
                    if let Some(args) = item.get("arguments").and_then(|a| a.as_str()) {
                        if let Some(rec) = acc.tool_calls.last_mut() {
                            rec.arguments.push_str(args);
                        }
                    }
                }
            }
        }
        "response.function_call_arguments.delta" => {
            acc.last_delta_type = LastDeltaType::ToolCalls;
            if let Some(d) = v.get("delta").and_then(|x| x.as_str()) {
                if let Some(rec) = acc.tool_calls.last_mut() {
                    rec.arguments.push_str(d);
                }
            }
        }
        "response.completed" => {
            acc.responses_completed = true;
            // 兜底：从 response.output 汇总
            if let Some(output) = v.get("response").and_then(|r| r.get("output")).and_then(|o| o.as_array()) {
                for item in output {
                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if item_type == "function_call" {
                        acc.last_delta_type = LastDeltaType::ToolCalls;
                        let call_id = item.get("call_id").and_then(|c| c.as_str()).map(|s| s.to_string());
                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                        let args = item.get("arguments").and_then(|a| a.as_str()).unwrap_or("").to_string();
                        upsert_tool_call(acc, call_id, name, args);
                    } else if item_type == "function_call_output" {
                        let out = item.get("output").and_then(|o| o.as_str()).unwrap_or("").to_string();
                        acc.tool_results.push(ToolResultRecord {
                            call_id: item.get("call_id").and_then(|c| c.as_str()).map(|s| s.to_string()),
                            name: None,
                            output_preview: out.chars().take(200).collect(),
                            is_error: out.to_lowercase().contains("error") || out.contains("错误"),
                        });
                    }
                }
            }
            // 注意：response.completed 是正常终止符，不覆盖 last_delta_type
        }
        _ => {}
    }
}

fn parse_message_like(acc: &mut Accumulator, v: &Value) {
    // 顶层 role:tool 消息（导出格式）
    if let Some(role) = v.get("role").and_then(|r| r.as_str()) {
        if role == "tool" || role == "function" {
            let call_id = v.get("tool_call_id").and_then(|c| c.as_str()).map(|s| s.to_string());
            let out = v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
            acc.tool_results.push(ToolResultRecord {
                call_id: call_id.clone(),
                name: v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()),
                output_preview: out.chars().take(200).collect(),
                is_error: out.to_lowercase().contains("error") || out.contains("错误"),
            });
            if let Some(cid) = call_id {
                for tc in acc.tool_calls.iter_mut() {
                    if tc.had_result == false {
                        tc.had_result = true;
                        let _ = &cid;
                        break;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

fn tool_invocations(acc: &Accumulator) -> usize {
    acc.tool_calls.iter().filter(|t| !t.name.is_empty()).count()
}

fn any_tool_call_truncated(acc: &Accumulator) -> bool {
    // 有名字但参数明显未闭合（不以 } 结尾且未收到 tool_calls finish）
    let expecting_tool_finish = acc.finish_reason.as_deref() == Some("tool_calls");
    for tc in &acc.tool_calls {
        if tc.name.is_empty() {
            return true; // 名字都没拿到 = 截断
        }
        if !expecting_tool_finish && !tc.arguments.ends_with('}') && !tc.arguments.is_empty() {
            // 未结束且无正常 finish（除非是已知无需 JSON 的工具，仍保守判为可疑）
            return true;
        }
    }
    false
}

fn classify(acc: &Accumulator) -> (Category, SuspectedLayer, String) {
    let invocations = tool_invocations(acc);
    let results = acc.tool_calls.iter().filter(|t| t.had_result).count();
    let ended_cleanly = acc.ended_with_done || acc.finish_reason.is_some() || acc.responses_completed;

    // 流异常断开（无 [DONE] / finish_reason / response.completed）
    if !ended_cleanly {
        return match acc.last_delta_type {
            LastDeltaType::ToolCalls => (
                Category::A,
                SuspectedLayer::Inconclusive,
                "流式在上游工具调用分片途中断开（未收到 [DONE]/finish_reason），疑似代理层或传输层中断；若此段为上游原始响应则偏第三方 API / 传输层".into(),
            ),
            LastDeltaType::Content => (
                Category::A,
                SuspectedLayer::Inconclusive,
                "流式在文本输出途中断开（未收到 [DONE]/finish_reason），疑似代理层或传输层中断".into(),
            ),
            _ => (
                Category::A,
                SuspectedLayer::Inconclusive,
                "流异常结束（无终止信号），疑似被截断".into(),
            ),
        };
    }

    match acc.finish_reason.as_deref() {
        Some("tool_calls") => {
            if any_tool_call_truncated(acc) {
                (
                    Category::A,
                    SuspectedLayer::ThirdPartyApi,
                    "finish_reason=tool_calls 但工具调用参数/名字被截断（不完整），疑似上游未发全".into(),
                )
            } else if invocations > 0 && results < invocations {
                (
                    Category::Other,
                    SuspectedLayer::Inconclusive,
                    format!(
                        "模型正常发出 {} 个工具调用（finish_reason=tool_calls），代理应继续下一轮；若 Codex 未执行则为 Codex 层问题。已观察返回结果 {} 个",
                        invocations, results
                    ),
                )
            } else {
                (
                    Category::Other,
                    SuspectedLayer::Inconclusive,
                    "模型正常发出工具调用，等待执行（非终止，属正常多轮链路）".into(),
                )
            }
        }
        Some("stop") | Some("end_turn") | Some("") => {
            if invocations > 0 && invocations == results {
                (
                    Category::B,
                    SuspectedLayer::Inconclusive,
                    "最后工具调用均成功执行并返回结果，模型随后输出结论并自然停止（B 类：模型主动结束本轮）".into(),
                )
            } else if invocations == 0 {
                if acc.content_intent_hit {
                    (
                        Category::A,
                        SuspectedLayer::ThirdPartyApi,
                        "纯文本结束但内容含「调用/工具」等意图词且无 tool_calls 字段，疑似上游未发送工具调用（A 类）".into(),
                    )
                } else {
                    (
                        Category::B,
                        SuspectedLayer::Inconclusive,
                        "模型以纯文本结论自然结束（B 类：无工具调用）".into(),
                    )
                }
            } else {
                (
                    Category::B,
                    SuspectedLayer::Inconclusive,
                    "模型输出结论并停止（B 类：模型主动结束本轮）".into(),
                )
            }
        }
        Some(other) => (
            Category::Other,
            SuspectedLayer::Inconclusive,
            format!("finish_reason={other}，需人工结合工具执行链路判断", ),
        ),
        None => {
            // 已正常结束（[DONE] 或 response.completed）但无 finish_reason：
            // Responses API 本就无 finish_reason，属正常；仅当最后仍在流工具调用
            // 却无 finish_reason 时才疑似截断。
            if acc.last_delta_type == LastDeltaType::ToolCalls && !acc.responses_completed {
                (
                    Category::A,
                    SuspectedLayer::Inconclusive,
                    "流以 [DONE] 结束但最后 delta 为 tool_calls 且无 finish_reason，工具调用疑似被截断".into(),
                )
            } else {
                (
                    Category::Other,
                    SuspectedLayer::Inconclusive,
                    "流正常结束（[DONE] / response.completed）但无 finish_reason 字段".into(),
                )
            }
        }
    }
}

fn format_name(fmt: Format) -> &'static str {
    match fmt {
        Format::ChatCompletions => "chat_completions",
        Format::ResponsesApi => "responses_api",
        Format::Unknown => "unknown",
    }
}

/// 按 call_id（优先）或 name 去重合并一个工具调用；已存在则补全 name/arguments。
fn upsert_tool_call(acc: &mut Accumulator, call_id: Option<String>, name: String, args: String) {
    if let Some(cid) = &call_id {
        if let Some(rec) = acc.tool_calls.iter_mut().find(|t| t.call_id.as_deref() == Some(cid.as_str())) {
            if !name.is_empty() {
                rec.name = name;
            }
            if !args.is_empty() {
                rec.arguments = args;
            }
            return;
        }
    }
    // 无 call_id 或按 call_id 未命中时，按 name 合并（同一调用多事件去重）
    if let Some(rec) = acc.tool_calls.iter_mut().find(|t| !t.name.is_empty() && t.name == name) {
        if !args.is_empty() {
            rec.arguments = args;
        }
        if call_id.is_some() {
            rec.call_id = call_id;
        }
        return;
    }
    acc.tool_calls.push(ToolCallRecord {
        index: acc.tool_calls.len(),
        call_id,
        name,
        arguments: args,
        had_result: false,
    });
}

/// 解析结束后，把工具返回结果与工具调用按 call_id 关联，未带 call_id 时按序兜底。
fn reconcile_results(acc: &mut Accumulator) {
    for r in &acc.tool_results {
        if let Some(cid) = &r.call_id {
            for tc in acc.tool_calls.iter_mut() {
                if tc.call_id.as_deref() == Some(cid.as_str()) {
                    tc.had_result = true;
                }
            }
        }
    }
    // 兜底：若返回结果数 >= 工具调用数且仍有未匹配的，按出现顺序填充
    let unmatched = acc.tool_calls.iter().filter(|t| !t.had_result).count();
    if unmatched > 0 && !acc.tool_results.is_empty() && acc.tool_results.len() >= acc.tool_calls.len() {
        let mut ri = 0;
        for tc in acc.tool_calls.iter_mut() {
            if !tc.had_result && ri < acc.tool_results.len() {
                tc.had_result = true;
                ri += 1;
            }
        }
    }
}

/// 分析单段日志。
pub fn analyze_transcript(raw: &str) -> DiagnosticReport {
    let mut acc = Accumulator::default();
    parse_into(&mut acc, raw);
    reconcile_results(&mut acc);
    acc.ended_with_done = raw.lines().any(|l| {
        let t = l.trim();
        t == "[DONE]" || t.ends_with("data: [DONE]") || t.starts_with("data: [DONE]")
    });

    let (category, suspected_layer, reasoning) = classify(&acc);

    DiagnosticReport {
        source_format: format_name(acc.format).to_string(),
        model: acc.model.clone(),
        num_events: acc.num_events,
        tool_calls: acc.tool_calls.clone(),
        tool_results_observed: acc.tool_results.len(),
        tool_invocations: tool_invocations(&acc),
        last_delta_type: acc.last_delta_type,
        finish_reason: acc.finish_reason.clone(),
        ended_cleanly: acc.ended_with_done || acc.finish_reason.is_some() || acc.responses_completed,
        category,
        suspected_layer,
        reasoning,
    }
}

/// 配对分析：上游原始响应 vs 代理转发给 Codex 的响应。
pub fn analyze_pair(upstream_raw: &str, proxy_emitted_raw: &str) -> PairReport {
    let upstream = analyze_transcript(upstream_raw);
    let proxy_emitted = analyze_transcript(proxy_emitted_raw);

    // 定位故障层
    let (pinned, reason) = if upstream.category == Category::A && proxy_emitted.category == Category::B {
        (
            SuspectedLayer::Proxy,
            "上游原始响应为 A 类（被截断），但代理转发给 Codex 的响应却正常 —— 故障在代理层（转发时丢失/重排了工具调用）".to_string(),
        )
    } else if upstream.category == Category::A && proxy_emitted.category == Category::A {
        (
            SuspectedLayer::ThirdPartyApi,
            "上游与代理转发均为 A 类 —— 故障在上游第三方 API（代理忠实透传了截断的响应）".to_string(),
        )
    } else if upstream.category == Category::B && proxy_emitted.category == Category::B {
        (
            SuspectedLayer::Inconclusive,
            "上下游均为 B 类（正常结束） —— 若 Codex 仍显示中断，则故障在 Codex 客户端侧（收到但未执行 / UI 未渲染）".to_string(),
        )
    } else if upstream.tool_invocations > 0 && proxy_emitted.tool_invocations == 0 {
        (
            SuspectedLayer::Proxy,
            "上游发出了工具调用，但代理转发给 Codex 的响应里 tool_calls 数量为 0 —— 故障在代理层（协议转换丢失工具调用）".to_string(),
        )
    } else {
        (
            SuspectedLayer::Inconclusive,
            "上下游对比无法唯一确定故障层，请结合 Codex 侧 debug 日志进一步判断".to_string(),
        )
    };

    PairReport {
        upstream,
        proxy_emitted,
        pinned_layer: pinned,
        reasoning: reason,
    }
}

/// 如何从 Codex 桌面版 / Work 版获取 debug 日志（返回纯文本说明，便于在 UI/文档展示）。
pub fn how_to_capture_logs() -> &'static str {
    r#"
如何获取 Codex / ChatGPT Work 的 debug 日志并喂入本分析逻辑
===========================================================

1) Codex CLI（OpenAI 官方 codex / 兼容实现）
   - 设置环境变量开启调试：
       export CODEX_DEBUG=1
       # 或
       export OPENAI_LOG=debug
   - 运行对话后，日志会输出到 stderr / ~/.codex/logs/。
   - 捕获某一轮的 SSE：在日志中定位对应请求的响应体（通常为
     `data: {...}` 逐行流），复制保存为 upstream.log（上游视角，Chat Completions 格式）。

2) ChatGPT Work / 桌面版（走本代理的 Responses API 路径）
   - 打开开发者工具 / 调试控制台（通常 Help → Toggle Developer Tools，
     或快捷键 Ctrl+Shift+I / Cmd+Option+I）。
   - 在 Network / Console 面板筛选对代理地址（如 http://127.0.0.1:<port>/v1/...）
     的请求，复制 Response 的 SSE 文本保存为 proxy.log（代理视角，Responses API 格式）。

3) 喂入分析
   - 单段：     diagnose upstream.log
   - 上下游配对： diagnose upstream.log proxy.log
   - 管道：     cat upstream.log | diagnose
   - 输出为 JSON 结构化报告（见 DiagnosticReport / PairReport）。

4) 判定要点（与界面现象对应）
   - 若报告 category=A 且 last_delta_type=tool_calls、ended_cleanly=false
     → 流式在工具调用分片途中被砍，界面表现「准备调工具却停了」。
   - 若报告 category=B 且工具调用均有结果 → 模型主动结束，正常现象，
     不应归咎于代理。
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_b_active_end() {
        let log = r#"[
          {"choices":[{"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"shell","arguments":""}}]}}]},
          {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"cmd\":\"ls\"}"}}]}}]},
          {"choices":[{"delta":{},"finish_reason":"tool_calls"}]},
          {"role":"tool","tool_call_id":"call_1","name":"shell","content":"file1\nfile2"},
          {"choices":[{"delta":{"role":"assistant","content":"这个目录有两个文件。"}}]},
          {"choices":[{"delta":{},"finish_reason":"stop"}]}
        ]"#;
        let r = analyze_transcript(log);
        assert_eq!(r.category, Category::B, "应为 B 类主动结束: {}", r.reasoning);
        assert_eq!(r.tool_invocations, 1);
        assert_eq!(r.tool_results_observed, 1);
        assert_eq!(r.last_delta_type, LastDeltaType::Content);
        assert_eq!(r.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn test_a_stream_cut_mid_tool_call() {
        let log = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"好的，我来调用工具查看。\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_2\",\"type\":\"function\",\"function\":{\"name\":\"shell\",\"arguments\":\"\"}}]}}]}\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"cmd\\\":\\\"cat \"}}]}}]}";
        let r = analyze_transcript(log);
        assert_eq!(r.category, Category::A, "应为 A 类截断: {}", r.reasoning);
        assert_eq!(r.last_delta_type, LastDeltaType::ToolCalls);
        assert!(!r.ended_cleanly);
    }

    #[test]
    fn test_a_content_only_with_intent() {
        // 纯文本结束、含调用意图词、无 tool_calls → 疑似 A
        let log = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"好的，我来调用工具查看这个页面。\"}}]}\n\
data: {\"choices\":[{\"delta\":{}, \"finish_reason\":\"stop\"}]}";
        let r = analyze_transcript(log);
        assert_eq!(r.category, Category::A, "纯文本+意图词应判 A: {}", r.reasoning);
        assert_eq!(r.tool_invocations, 0);
    }

    #[test]
    fn test_responses_api_tool_call_then_completed() {
        let log = r#"{"type":"response.output_item.added","item":{"type":"function_call","name":"read_file","arguments":"","call_id":"fc_1"}}
{"type":"response.output_item.delta","item":{"type":"function_call","arguments":"{\"path\":\"/x\"}"}}
{"type":"response.output_item.done","item":{"type":"function_call","name":"read_file","arguments":"{\"path\":\"/x\"}","call_id":"fc_1"}}
{"type":"response.completed","response":{"output":[{"type":"function_call","name":"read_file","arguments":"{\"path\":\"/x\"}","call_id":"fc_1"}]}}"#;
        let r = analyze_transcript(log);
        assert_eq!(r.source_format, "responses_api");
        assert_eq!(r.tool_invocations, 1);
        // 仅一轮、无 stop 收尾 → Other（等待执行），非 A 非 B
        assert_eq!(r.category, Category::Other);
    }

    #[test]
    fn test_pair_pins_proxy_layer() {
        // 上游完整发出了工具调用，但代理转发给 Codex 的响应里却没有任何 function_call
        // → 故障在代理层（协议转换丢失工具调用）。
        let upstream = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"shell\",\"arguments\":\"{\\\"cmd\\\":\\\"ls\\\"}\"}}]}}]}\n\
data: {\"choices\":[{\"delta\":{}, \"finish_reason\":\"tool_calls\"}]}";
        let proxy = "{\"type\":\"response.output_item.added\",\"item\":{\"type\":\"message\",\"content\":\"好的\"}}\n\
{\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"content\":\"好的\"}]}}";
        let p = analyze_pair(upstream, proxy);
        assert_eq!(p.pinned_layer, SuspectedLayer::Proxy, "代理丢失工具调用应钉死 Proxy 层: {}", p.reasoning);
        assert_eq!(p.upstream.tool_invocations, 1);
        assert_eq!(p.proxy_emitted.tool_invocations, 0);
    }

    #[test]
    fn test_pair_pins_third_party_api() {
        // 上游与代理转发均为 A 类（都被截断）→ 故障在上游第三方 API。
        let upstream = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c2\",\"function\":{\"name\":\"shell\",\"arguments\":\"{\"cmd\":\"cat \"}}]}}]}\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"";
        let proxy = "{\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"name\":\"shell\",\"arguments\":\"{\"cmd\":\"cat \"";
        let p = analyze_pair(upstream, proxy);
        assert_eq!(p.pinned_layer, SuspectedLayer::ThirdPartyApi, "上下游均截断应钉死 ThirdPartyApi: {}", p.reasoning);
    }
}
