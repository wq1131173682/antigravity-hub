# Antigravity Hub - Context Guide

## Project Overview

**Antigravity Hub** 是一个轻量级的本地 API Key 轮询代理工具，基于 [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) v4.2.1 fork 简化而来。

核心功能：将多个 API Key 注入，自动做负载均衡和故障切换，对外暴露统一的 OpenAI 兼容接口（本地代理服务）。

本项目是原项目的精简版本，主要聚焦于 **API Key 轮询代理** 场景，重点增强了系统托盘交互体验。

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **前端框架** | React 19 + TypeScript |
| **UI 组件库** | Ant Design 5 + DaisyUI + Tailwind CSS 3 |
| **状态管理** | Zustand 5 |
| **路由** | React Router v7 |
| **国际化** | i18next + react-i18next (12 种语言) |
| **图表** | Recharts |
| **拖拽** | @dnd-kit |
| **动画** | Framer Motion |
| **后端引擎** | Rust + Tauri v2 |
| **后端框架** | Axum 0.7 |
| **数据库** | SQLite (Tauri 内置) |
| **HTTP 客户端** | Reqwest 0.12 |
| **构建工具** | Vite 7 |

## Directory Structure

```
Antigravity-Manager-main/
├── src/                          # Frontend React app
│   ├── App.tsx                   # Root component with routing
│   ├── i18n.ts                   # i18n configuration
│   ├── main.tsx                  # Entry point
│   ├── components/               # Reusable UI components
│   │   ├── layout/               # Layout wrappers (Header, Sidebar, etc.)
│   │   └── common/               # Shared components (ThemeManager, etc.)
│   ├── pages/                    # Page-level components
│   │   ├── Dashboard.tsx         # Main dashboard (stats, charts)
│   │   ├── Accounts.tsx          # Account/Key management
│   │   └── Settings.tsx          # App settings
│   ├── stores/                   # Zustand stores
│   ├── services/                 # API service layer
│   ├── hooks/                    # Custom React hooks
│   ├── utils/                    # Utility functions
│   ├── types/                    # TypeScript type definitions
│   ├── config/                   # Configuration modules
│   ├── locales/                  # i18n translation files
│   └── assets/                   # Static assets
├── src-tauri/                    # Rust backend (Tauri)
│   ├── src/
│   │   ├── main.rs               # Tauri entry point
│   │   ├── lib.rs                # App setup, tray menu, handlers
│   │   ├── constants.rs          # Global constants
│   │   ├── error.rs              # Error types
│   │   ├── commands/             # Tauri command handlers (Rust <-> JS bridge)
│   │   ├── modules/              # Core business logic
│   │   │   ├── proxy.rs          # Local proxy server (Axum)
│   │   │   ├── keystore.rs       # API Key storage & rotation
│   │   │   ├── quota_window.rs   # Sliding window quota tracking
│   │   │   ├── token_stats.rs    # Token I/O counter aggregation
│   │   │   ├── scheduler.rs      # Background cleanup scheduler
│   │   │   ├── logger.rs         # Structured logging
│   │   │   ├── log_bridge.rs     # Log bridge to debug console
│   │   │   ├── config.rs         # App config management
│   │   │   ├── i18n.rs           # Tray menu i18n
│   │   │   ├── platform_manager.rs
│   │   │   ├── model_manager.rs
│   │   │   └── key_model_map.rs
│   │   └── models/               # Data models
│   │   └── utils/                # Rust utility functions
│   ├── Cargo.toml                # Rust dependencies
│   ├── tauri.conf.json           # Tauri app config
│   └── icons/                    # App icons
├── public/                       # Static frontend assets
├── docs/                         # Documentation
├── scripts/                      # Build/utility scripts
├── deploy/                       # Deployment configs
├── docker/                       # Docker configurations
├── .github/workflows/            # CI/CD workflows
├── vite.config.ts                # Vite config (port 1420, proxy /api/ -> 8045)
├── tailwind.config.js            # Tailwind + DaisyUI theme config
└── package.json                  # Node.js dependencies
```

## Building and Running

### Prerequisites

- **Node.js** >= 18
- **Rust** >= 1.75 (当前使用 1.96.0)
- **Tauri v2 依赖**: 参考 https://v2.tauri.app/start/prerequisites/
- **Windows**: VS 2022 Build Tools + VC++ Workload

### Commands

| Command | Description |
|---------|-------------|
| `npm install` | 安装 Node.js 依赖 |
| `npm run dev` | 仅启动前端 Vite 开发服务器 (http://localhost:1420) |
| `npm run tauri dev` | **完整开发模式**：Tauri 窗口 + 前端热重载 |
| `npm run tauri:debug` | 带调试日志的 Tauri 开发模式 (`RUST_LOG=debug`) |
| `npm run build` | 构建前端生产包 |
| `npm run tauri build` | 构建桌面应用安装包 (MSI/NSIS) |
| `npm run preview` | 预览前端生产构建 |

### 端口说明

- **前端开发服务器**: `1420`
- **HMR WebSocket**: `1421`
- **代理服务端口**: `8045` (默认，可在设置中修改)

## Development Conventions

### Frontend
- **文件命名**: PascalCase 用于组件文件 (e.g., `Accounts.tsx`)，camelCase 用于工具函数
- **状态管理**: 使用 Zustand stores，store 文件位于 `src/stores/`
- **国际化**: 所有用户可见文本必须走 i18n，翻译文件在 `src/locales/`
- **RTL 支持**: 阿拉伯语 (ar) 自动切换 `dir="rtl"`
- **类型安全**: TypeScript strict mode，启用 `noUnusedLocals` 和 `noUnusedParameters`
- **CSS**: Tailwind CSS + DaisyUI 组件库，支持 light/dark 主题切换

### Backend (Rust)
- **日志**: 使用 tracing + tracing-subscriber，支持 env-filter
- **错误处理**: thiserror + anyhow 组合
- **并发**: tokio async runtime
- **Tauri 命令**: 所有 Rust <-> JS 通信通过 `tauri::generate_handler![]` 注册
- **模块化**: 业务逻辑按功能拆分到 `modules/` 目录

### Key Modules
- **proxy.rs**: 本地代理服务，转发请求到后端 API Key，兼容 OpenAI API 格式
- **keystore.rs**: API Key 的增删改查、启用/禁用、状态管理
- **quota_window.rs**: 滑动窗口额度追踪 (per model+key)，防止超额使用
- **token_stats.rs**: Token 输入/输出量统计聚合
- **scheduler.rs**: 后台定时任务 (清理过期/禁用的 Key 等)

## Architecture Notes

### Tauri v2 Pattern
- 前端通过 `@tauri-apps/api` 调用后端命令
- 后端命令在 `src-tauri/src/commands/` 中实现
- 使用 `app.emit()` 进行事件通知 (如托盘菜单事件)
- 关闭窗口时最小化到托盘，而非真正退出

### Proxy Flow
```
Client -> Local Proxy (8045) -> Key Rotation -> Remote API -> Response
```
- 前端 Vite dev server 将 `/api/` 代理到 `127.0.0.1:8045`
- 代理层负责 Key 轮询、429/500 错误自动切换、额度追踪

### Data Persistence
- 配置和账户数据存储在 SQLite (Tauri 内置)
- 数据目录可通过 `dirs` crate 获取跨平台路径

## i18n

支持 12 种语言，系统托盘菜单和前端界面均接入国际化。语言检测优先级：
1. 用户设置中的语言选项
2. 系统环境变量 (Windows: `VSLang`/`LANG`, macOS/Linux: `LANG`)

## License

[CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/)
