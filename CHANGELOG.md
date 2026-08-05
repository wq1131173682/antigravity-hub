# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 的格式约定。版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

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
