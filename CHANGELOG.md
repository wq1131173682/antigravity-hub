# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 的格式约定。版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [5.3.3] - 2026-08-11

### 修复
- **WorkBuddy 会话探测请求不再被拒绝**：WorkBuddy 建连探测请求的 body 可能只有 HTTP 请求行 + 请求头（无 `\r\n\r\n` 分隔符、无内嵌 JSON）。此前 `extract_json_from_http_text` 提取失败后直接返回 400，导致会话建立失败。现新增 `looks_like_http_request_text` 识别此类请求，返回 200 确认而非 400，让后续真实对话请求正常进入代理
- 新增 6 个单元测试覆盖探测请求识别与负例场景

## [5.3.2] - 2026-08-11

### 修复
- **多轮工具调用会话提前终止（主代理 Responses 路径）**：`transform_stream_to_responses` 收到上游 `finish_reason` 或 `data: [DONE]` 时立即结束流并下发 `response.completed`——当上游在输出文本段后（`finish_reason: stop` / `[DONE]`）继续推送工具调用时，后续 `tool_calls` 全部丢失，客户端提前结束会话、无法发起下一轮工具调用
  - 修复：`finish_reason` / `[DONE]` 仅记录状态（`saw_finish_reason` / `seen_done`），**只有上游流 EOF / 超时 / 硬错误才收尾**（与 `responses_bridge` 的终结语义对齐）
  - 新增 `STREAM_POST_TERMINATION_WINDOW_SECS=5` 尾声窗口：`[DONE]`/`finish_reason` 后 5 秒内无新数据即收尾，多段输出上游仍可继续推流，单段上游不挂起
  - 纯协议级通用实现，不区分平台与下游工具
- 新增 3 个流式回归测试（`finish_reason` 后继续 tool_calls 不丢失、`[DONE]` 后内容继续、尾声窗口自动收尾）

## [5.3.1] - 2026-08-11

### 修复
- **兼容客户端发送「HTTP 请求文本」作为请求体的场景**（WorkBuddy 等 Agent 客户端会话首请求）：代理收到非 JSON body 时，若为完整 HTTP/1.1 请求文本（请求行 + 请求头），提取内嵌 JSON payload 正常转发；无法解析的非 JSON body 返回明确的 400 错误提示，不再原样透传导致上游 `400 invalid arguments`
- 新增 6 个单元测试覆盖提取与负例场景

## [5.3.0] - 2026-08-11

### 修复（线上 BUG）
- **多轮工具调用会话提前终止**：Agent 自动多轮连续工具调用场景下，完成单次 `tool_call`、输出一段回复文本之后会话直接终止（ChatGPT Work 等 Chat 客户端不再发起下一轮）
  - 根因：旧版在收到 `response.completed` / `data: [DONE]` 事件时立即向下游注入 `finish_reason: stop`；且请求方向不维护 `previous_response_id`，依赖 response_id 续接的上游多轮后上下文断裂
  - 修复：终结信号只在上游 SSE 流真正结束时下发（EOF / 超时 / 硬错误）；`response.completed` 与 `[DONE]` 仅更新状态，绝不注入 stop；`finish_reason` 流末统一判定（出现过 function_call → `tool_calls`，否则 `stop`）
- **response_id 会话上下文续接**：新增会话上下文缓存（`SessionStore`），为每条下游会话缓存 `previous_response_id`，每轮请求注入 `previous_response_id` + 增量 `input`，维持 Responses API 多轮工具调用链路；上游拒绝续接（400/422）时自动清除缓存并以全量模式重试一次
- **死流不被清理**：心跳计时与空闲超时解耦（心跳不再重置数据时间），空闲超时仍能清理死流
- **多 response 会话工具调用串号**：`tool_calls` 改以 `item_id` 为主键 + `output_index→item_id` 辅助映射，多 response 的 output_index 重置不再互相覆盖
- **`/v1/models` 端点 404**：`models_handler` 此前未注册到路由，现已修复
- 非流式响应 `status=incomplete`（max_tokens 截断）映射为 `finish_reason: length`，不再误报 `error`

### 变更
- 新增配置：`context_mode`（`response_id` 多轮续接 / `full` 全量无状态）、`max_session_contexts`（会话缓存上限）
- 新增文档：`docs/responses_api_mapping.md`（Responses API ↔ Chat Completions 双向往返字段映射 + 可调参数说明）

### 测试
- `responses_relay` 单元测试 17 → 27 个，新增覆盖：多 response 连续推送不提前终止、流中途 `[DONE]` 不终结、参数分片原样转发、坏块跳过、心跳不无限续命死流、会话上下文（previous_response_id / 增量 input / LRU 淘汰）

## [5.2.23] - 2026-08-11

### 新增
- **Responses API 中转转发服务**（`responses_relay` 独立二进制）：将上游原生 Responses API 请求/流式响应，双向转换为标准 OpenAI `/v1/chat/completions` 格式，供仅支持 Chat 协议的客户端使用
  - 请求方向：Chat Completions → Responses API（`messages`→`input`/`instructions`、`tool_calls`→`function_call`、`max_tokens`→`max_output_tokens`、`tools`/`tool_choice`/`response_format` 格式转换）
  - 流式响应方向：Responses API SSE → Chat Completions delta（边接收边转发，无大包缓存）
  - 多轮 Function Calling：维护 `tool_call_id`/`index` 上下文，分段参数拼接防非法 JSON 分片，多工具交错 index 不串号
  - 连接稳定性：SSE 空闲心跳保活（`: keepalive` 注释分片）、四层超时防线（连接/读取/会话最大时长/分片空闲）、上游异常断连时优雅推送 `finish_reason` + `[DONE]`
  - 部署：`config/responses_relay.example.toml` 配置模板 + `README_RESPONSES_RELAY.md` 部署说明，全部超时/心跳参数可调
  - 测试：17 个单元测试，重点覆盖连续多轮工具调用 Agent 场景
- 新增 `clap` 依赖及 `responses_relay` 二进制入口

## [5.2.11] - 2026-08-05

### 修复
- **Codex 配置**：`model_catalog_json` 改为**可选**（默认关闭），避免写入后导致 Codex CLI 初始化失败；关闭时会自动移除配置中的残留键
- **空流响应**：上游返回空 body / 非 SSE 格式时，改为发送明确的 `response.failed`，不再发送 `id` 为空、`output` 为空的静默 `response.completed`；`response_id` 预生成 UUID，所有事件均带有效 ID
- **配额误计**：仅 2xx 响应计入配额调用次数（此前 4xx 错误会灌满配额窗口，导致 Key 被过滤、对话无故中断）
- **对话中断**：流转换器首块超时 15s→30s、块间空闲超时 30s→120s，避免推理模型思考间隙过长导致流被截断；`[DONE]` 分支增加防重复终止事件守卫

### 兼容性增强
- 请求翻译：`reasoning`（Responses API）→ `reasoning_effort`；`tool_choice` 格式转换；`text.format` → `response_format`；清理 `include`/`truncation` 等 Responses 独有参数
- 非流式响应：补充 `status`（completed/failed）、`created` → `created_at`、保证 `id` 非空
- thinking 标记识别扩展：支持 `>think` / `>thinking` / `>reasoning`、`<think>` / `<thinking>` / `<reasoning>`（Qwen3 / GLM / Kimi）、`[think]` / `[thinking]` / `[reasoning]`；标签式块以闭合标签为准（`</think>` 等），前缀式块以 `\n\n` 为准
- 推理字段名扩展：`reasoning_content` / `reasoning` / `thinking` / `thinking_content` / `reasoning_text` / `thought` / `thoughts`
- 模型改写（未知模型 → 默认模型）仅对 Responses API（Codex）请求生效，直连 Chat Completions 请求对未知模型透传

### 界面
- 「Codex CLI 集成 → 高级选项」新增「生成模型目录（仅 Codex Desktop）」开关，配置预览同步显示

## [5.2.10] - 2026-07

### 新增
- 持久化按平台 Token 统计（重启不丢失）
- Codex 模型改写（未知模型名 → 平台默认模型）

## [5.2.9] - 2026-07

### 修复
- Codex 配置合并而非覆盖，保留用户其他设置
- 上游请求调试日志
- 支持多个推理字段名与顶层 chunk 推理内容

## [5.2.3] - 2026-06

### 修复
- Codex CLI 集成优化，修复 7 项问题
- 移除转发请求中的 `content-length` 头，修复空响应 bug
- 单 Key 退避策略，修复 Codex CLI 的 `proxy_host`

## [5.2.2] - 2026-05

### 修复
- Codex 应用改为全新生成（不合并），并使用标准化的 `custom` provider 名称
- Codex 应用按钮静默无效问题：增加加载状态、Toast 反馈与配置空值检查

## [5.2.0] - 2026-05

### 新增
- 上游模型同步：多 URL 回退 + 自动创建
- 模型测试功能
- 可配置上游代理（upstream proxy）
- 可配置上下文窗口大小（`max_input_tokens`）
- 处理非标准 SSE 格式，流翻译增加超时

---

[//]: # (历史版本（v5.2.0 之前）请查阅 git log。)
