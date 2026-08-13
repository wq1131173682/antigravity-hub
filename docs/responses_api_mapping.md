# Responses API ↔ Chat Completions 双向字段映射文档

> ⚠️ **已封存（v5.3.10）**：Codex 集成与 Responses API 协议转换已于 v5.3.10 封存停用（`CODEX_ENABLED=false`，代码保留）。该转换器当前不启用，本文档仅供历史参考。

> 适用范围：`src-tauri/src/modules/responses_bridge.rs`（核心转换器）与
> `src-tauri/src/bin/responses_relay.rs`（中转网关服务，端口 8046）。
> 本文档是 **v2 修复版**（多轮工具调用会话不被提前终止）的权威映射说明。

---

## 1. 架构与数据流

```
┌──────────────┐  POST /v1/chat/completions   ┌───────────────────────────┐
│  Chat 客户端  │ ───────────────────────────► │     responses_relay      │
│ (ChatGPT Work│                              │                           │
│  Codex CLI …)│                              │ ① 请求 Chat → Responses   │
│              │ ◄─────────────────────────── │ ② 注入 previous_response_id│
│   Chat delta │    SSE 流 (data: {...})      │ ③ 上游 Responses SSE      │
│   SSE 流     │                              │    → Chat delta（边收边转）│
└──────────────┘                              │ ④ 心跳 : ping + 延迟 finish│
                                              └────────────┬──────────────┘
                                                           │ /v1/responses (SSE)
                                                           ▼
                                                   ┌──────────────────┐
                                                   │ 上游 Responses    │
                                                   │ API 服务          │
                                                   └──────────────────┘
```

**多轮工具调用链路（重点）**：

```
第 1 轮  Chat: user "查天气"                     → Responses: input=[user], no prev_id
  上游返回 function_call(weather)                → Chat: delta.tool_calls + finish=tool_calls
客户端执行工具，追加 tool 结果消息

第 2 轮  Chat: messages 追加 assistant(tool_calls)+tool(result)
                                                 → Responses: previous_response_id=resp_1
                                                    + input=[function_call_output]（增量）
  上游返回 function_call(search)                 → Chat: delta.tool_calls + finish=tool_calls
客户端继续执行……

第 N 轮  Chat: 无新 tool，模型给出最终答复
                                                 → Responses: previous_response_id=resp_{N-1}
  上游返回 message（纯文本，无 function_call）    → Chat: delta.content + finish=stop
```

> 关键：网关为每条下游会话（key = model + 首条 user 内容哈希）缓存
> `previous_response_id`，每轮请求注入续接，**上下文标识不丢失**，
> 模型才能持续发起下一轮工具调用。

---

## 2. 请求方向：Chat Completions → Responses API

### 2.1 顶层字段

| Chat Completions | Responses API | 说明 |
|---|---|---|
| `model` | `model` | 透传 |
| `messages`（数组） | `input`（数组）+ `instructions` | 见 2.2 |
| `stream` | `stream` | 透传 |
| `max_tokens` | `max_output_tokens` | 改名 |
| `temperature` / `top_p` / `stop` / `seed` / `user` / `n` | 同名 | 透传 |
| `presence_penalty` / `frequency_penalty` / `logit_bias` | 同名 | 透传 |
| `tools[].function.{name,description,parameters,strict}` | `tools[].{name,description,parameters,strict}` | 反嵌套（type=function） |
| `tool_choice.function.name` | `tool_choice.name` | 反嵌套 |
| `response_format` | `text.format` | 见 2.3 |
| *（无）* | `previous_response_id` | v2 新增：会话续接注入 |
| *（无）* | `instructions` | 由第一条 system 消息提取（仅全量模式） |

### 2.2 messages → input 数组

| messages[].role | Responses input 项 | 说明 |
|---|---|---|
| `system`（仅第一条，且为全量模式） | `instructions`（顶层） | 其余 system 当 message |
| `user` / `system`（非首条） | `{"type":"message","role":"…","content":[…]}` | content 见 2.3 |
| `assistant`（纯文本） | `{"type":"message","role":"assistant","content":[{"type":"output_text","text":…}]}` | |
| `assistant`（含 tool_calls） | 文本部分 → `message` 项；每个 tool_call → `{"type":"function_call","id":…,"call_id":…,"name":…,"arguments":…}` | 先文本后 function_call |
| `tool` | `{"type":"function_call_output","call_id":…,"output":…}` | |

### 2.3 content 转换

| Chat content | Responses content |
|---|---|
| 字符串 `"Hello"` | `[{"type":"input_text","text":"Hello"}]` |
| `[{"type":"text",…}]` | `[{"type":"input_text",…}]` |
| `[{"type":"image_url",…}]` | `[{"type":"input_image",…}]` |
| `[{"type":"file",…}]` / `[{"type":"audio",…}]` | `input_file` / `input_audio` |
| `null` / 空串 | `[{"type":"input_text","text":""}]` |

### 2.4 response_format → text.format

```
Chat:  {"type":"json_schema","json_schema":{"name":"answer","schema":{…},"strict":true}}
Responses: {"text":{"format":{"type":"json_schema","name":"answer","schema":{…},"strict":true}}}
```

---

## 3. 响应方向：Responses API SSE → Chat Completions delta

### 3.1 事件 → delta 映射

| Responses SSE 事件 | Chat delta 输出 | 时机 |
|---|---|---|
| `response.created` | 记录 response.id / model / created_at；`role="assistant"` 初始 chunk（惰性） | 首个 delta 前 |
| `response.output_item.added`（message） | 记录文本项 | — |
| `response.output_text.delta` | `choices[0].delta.content` | 即时转发 |
| `response.output_item.added`（function_call） | `choices[0].delta.tool_calls[]: {index,id,type:"function",function:{name,arguments:""}}` | 即时转发 |
| `response.function_call_arguments.delta` | `choices[0].delta.tool_calls[].function.arguments`（**原始分片，不合并不截断**） | 即时转发 |
| `response.completed` / `response.done` | **仅更新状态，不下发 finish**（v2 修复） | — |
| `data: [DONE]` | **仅标记，不结束流**（v2 修复） | — |
| 上游流 EOF（连接关闭） | `finish_reason`（唯一权威收尾点）+ `data: [DONE]` | 流末尾 |
| `response.failed` / `error` 事件 | error chunk + `finish_reason:"error"`，结束流 | 硬错误 |
| 会话超时 / 首片超时 / 空闲超时 | `finish_reason` + `[DONE]`（优雅收尾） | 异常路径 |

### 3.2 finish_reason 判定规则（v2 核心）

| 条件 | finish_reason |
|---|---|
| 整个流中出现过 `function_call`（任意轮次） | `tool_calls` |
| 整个流中从未出现 `function_call`（纯文本会话结束） | `stop` |
| `response.failed` / 协议级 `error` 事件 | `error` |
| 上游返回 `status: incomplete`（非流式，max_tokens 截断） | `length` |

> **严禁随意生成 `stop`**：只有上游 SSE 流真正结束（EOF / 超时 / 硬错误）时
> 才由统一收尾点下发 finish_reason；`response.completed`、`[DONE]` 事件
> 一律**不注入**任何终止标记，确保模型输出阶段性文本后仍能继续发起工具调用。

### 3.3 非流式：Responses body → Chat completion

| Responses | Chat Completions |
|---|---|
| `id` (`resp_xxx`) | `id` (`chatcmpl-xxx`) |
| `object:"response"` | `object:"chat.completion"` |
| `output[]` message.content → 文本 | `choices[0].message.content`（拼接） |
| `output[]` function_call | `choices[0].message.tool_calls[]` |
| `status` | 见 3.2 finish_reason 表 |
| `usage.input_tokens` / `output_tokens` | `usage.prompt_tokens` / `completion_tokens` |

---

## 4. 工具调用结构兼容性（规范 5）

多轮连续工具调用时，保证以下字段完整、不串号：

| 保证项 | 实现 |
|---|---|
| `tool_call_id`（call_id） | 以 `item_id` 为主键跟踪，`call_id` 原样透传（`call_id` 缺失时回退 `id`） |
| `index` 连续 | chat 索引由 `next_tool_chat_index` 全流连续分配，多 response 会话不重置、不冲突 |
| `name` | 初始 delta 携带 `type:"function"` + `function.name` |
| 参数分片 | `function_call_arguments.delta` **原始分片即时转发**（不合并、不缓冲、不截断），客户端自行拼接 |
| 交错调用 | 两个工具调用交替推送时，按 `item_id`/`output_index` 独立定位，不互相覆盖 |

---

## 5. 会话上下文（规范 2）：response_id 缓存

- **存储**：`SessionStore`（线程安全，`Mutex<HashMap<session_key, SessionContext>>`，LRU 淘汰）。
- **session_key**：`model + 第一条 user 消息内容哈希`——同一轮多轮调用的所有请求共享根 user 消息，key 稳定。
- **缓存项**：`{ previous_response_id: Option<String>, processed_msg_len: usize }`。
- **请求注入**：有 `previous_response_id` 时，仅发送 `messages[processed_msg_len..]` 增量作为 input，
  并注入顶层 `previous_response_id`；无新增消息时发送空 input（上下文由 response_id 携带）。
- **响应回写**：流结束时由 `StreamHooks.on_complete` 回调回写最新 response_id；非流式从响应体 `id` 字段提取。
- **自动降级**：上游返回 4xx 且错误体含 `previous_response_id` → 清除该会话缓存，以全量模式重试一次；
  `context_mode = "full"` 可完全禁用续接。

---

## 6. 可调整参数（超时 / 心跳 / 上下文）

| 参数 | 默认值 | 环境变量 | 说明 |
|---|---|---|---|
| `context_mode` | `response_id` | `RESPONSES_RELAY_CONTEXT_MODE` | `response_id` 多轮续接 / `full` 全量无状态 |
| `max_session_contexts` | `1024` | `RESPONSES_RELAY_MAX_SESSION_CONTEXTS` | 会话上下文缓存条目上限 |
| `connect_timeout_secs` | `30` | `RESPONSES_RELAY_CONNECT_TIMEOUT` | 上游连接超时 |
| `read_timeout_secs` | `600` | `RESPONSES_RELAY_READ_TIMEOUT` | reqwest 整体墙钟上限（长任务调大） |
| `session_max_duration_secs` | `600` | `RESPONSES_RELAY_SESSION_MAX_DURATION` | 单请求会话最大时长（超时优雅收尾） |
| `heartbeat_interval_secs` | `15` | `RESPONSES_RELAY_HEARTBEAT_INTERVAL` | SSE 心跳间隔（`: ping` 注释行） |
| `first_chunk_timeout_secs` | `120` | `RESPONSES_RELAY_FIRST_CHUNK_TIMEOUT` | 等待首个有效分片 |
| `chunk_idle_timeout_secs` | `300` | `RESPONSES_RELAY_CHUNK_IDLE_TIMEOUT` | 相邻分片空闲上限（心跳不推迟它） |

> 长任务（多轮 Agent）推荐：`read_timeout_secs=1800, session_max_duration_secs=1800,
> heartbeat_interval_secs=10, first_chunk_timeout_secs=180, chunk_idle_timeout_secs=600`。

### 心跳 vs 空闲超时的关系（规范 4）

- `last_activity`：任何上游字节到达即刷新 → 驱动心跳计时。
- `last_data`：任何可解析 SSE 行刷新 → 驱动空闲/首片超时。
- 心跳发送**只**重置 `last_activity`，**不**重置 `last_data`——保证死流仍会在
  `chunk_idle_timeout_secs` 后收尾，同时正常空闲的会话靠心跳不被网关切断。

---

## 7. 容错（规范 6）

| 异常 | 行为 |
|---|---|
| 单行 JSON 解析失败 | 跳过该块，继续读流 |
| SSE 多行片段未拼完整 | 累积到缓冲区，拼成合法 JSON 再处理；超 1MB 丢弃缓冲继续 |
| 参数 delta 指向未知工具调用 | 跳过该参数块（日志告警），不中断流 |
| 未知事件类型 | 尝试提取 `delta` / `text` 字段转发 |
| `response.failed` / `error` 事件 | 发送 error chunk + `finish_reason:"error"`，结束流 |
| 上游流错误 / 连接断开 | 若未收尾则补发 finish + `[DONE]`（优雅关闭，客户端不卡死） |

---

## 8. 测试用例（`cargo test --bin responses_relay`）

| 用例 | 验证点 |
|---|---|
| `test_stream_simple_text` | 纯文本流，finish=stop 在流末尾 |
| `test_stream_tool_call` | 单工具调用 id/name/参数完整，finish=tool_calls |
| `test_stream_multi_round_tool_calls` | 单响应内多工具交错，index 不串号 |
| `test_stream_multi_response_no_premature_stop` | **回归**：多 response 连续推送（文本→工具），全流无 stop、唯一 finish=tool_calls |
| `test_stream_done_midstream_not_terminate` | **回归**：中途 [DONE] 不终结，后续事件正常下发 |
| `test_stream_arg_deltas_forwarded_as_is` | 参数分片原样转发，不合并 |
| `test_stream_malformed_chunk_skipped` | 坏块跳过，流不中断 |
| `test_stream_keepalive_heartbeat` | 空闲时下发 `: ping` 心跳 |
| `test_stream_idle_timeout` | 死流仍被空闲超时清理（心跳不无限续命） |
| `test_stream_complete_hook_fires` | 流结束钩子回传 response_id |
| `test_chat_to_responses_with_previous_response_id` | 请求注入 previous_response_id |
| `test_chat_to_responses_incremental_mode` | 增量 input 只含新增消息 |
| `test_session_store_context` / `test_session_store_eviction` | 会话 key 稳定 / LRU 淘汰 |
| `test_responses_to_chat_incomplete_status` | incomplete → finish_reason=length |
