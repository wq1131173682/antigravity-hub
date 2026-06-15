<div align="center">

# Antigravity Hub

**基于 [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) 的增强版 AI 账号管理与协议代理桌面应用**

<img src="public/icon.png" alt="Antigravity Hub Logo" width="120" height="120" style="border-radius: 24px; box-shadow: 0 10px 30px rgba(0,0,0,0.15);">

<br/>

<a href="https://github.com/wq1131173682/antigravity-hub">
  <img src="https://img.shields.io/badge/Version-4.2.1--enhanced-blue?style=flat-square" alt="Version">
</a>
<img src="https://img.shields.io/badge/Tauri-v2-orange?style=flat-square" alt="Tauri">
<img src="https://img.shields.io/badge/Backend-Rust-red?style=flat-square" alt="Rust">
<img src="https://img.shields.io/badge/Frontend-React-61DAFB?style=flat-square" alt="React">
<img src="https://img.shields.io/badge/License-CC--BY--NC--SA--4.0-lightgrey?style=flat-square" alt="License">

<p>
  <a href="#-关于本项目">关于本项目</a> •
  <a href="#-改进内容">改进内容</a> •
  <a href="#-快速开始">快速开始</a> •
  <a href="#-技术架构">技术架构</a> •
  <a href="#-致谢">致谢</a>
</p>

</div>

---

## 📖 关于本项目

本项目 fork 自 [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager)（v4.2.1），在保留原版全部功能的基础上，针对 **系统托盘交互体验** 进行了深度优化和增强。

**Antigravity Hub** 是一个专为开发者和 AI 爱好者设计的全功能桌面应用，集成了：

- 🎛️ **智能账号管理** — 多账号统一调度，OAuth 2.0 授权，自动切换最佳账号
- 🔌 **API 协议转换** — 将 Web Session 转化为标准 OpenAI 兼容 API
- 📊 **实时配额监控** — 仪表盘式展示所有账号的健康状况与剩余配额
- 🌐 **本地代理网关** — 高性能反向代理，支持多平台多模型智能路由
- 🌍 **多语言支持** — 12 种语言界面，满足全球用户需求

## ✨ 改进内容

相较于原版 [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager)，本项目做了以下增强：

### 1. 🖱️ 系统托盘菜单全面优化

**原版问题：**
- 菜单项仅有 2 个（显示窗口 / 退出），功能过于简陋
- 菜单文本硬编码为英文，未接入 i18n 国际化系统
- 缺少账号快捷操作入口

**优化后：**

```
┌─────────────────────────────┐
│  显示主窗口                   │
├─────────────────────────────┤
│  切换下一个账号               │
│  刷新当前账号额度             │
├─────────────────────────────┤
│  退出应用 (Exit)              │
└─────────────────────────────┘
```

- ✅ **完整的 i18n 支持** — 菜单文本自动匹配系统语言（支持 12 种语言）
- ✅ **新增「切换账号」** — 一键触发账号切换，通过事件系统通知前端
- ✅ **新增「刷新额度」** — 从托盘直接刷新当前账号配额信息
- ✅ **分隔线分组** — 使用 `PredefinedMenuItem::separator` 逻辑分组，视觉更清晰
- ✅ **左键单击显示窗口** — 添加 `on_tray_icon_event` 处理，左键单击托盘图标即可显示/聚焦窗口
- ✅ **Tooltip 提示** — 鼠标悬停时显示 "Antigravity Hub" 提示文字
- ✅ **模板图标模式** — 设置 `icon_as_template(true)`，macOS 下自动适配深色/浅色菜单栏

### 2. 🌐 i18n 模块语言检测增强

**原版问题：**
- `i18n.rs` 仅支持 3 种语言检测（`en`、`tr`、默认 `zh`），其余 9 种语言文件被忽略

**优化后：**
- 完整支持全部 12 种语言的自动检测与匹配
- 使用 `starts_with` 前缀匹配，兼容各种 locale 格式（如 `en-US`、`zh-TW`、`zh_Hant`）
- 新增支持：日语、韩语、俄语、西班牙语、葡萄牙语、越南语、阿拉伯语、马来语、繁体中文

### 3. 📡 事件系统集成

托盘菜单操作通过 Tauri 事件系统（`app.emit`）与前端解耦：

| 事件名 | 触发时机 | 用途 |
| :--- | :--- | :--- |
| `tray:switch_next` | 点击「切换下一个账号」 | 通知前端执行账号切换逻辑 |
| `tray:refresh_quota` | 点击「刷新当前账号额度」 | 通知前端刷新配额数据 |

前端可通过 `listen("tray:switch_next", ...)` 监听这些事件并做出响应。

## 🚀 快速开始

### 环境要求

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://www.rust-lang.org/tools/install) >= 1.75
- [Tauri CLI Prerequisites](https://v2.tauri.app/start/prerequisites/)

### 安装与运行

```bash
# 克隆仓库
git clone git@github.com:wq1131173682/antigravity-hub.git
cd antigravity-hub

# 安装前端依赖
npm install

# 开发模式运行
npm run tauri dev

# 构建生产版本
npm run tauri build
```

## 🏗️ 技术架构

```
┌──────────────────────────────────────────┐
│              Frontend (React)             │
│   React 19 · Ant Design · Tailwind CSS   │
│   Zustand · React Router · i18next       │
├──────────────────────────────────────────┤
│           Tauri v2 Bridge (IPC)          │
├──────────────────────────────────────────┤
│              Backend (Rust)               │
│   Axum Proxy · SQLite · Reqwest          │
│   System Tray · OAuth · Scheduler        │
└──────────────────────────────────────────┘
```

| 层级 | 技术栈 | 职责 |
| :--- | :--- | :--- |
| **前端** | React 19 + TypeScript + Ant Design | UI 渲染、交互、国际化 |
| **桥接层** | Tauri v2 IPC | 前后端通信、事件分发 |
| **后端** | Rust + Axum | 代理服务、账号调度、系统托盘 |
| **存储** | SQLite | 本地数据持久化 |
| **网络** | Reqwest (rustls) | API 请求、协议转换 |

## 📁 项目结构

```
├── src/                    # 前端源码
│   ├── components/         # React 组件
│   ├── locales/            # 12 种语言翻译文件
│   ├── stores/             # Zustand 状态管理
│   └── utils/              # 工具函数
├── src-tauri/              # Rust 后端源码
│   ├── src/
│   │   ├── lib.rs          # 应用入口 & 托盘菜单配置
│   │   ├── commands/       # Tauri 命令（IPC 接口）
│   │   └── modules/        # 核心模块（代理、配置、i18n 等）
│   ├── icons/              # 应用图标 & 托盘图标
│   ├── Cargo.toml          # Rust 依赖配置
│   └── tauri.conf.json     # Tauri 应用配置
├── public/                 # 静态资源
├── index.html              # HTML 入口
├── package.json            # Node.js 依赖配置
└── vite.config.ts          # Vite 构建配置
```

## 📝 变更日志

### v4.2.1-enhanced (2026-06-15)

- 🆕 系统托盘菜单新增「切换下一个账号」和「刷新当前账号额度」选项
- 🌐 托盘菜单完整接入 i18n 国际化，支持 12 种语言自动检测
- 🖱️ 新增托盘图标左键单击显示窗口功能
- 🎨 托盘菜单添加分隔线分组，提升视觉层次
- 💡 添加 Tooltip 提示和模板图标模式
- 🔧 i18n 模块扩展语言检测覆盖范围（3 → 12 种语言）
- 📡 托盘操作通过事件系统与前端解耦

## 🙏 致谢

本项目基于以下优秀开源项目：

| 项目 | 作者 | 说明 |
| :--- | :--- | :--- |
| [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) | [@lbjlaq](https://github.com/lbjlaq) | 原版项目，提供了完整的 AI 账号管理与代理系统 |
| [Tauri](https://github.com/tauri-apps/tauri) | Tauri Team | 跨平台桌面应用框架 |
| [Ant Design](https://github.com/ant-design/ant-design) | Ant Group | 企业级 UI 组件库 |
| [Axum](https://github.com/tokio-rs/axum) | Tokio Team | Rust 异步 Web 框架 |

## 📄 许可证

本项目继承原项目的 [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/) 许可证。

> 仅限非商业用途。如需商业授权，请联系原项目作者。

---

<div align="center">

**原项目**：[lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) · ⭐ 如果觉得不错，请给原项目一个 Star！

</div>
