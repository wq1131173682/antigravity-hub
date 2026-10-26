<div align="center">

# Antigravity Hub

**多平台 API Key 轮询代理工具 · 桌面应用（Tauri v2）**

基于 [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) 重构的轻量版本，面向 **多账号管理与 API Key 轮询代理** 场景。

<br/>

<img src="public/icon.png" alt="Logo" width="80" height="80" style="border-radius: 16px;">

<br/><br/>

<img src="https://img.shields.io/badge/Tauri-v2-orange?style=flat-square" alt="Tauri">
<img src="https://img.shields.io/badge/Rust-1.75%2B-blue?style=flat-square" alt="Rust">
<img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square" alt="React">
<img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform">
<img src="https://img.shields.io/badge/license-CC%20BY--NC--SA%204.0-green?style=flat-square" alt="License">

<br/>

[English](README_EN.md) · [更新日志](CHANGELOG.md) · [贡献指南](CONTRIBUTING.md) · [安全报告](SECURITY.md)

</div>

---

## 这是什么

一个本地运行的 **API Key 轮询代理工具**。把多个平台（Sensenova、Agnes、OpenAI 兼容网关等）的多个 API Key 交给它，它会在本地启动一个 **OpenAI 兼容代理服务**，自动完成：

- **负载均衡**：请求在可用 Key 之间均匀分发
- **故障切换**：429 / 5xx 自动轮换到下一个可用 Key（指数退避重试）
- **配额管理**：按「模型 × Key」维度跟踪 5 小时 / 天 / 月滑动窗口用量，超限自动切换
- **OpenAI 协议穿透 + Key 轮转**：OpenAI Chat Completions 请求原样转发上游，本地统一管理并按配额/故障智能轮换 API Key

核心就一件事：**统一入口，多 Key 轮询，按配额智能调度**。

## 功能特性

### 🚀 核心代理
- 本地 OpenAI 兼容代理（默认 `127.0.0.1:5343`），支持任意 OpenAI 兼容 SDK / 客户端接入
- 自动 Key 轮换与负载均衡，429/5xx 智能退避，单 Key 也支持
- ⚠️ **Codex 集成与 Responses API 协议转换已于 v5.3.10 封存**：代码保留、界面灰显禁用；代理以 OpenAI Chat Completions 穿透为主路径
- 兼容多种上游格式：多字段推理内容（`reasoning_content` / `thinking` 等）、内联 thinking 块（`>think` / `<think>` / `[think]` 等标记）、工具调用、流式与非流式

### 💳 多平台账号管理
- 任意数量的平台与账号，每个平台独立配置 `base_url` / `path_prefix` / API Key
- 按「模型 × Key」建立映射，精细控制哪个 Key 可以调用哪个模型
- 支持上游模型列表自动同步与自动创建、上下文窗口（`max_input_tokens`）配置

### 📊 配额与统计
- 5 小时 / 天 / 月 **滑动窗口配额跟踪**（按模型 × Key 维度）
- 429 / 500 错误计数与自动退避，到期自动恢复
- 全局 + 按平台的 **Token 用量统计**（持久化，重启不丢失）

### 🤖 Codex CLI 集成（已封存 · v5.3.10）
> 自 v5.3.10 起 Codex 集成与 Responses API 协议转换已**封存停用**（代码保留、设置页灰显禁用、不响应交互）。以下能力在当前版本暂不可用。
- 一键生成并应用 Codex `config.toml`（`model_providers.custom` + API Key 认证）
- 配置备份 / 一键恢复，清除残留 OAuth 数据
- 环境变量冲突检测（`OPENAI_API_KEY` / `OPENAI_BASE_URL` 等）
- 可选生成模型目录（`model_catalog_json`，仅 Codex Desktop 需要）

### 🎨 其他
- 12 种语言国际化（跟随系统语言）
- 系统托盘：快速切换账号、刷新额度
- 自动更新（Tauri updater）

## 快速开始

### 环境要求

- Node.js >= 18
- Rust >= 1.75
- Tauri v2 系统依赖（见 [Tauri 官方文档](https://v2.tauri.app/start/prerequisites/)）

### 开发运行

```bash
git clone git@github.com:wq1131173682/antigravity-hub.git
cd antigravity-hub
npm install

# 开发模式（带热更新）
npm run tauri dev

# 打包发布
npm run tauri build
```

### 使用步骤

1. 启动应用，在「账号管理」中添加平台与 API Key
2. 启动本地代理（默认端口 `5343`）
3. 任意 OpenAI 兼容客户端指向 `http://127.0.0.1:5343/v1`（统一入口），或 `http://127.0.0.1:5343/{平台前缀}/v1`（按平台前缀）
4. （可选）Codex CLI 集成已在 v5.3.10 封存停用，如需启用请关注后续版本

> 完整的使用说明与各平台配置示例，见项目 Wiki（建设中）与「设置」页面的内嵌提示。

## 统一入口：一个 URL 访问所有平台的模型

代理提供两种等价的寻址方式：

| 方式 | 请求地址 | 平台如何确定 | 模型 id 写法 |
|---|---|---|---|
| **统一入口**（推荐） | `http://host:5343/v1/...` | 由模型 id 的命名空间决定 | `平台前缀:模型名` |
| 平台前缀（兼容旧用法） | `http://host:5343/{平台前缀}/v1/...` | 由路径第一段决定 | 裸模型名，或同名空间 |

`GET /v1/models` 返回**所有平台**的模型，每条 id 都带平台命名空间：

```json
{
  "object": "list",
  "data": [
    {
      "id": "sensenova:deepseek-v4-flash",
      "object": "model",
      "created": 1750000000,
      "owned_by": "sensenova",
      "model": "deepseek-v4-flash",
      "max_input_tokens": 1000000
    }
  ]
}
```

- `id` 是客户端**必须发送**的模型标识，格式 `平台前缀:模型名`。用冒号而不是连字符，因为模型名本身常含连字符（`deepseek-v4-flash`），用 `-` 无法无歧义拆分。
- `model` 是剥离命名空间后的裸模型名，`owned_by` 是平台前缀（早期版本这里是不可用的平台 UUID）。
- 同一模型名出现在多个平台时，两个平台各有独立 id，互不冲突。

调用对话接口时，把上面拿到的 `id` 原样放进 `model` 字段即可：

```bash
curl http://127.0.0.1:5343/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"sensenova:deepseek-v4-flash","messages":[{"role":"user","content":"hi"}]}'
```

代理会按命名空间选平台、注入轮转后的 Key，并把转发给上游的 `model` 改回裸名（上游不认识命名空间）。

**裸模型名也接受**，但仅当它只属于一个平台时；若多个平台都有同名模型，请求返回 `400` 并列出可用的具体 id，而不是猜一个。

错误语义：

| 情况 | 状态码 |
|---|---|
| 模型不属于任何平台 / 平台前缀未知 | `404`，错误正文指向 `GET /v1/models` |
| 裸模型名在多个平台重名（需加命名空间） | `400`，正文列出 `a:model` 或 `b:model` |
| 带前缀路径与 `model` 的命名空间互相矛盾 | `400`，正文指出二者 |
| 全池 Key 都在 429 冷却 | `503` + `Retry-After`（OpenAI 兼容错误体） |
| 真实传输 / 上游故障 | `502` |

> **路径前缀限制**：平台前缀不能是 `v1`、`v2`、`v1beta` 这类版本段（那是统一入口的根），也不能含 `/`、`:` 或空白——保存时会校验并拒绝。前缀同时用作模型 id 的命名空间，改动会改变客户端看到的 id。

### 兼容性

统一入口是新增的，`/{平台前缀}/v1/...` 与 `/{平台前缀}/v1/models` 行为不变。已有客户端无需修改，除非要改用统一入口。

## 技术栈

| 层 | 技术 |
|---|---|
| 前端 | React 19 · TypeScript · Ant Design · Tailwind CSS · daisyUI · Zustand · i18next |
| 后端 | Rust · Tauri v2 · Axum（本地代理） |
| 存储 | JSON 文件（`api_keys.json` / `quota_windows.json` / `token_stats.json` / `config.json` 等，位于系统数据目录） |
| 代理协议 | OpenAI Chat Completions（穿透） / Responses API（v5.3.10 起封存） |

## 项目结构

```
├── src/                  # 前端（React 19 + TypeScript）
│   ├── components/       # UI 组件（账号 / 仪表盘 / 设置 / 布局）
│   ├── pages/            # 顶层页面（Accounts / Dashboard / Settings）
│   ├── services/         # 后端 IPC 调用封装
│   ├── stores/           # Zustand 状态（账号 / 配置 / 平台）
│   ├── locales/          # 12 种语言
│   └── utils/            # 工具函数
├── src-tauri/            # 后端（Rust + Tauri v2）
│   ├── src/commands/     # Tauri IPC 命令
│   ├── src/modules/      # 核心逻辑（proxy / codex / quota / keystore）
│   └── src/models/       # 数据模型
└── .github/workflows/    # CI / 发布流水线
```

## 更新日志

<!-- 注意：版本条目必须保持「- **vX.Y.Z** 标题 + 缩进子项」格式，release.yml 依赖该格式提取发布说明 -->

- **v5.3.42** 统一入口：一个 URL 访问所有平台的模型
  - 新增统一入口 `/v1/...`：不带平台前缀即可访问所有平台，平台由请求体 `model` 的命名空间（`平台前缀:模型名`）决定；`GET /v1/models` 前缀无关地返回全部平台的模型
  - 模型 id 统一改为 `平台前缀:模型名`（如 `sensenova:deepseek-v4-flash`），`model` 字段保留裸名、`owned_by` 改为平台前缀（原为不可用的平台 UUID）
  - 完全兼容旧的 `/{平台前缀}/v1/...` 与 `/{平台前缀}/v1/models`；带前缀路径也接受命名空间 id（须与该前缀一致，矛盾时 400 而非静默改道）
  - 裸模型名仍可用，但仅在单一平台拥有该模型时；多平台重名返回 400 并列出可用的具体 id
  - 新增平台前缀校验：拒绝 `v1`/`v2`/`v1beta` 等版本段（统一入口保留字）与含 `/`、`:`、空白的值；前端输入框与后端校验同步
  - 修复：增删改平台 / 模型、从上游导入或刷新模型后，`/v1/models` 缓存不失效导致最长 60s 返回旧列表
  - 修复：单个代理请求最多读取 3 次 `gui_config.json`（平台查找、命名空间解析、base_url 覆盖）→ 收敛为请求开始时读一次
  - 仪表盘显示统一入口地址与每个模型的命名空间 id；新增 23 个后端测试（含真实 HTTP 栈与真实监听端口的端到端验证）
  - 默认端口统一为 5343（原 8045），涉及 Rust 默认值、前端设置页与 store；已装用户沿用持久化值
  - 清理：移除 `vite.config.ts` 中指向旧端口的 `/api/` 死代理（前端从不请求 `/api/`，全走 Tauri IPC）
- **v5.3.41** 限流语义修正与重试预算增强
  - 修复：全池 429 冷却时错误报 `502 No active API keys available`（既非「无 key」也非代理故障）→ 改为 **503 + `Retry-After`** + OpenAI 兼容 JSON 错误体；502 仅保留给真实传输/上游故障
  - 重试预算由固定 60s 改为可续期（覆盖已知冷却到期时间，上限 180s，锚定请求起点）
  - 修复：单次冷却等待被 `clamp(1,30)` 截断可能提前醒来白耗一次尝试 → 等满冷却 + 2s 宽限
  - 修复：配额超限被误述为「无可用 key」→ 独立文案并给出候选数量
  - 修复：默认端口 8080 与文档/开发代理/UI 提示的 8045 不一致 → 统一为 8045（已装用户沿用持久化值）
  - 修复：`proxy.rs` / `keystore.rs` 注释 GBK↔UTF-8 双重误码（`—`、`─` 变乱码）
  - 清理：`utils/request.ts` 移除无后端的「Web 模式」死分支
- **v5.3.10** 重点版本 · 稳定性集中修复（⭐ milestone）
  - 封存 Codex 集成与 Responses API 协议转换（`CODEX_ENABLED=false`，代码保留、界面灰显禁用）
  - OpenAI 协议直接穿透，API Key 本地轮转（429/5xx 自动切换下一把 Key）
  - 修复：代理下对话/工具调用中断（SSE 流结束 `Connection: close`）、半帧静默中断（保活仅事件边界注入）、平台 key 429 限流中断（冷却期退避重试）
- **v5.2.11** 兼容性增强与多项修复
  - 兼容性：Responses API 翻译全面兼容不同平台/模型（`reasoning` → `reasoning_effort`、`tool_choice` 格式转换、thinking 标记 `>think`/`<think>`/`[think]` 识别、多字段推理名）
  - 修复：空上游流改为明确报错而非静默空完成；仅 2xx 计入配额（修复 4xx 灌满配额导致对话中断）；模型改写仅限 Responses API
  - 修复：Codex `model_catalog_json` 改为可选（默认关闭，避免 CLI 初始化失败）
- **v5.2.10** Codex 模型改写 + 持久化按平台 Token 统计
- **v5.2.9** Codex 配置合并、上游请求调试日志、多推理字段与顶层 chunk 推理支持
- **v5.2.3** Codex CLI 集成优化（修复 7 项问题）
- **v5.2.0** 上游模型同步多 URL 回退与自动创建、模型测试
- 完整历史见 [CHANGELOG.md](CHANGELOG.md)

## 致谢

感谢 [lbjlaq](https://github.com/lbjlaq) 开发的 [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager)，本项目在其基础上做了针对性重构与精简。如需要完整功能，建议同时参考原版项目。

## 许可证

本项目基于原项目 **CC BY-NC-SA 4.0**（[Creative Commons Attribution-NonCommercial-ShareAlike 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/)）。

> ⚠️ **注意**：该许可证为**非商用**许可，且要求以相同方式共享（ShareAlike）。商用前请务必确认符合许可条款或获得授权。
