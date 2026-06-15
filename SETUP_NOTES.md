# 项目环境 & 依赖

## 已安装工具

| 工具 | 安装方式 | 用途 |
|------|---------|------|
| Rust 工具链 (rustc 1.96.0) | [rustup](https://rustup.rs/) | Tauri 后端编译 |
| VS 2022 Build Tools + VC++ workload | `winget install Microsoft.VisualStudio.2022.BuildTools` + 引导程序添加 VC++ | Tauri 原生模块编译 |
| Node.js 依赖 | `npm install`（项目已配置 package.json） | 前端构建 |

## 开发命令

```bash
# 前端热重载开发（浏览器，不含 Tauri API）
npm run dev

# Tauri 原生窗口开发模式（完整功能 + 热重载）
cd src-tauri && cargo tauri dev

# 构建安装包（MSI / NSIS）
cd src-tauri && cargo tauri build
```

## Rust 路径

- `cargo.exe` 位置：`C:\Users\11311\.cargo\bin\cargo.exe`
- 如果终端找不到 cargo，需将 `%USERPROFILE%\.cargo\bin` 加入 PATH
