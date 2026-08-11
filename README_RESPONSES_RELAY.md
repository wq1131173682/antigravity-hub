# Responses API 中转转发服务

> 将上游原生 **Responses API** 请求/流式响应，双向转换为标准 **OpenAI `/v1/chat/completions`** 格式，供给仅支持 Chat 协议的客户端使用。

本服务重点解决 **多轮 Function Calling / Agent 持续工具调用** 场景下的三大线上故障：

1. **流式中断** — 连续多次循环工具调用时分片乱序、tool_call 信息丢失，Agent 流程中途终止
2. **连接提前断开** — 长时间执行工具调用时缺少保活包，链路空闲超时被网关强制切断
3. **分片解析崩溃** — 空内容分片、分段 tool_call 参数拼接导致非法 JSON，客户端解析失败而卡死

---

## 一、架构总览

```
┌──────────────┐   /v1/chat/completions    ┌─────────────────────────┐
│  Chat 客户端  │ ────────────────────────► │   responses_relay 服务  │
│ (仅支持Chat)  │                           │                         │
│              │ ◄──────────────────────── │ ① 请求: Chat→Responses │
└──────────────┘    Chat delta SSE 流       │ ② 转发: 上游 Responses  │
                                           │ ③ 响应: Responses SSE→Chat│
                                           │ ④ 保活心跳 + 超时优雅关闭 │
                                           └──────────┬──────────────┘
                                                      │ /v1/responses (SSE)
                                                      ▼
                                              ┌──────────────────┐
                                              │ 上游 Responses    │
                                              │ API 服务          │
                                              └──────────────────┘
```

---

## 二、快速开始

### 1. 编译

```bash
cd src-tauri
cargo build --bin responses_relay
```

### 2. 配置

复制模板并填写上游信息：

```bash
cp config/responses_relay.example.toml config/responses_relay.toml
# 编辑 responses_relay.toml，填写 upstream_url 和 api_key
```

### 3. 启动

```bash
# 方式一：命令行参数
RESPONSES_RELAY_API_KEY=sk-xxx \
RESPONSES_RELAY_UPSTREAM_URL=https://api.openai.com \
cargo run --bin responses_relay

# 方式二：配置文件
RESPONSES_RELAY_CONFIG=config/responses_relay.toml \
cargo run --bin responses_relay

# 方式三：编译产物直接运行
./target/debug/responses_relay --port 8046 --upstream-url https://api.openai.com --api-key sk-xxx
```

### 4. 客户端接入

把客户端（支持 Chat Completions 的任意应用）的 base URL 指向本服务：

```
http://127.0.0.1:8046
```

客户端发送标准 `POST /v1/chat/completions` 请求即可，无需任何改动。

---

## 三、端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | **主入口**：Chat→Responses 双向转换，流式/非流式均支持 |
| `/v1/responses` | POST | 透传原生 Responses API（不转换），供调试/原生客户端 |
| `/v1/models` | GET | 返回可用模型列表 |
| `/health` | GET | 健康检查，返回运行状态与生效配置 |

---

## 四、可调参数

> 这些参数是解决"长任务被切断"的关键，全部支持 **配置文件 / CLI / 环境变量** 三种方式设置。

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `host` | `127.0.0.1` | 监听地址 |
| `port` | `8046` | 监听端口 |
| `upstream_url` | `https://api.openai.com` | 上游 Responses API 基础 URL |
| `api_key` | *空* | 上游 API Key（可被请求体 `api_key` 字段覆盖） |
| `default_model` | `gpt-4o` | 请求未指定 model 时的默认模型 |
| `connect_timeout_secs` | `30` | 上游 **连接超时**（秒） |
| `read_timeout_secs` | `600` | 上游 **读取超时**（秒）——整体墙钟上限，长任务务必设大 |
| `session_max_duration_secs` | `600` | **整体会话最大时长**（秒）——超时优雅关闭 |
| `heartbeat_interval_secs` | `15` | **SSE 心跳间隔**（秒）——空闲保活，防网关断连 |
| `first_chunk_timeout_secs` | `120` | **首次分片超时**（秒） |
| `chunk_idle_timeout_secs` | `300` | **分片空闲超时**（秒） |

### 环境变量对照表

| 环境变量 | 对应参数 |
|---------|---------|
| `RESPONSES_RELAY_HOST` | `host` |
| `RESPONSES_RELAY_PORT` | `port` |
| `RESPONSES_RELAY_UPSTREAM_URL` | `upstream_url` |
| `RESPONSES_RELAY_API_KEY` | `api_key` |
| `RESPONSES_RELAY_MODEL` | `default_model` |
| `RESPONSES_RELAY_CONNECT_TIMEOUT` | `connect_timeout_secs` |
| `RESPONSES_RELAY_READ_TIMEOUT` | `read_timeout_secs` |
| `RESPONSES_RELAY_SESSION_MAX_DURATION` | `session_max_duration_secs` |
| `RESPONSES_RELAY_HEARTBEAT_INTERVAL` | `heartbeat_interval_secs` |
| `RESPONSES_RELAY_FIRST_CHUNK_TIMEOUT` | `first_chunk_timeout_secs` |
| `RESPONSES_RELAY_CHUNK_IDLE_TIMEOUT` | `chunk_idle_timeout_secs` |

### 长任务场景推荐配置

```toml
# 适用于：多轮工具调用 Agent、长推理、长时间思考
read_timeout_secs = 1800          # 30 分钟整体读取
session_max_duration_secs = 1800  # 30 分钟会话
heartbeat_interval_secs = 10      # 10 秒心跳，更激进地防断连
first_chunk_timeout_secs = 180    # 3 分钟等首片
chunk_idle_timeout_secs = 600     # 10 分钟空闲容忍
```

---

## 五、部署说明

### 反向代理（Nginx）

若需对外暴露，建议放在 Nginx 后面并开启 SSE 缓冲禁用：

```nginx
location / {
    proxy_pass http://127.0.0.1:8046;
    proxy_http_version 1.1;
    proxy_set_header Connection "";
    # 关键：禁用缓冲，保证 SSE 实时推送
    proxy_buffering off;
    proxy_cache off;
    chunked_transfer_encoding on;
    proxy_read_timeout 3600s;   # 对齐服务的 session_max_duration
    proxy_send_timeout 3600s;
}
```

### 安全建议

- 默认绑定 `127.0.0.1`，仅本机访问
- 如需对外，务必在请求入口增加鉴权（Nginx Basic Auth / 前端 token）
- 上游 API Key 通过环境变量或配置文件注入，切勿硬编码进代码

---

## 六、协议转换细节

### 请求方向：Chat Completions → Responses API

| Chat Completions 字段 | Responses API 字段 |
|----------------------|-------------------|
| `messages`（system 消息） | `instructions`（顶层字段） |
| `messages`（user/assistant 消息） | `input` 数组的 `type="message"` 项 |
| `messages`（assistant 的 `tool_calls`） | `input` 数组的 `type="function_call"` 项 |
| `messages`（role="tool"） | `input` 数组的 `type="function_call_output"` 项 |
| `messages[].content`（字符串） | `content[{"type":"input_text","text":...}]` |
| `messages[].content`（数组，含图片） | `content[{"type":"input_image",...}]` |
| `max_tokens` | `max_output_tokens` |
| `tools[].function.name/description/parameters` | `tools[].name/description/parameters`（反嵌套） |
| `tool_choice.function.name` | `tool_choice.name`（反嵌套） |
| `response_format` | `text.format` |

### 响应方向：Responses API SSE → Chat Completions delta

| Responses API SSE 事件 | Chat Completions delta |
|-----------------------|----------------------|
| `response.created` | 提取 `response.id` / `model` / `created_at` |
| `response.output_item.added`（message） | 记录文本输出项 |
| `response.output_text.delta` | `choices[0].delta.content` |
| `response.output_item.added`（function_call） | `choices[0].delta.tool_calls[].{id,type,function.name}` |
| `response.function_call_arguments.delta` | `choices[0].delta.tool_calls[].function.arguments` |
| `response.completed` | `finish_reason`（有工具调用→`tool_calls`，否则`stop`） |
| `response.failed` | 错误分片 + `finish_reason:"error"` |
| `data: [DONE]` | 流结束标记 |

---

## 七、关键实现说明

### 1. 多轮工具调用 index/id 上下文维护

流式转换器维护一个 `tool_calls: HashMap<output_index, ToolCallState>`，以 Responses API 的 `output_index` 为键，稳定映射到 Chat Completions 的 `tool_calls[].index`。每个工具调用的 `call_id`、`name`、累积的 `arguments_buffer` 都被独立跟踪，**不会因为多个工具调用交错出现而串号**。

### 2. 分段参数拼接（防非法 JSON 分片）

`function_call_arguments.delta` 可能被上游拆成多个片段（如 `{"city":` + ` "Beijing"}`），转换器逐片段累积到 `arguments_buffer`，再以完整的 delta 转发。客户端侧收到的每个参数 delta 都是连贯的，不会出现截断的非法 JSON。

### 3. SSE 分片重组

部分中继会把单个 JSON 事件拆成多行物理行（Agnes 等）。`parse_sse_line` 自动累积不完整片段，直到拼成合法 JSON 再解析，避免事件被静默丢弃导致工具参数丢失。

### 4. 心跳保活

`heartbeat_interval_secs` 定时器驱动：上游无数据时，向下游发送标准 SSE 注释行 `: keepalive\n\n`。这是合法的 SSE 事件（客户端忽略），但能有效防止反向代理/网关因连接空闲而切断 TCP。

### 5. 四层超时防线

- **连接超时** `connect_timeout_secs`：TCP/TLS 建连
- **读取超时** `read_timeout_secs`：reqwest 整体墙钟上限
- **会话最大时长** `session_max_duration_secs`：强制优雅关闭
- **分片空闲/首片超时**：防死流

任一层触发都会向客户端发送标准 `finish_reason` 分片 + `[DONE]`，**优雅关闭而非硬断**，前端不会卡死。

---

## 八、测试

单元测试覆盖了协议转换与多轮工具调用流式场景：

```bash
cd src-tauri
cargo test --bin responses_relay
# 或直接跑所有测试
cargo test responses_bridge
```

重点测试用例：

| 用例 | 验证内容 |
|------|---------|
| `test_chat_to_responses_tool_calls` | 请求方向：assistant tool_calls → function_call 项 |
| `test_chat_to_responses_multi_tool_rounds` | **连续多轮工具调用**请求转换，call_id 不串号 |
| `test_stream_simple_text` | 流式：文本 delta 正确下发，finish_reason=stop |
| `test_stream_tool_call` | 流式：单工具调用 id/name/参数完整，finish_reason=tool_calls |
| `test_stream_multi_round_tool_calls` | **流式：两个工具调用交错，index=0/1 不串号** |
| `test_stream_keepalive_heartbeat` | 心跳：无数据时发送 `: keepalive` |
| `test_stream_idle_timeout` | 超时：上游死流时强制完成 + `[DONE]` |
| `test_responses_to_chat_with_tool_calls` | 非流式：tool_calls 转换 |

---

## 九、文件结构

```
src-tauri/src/modules/responses_bridge.rs   # 核心双向转换逻辑（含测试）
src-tauri/src/bin/responses_relay.rs        # 独立可运行二进制服务
config/responses_relay.example.toml         # 配置文件模板
README_RESPONSES_RELAY.md                    # 本文档
```