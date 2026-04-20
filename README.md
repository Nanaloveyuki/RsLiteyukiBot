<div align="center">

[//]: # (<img  src="https://cdn.liteyuki.org/logos/bot.svg" style="align-content: center; width: 50%; margin-top:10%;" alt="a">)
![][banner]
<h2> <span style="color: #a2d8f4">轻雪</span> <span style="color: #d0e9ff">6 In Rust</span></h2>
<h4> <span style="color: #a2d8f4">✨ 轻量，高效，易于扩展✨</span></h4>

![][Liteyuki6.0]

</div>

## 关于

本项目是由 [LiteyukiBot-6](https://github.com/LiteyukiStudio/LiteyukiBot) 延申而来的 Rust 重构项目, 在保留原有轻量化特点的同时又迎合 AI 时代浪潮.

## 特点

- Rust 开发: 高效运行
- 多线程处理: 避免*一核有难, 七核围观*
- Websocket & SSE & TCP-Http三段兼容: 总有一个接口适合
- TUI & Docker & Tauri2-GUI: 想要什么风格都可以

## Todo

- [ ] Docker 适配
- [ ] Tauri2-GUI 适配
- [ ] 插件系统

## 快速开始

### 环境要求

- Rust 工具链（需支持 `edition = "2024"`）
- 建议使用最新稳定版 `cargo`

### 启动

目前仅支持 dev 开发模式, 暂时不发布 release
```bash
cargo run
```

首次启动会检测 `config.yaml` 并自动加载

## 配置说明

### 主配置文件

配置加载顺序如下：

1. 环境变量 `LY_CONFIG_PATH` 指定路径
2. `config.yaml`
3. `rust-config.yaml`
4. `rust-config.yml`
5. `rust-config.toml`
6. `config/rust-core.yaml`
7. `config/rust-core.toml`

### LLM 配置覆盖

- 建议将密钥放入独立 `llm-config.yaml` / `llm-config.toml`
- 可通过 `LY_LLM_CONFIG_PATH` 指定 LLM 配置文件
- 启动时会把独立文件中的 `llm` 段覆盖到主配置

### connect 三段配置

- `connect.websocket`：支持 `forward` / `reverse` / `both`
- `connect.tcp-http`：TCP-HTTP 通道
- `connect.sse`：SSE 通道
- `urls` 支持多端点，会自动展开为多个适配器实例

## 鸣谢

[@Snowykami](https://sfkm.me): Liteyuki 文档站支持和授权


[Liteyuki6.0]: https://img.shields.io/badge/Liteyuki-6.0-blue?style=for-the-badge

[banner]: https://socialify.git.ci/Nanaloveyuki/RsLiteyukiBot/image?description=1&font=Source+Code+Pro&forks=1&issues=1&logo=https%3A%2F%2Fcdn.liteyuki.org%2Flogos%2Fbot.svg&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto
