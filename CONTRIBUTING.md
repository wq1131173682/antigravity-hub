# 贡献指南

感谢你愿意为 Antigravity Hub 贡献力量！无论是修复 Bug、新增功能、完善文档还是改进 UI，都欢迎。

## 开发环境

- Node.js >= 18
- Rust >= 1.75
- Tauri v2 系统依赖（[官方文档](https://v2.tauri.app/start/prerequisites/)）

```bash
npm install
npm run tauri dev     # 开发模式
npm run tauri build   # 打包发布
```

## 开发命令速查

| 命令 | 说明 |
|---|---|
| `npm run dev` | 前端 Vite 开发服务器（端口 1420） |
| `npm run build` | 前端类型检查 + 构建 |
| `npm run tauri dev` | 完整 Tauri 开发模式 |
| `npx tsc --noEmit` | 前端类型检查 |
| `cd src-tauri && cargo check` | Rust 编译检查 |
| `cd src-tauri && cargo clippy -- -D warnings` | Rust 严格 lint |
| `cd src-tauri && cargo fmt -- --check` | Rust 格式检查 |

## 提交规范

遵循 [Conventional Commits](https://www.conventionalcommits.org/)：

- 格式：`type(scope): description`（主题行不超过 72 字符）
- 常用 type：`feat`、`fix`、`refactor`、`chore`、`docs`、`style`、`i18n`
- 示例：
  - `feat(proxy): support model-level key mapping`
  - `fix(codex): handle empty upstream stream gracefully`
  - `docs: update README quick start`

> 注意：前端使用 2 空格缩进、单引号、PascalCase 组件 / camelCase 文件；Rust 遵循 `cargo fmt` 与 `cargo clippy -D warnings`。

## 开发流程

1. **Fork** 本仓库并创建特性分支：`git checkout -b feat/your-feature`
2. **小步提交**，每个提交只做一件事，信息清晰
3. **保持 CI 通过**：提交前本地跑 `npx tsc --noEmit`、`cargo check`、`cargo clippy -- -D warnings`
4. **发起 Pull Request**，描述改动内容与动机，UI 改动请附截图

## 测试

项目目前没有自动化测试套件。添加测试时：
- 前端使用 Vitest（与 Vite 工具链匹配），测试文件与源码同目录（`*.test.ts(x)`）
- Rust 使用各模块内的 `#[cfg(test)]` 测试模块

## 行为准则

- 友善、尊重、建设性
- 讨论围绕代码与问题本身
- 不接受任何形式的骚扰或歧视

## 问题与讨论

- Bug 与功能请求请使用 [GitHub Issues](https://github.com/wq1131173682/antigravity-hub/issues)（参考模板）
- 安全问题请通过 [SECURITY.md](SECURITY.md) 中的方式私下报告
