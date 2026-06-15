<div align="center">

# Antigravity Hub

**API Key 轮询代理工具**

基于 [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) 简化改造

<br/>

<img src="public/icon.png" alt="Logo" width="80" height="80" style="border-radius: 16px;">

<br/><br/>

<img src="https://img.shields.io/badge/Tauri-v2-orange?style=flat-square" alt="Tauri">
<img src="https://img.shields.io/badge/Rust-blue?style=flat-square" alt="Rust">
<img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square" alt="React">

</div>

---

## 这是什么

一个轻量的本地 API Key 轮询代理工具。把多个 API Key 扔进去，它会自动帮你做负载均衡和故障切换，对外暴露一个统一的 OpenAI 兼容接口。

核心就一件事：**多 Key 轮询，统一入口**。

## 基于什么

本项目 fork 自 [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) v4.2.1，原项目是一个功能非常完善的 AI 账号管理与协议代理系统，功能丰富、设计精良，推荐大家去使用原版。

本项目在此基础上做了精简和调整，主要面向 **API Key 轮询** 这个使用场景。

## 改了什么

相对于原版，主要改动集中在系统托盘部分：

**托盘菜单增强**
- 原版只有「显示窗口」和「退出」两项，现在增加了「切换下一个账号」和「刷新额度」
- 菜单文本接入了 i18n，跟随系统语言显示（原版硬编码英文）
- 左键点击托盘图标可以显示窗口
- 菜单项加了分隔线，分组更清晰

**i18n 小修**
- 原版的托盘语言检测只覆盖了 3 种语言，补全到全部 12 种

改动不大，主要是让日常使用更方便一些。

## 快速开始

```bash
# 环境要求
# - Node.js >= 18
# - Rust >= 1.75
# - Tauri v2 依赖 (https://v2.tauri.app/start/prerequisites/)

git clone git@github.com:wq1131173682/antigravity-hub.git
cd antigravity-hub
npm install

# 开发
npm run tauri dev

# 构建
npm run tauri build
```

## 技术栈

- **前端**: React 19 + TypeScript + Ant Design + Tailwind CSS
- **后端**: Rust + Tauri v2 + Axum
- **存储**: SQLite
- **代理**: 本地 Axum 服务，兼容 OpenAI API 格式

## 致谢

感谢 [lbjlaq](https://github.com/lbjlaq) 开发的 [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager)，原项目做得非常好，本项目只是在其基础上做了一些针对性的简化。如果需要完整的功能，强烈建议直接使用原版。

## 许可证

继承原项目 [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/)。
