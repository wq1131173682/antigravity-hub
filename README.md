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
- 本地 OpenAI 兼容代理（默认 `127.0.0.1:8045`），支持任意 OpenAI 兼容 SDK / 客户端接入
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
2. 启动本地代理（默认端口 `8045`）
3. 任意 OpenAI 兼容客户端指向 `http://127.0.0.1:8045/{平台前缀}/v1` 即可
4. （可选）Codex CLI 集成已在 v5.3.10 封存停用，如需启用请关注后续版本

> 完整的使用说明与各平台配置示例，见项目 Wiki（建设中）与「设置」页面的内嵌提示。

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
