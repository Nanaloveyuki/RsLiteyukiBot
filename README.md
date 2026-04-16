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

```bash
cargo run
```

首次启动会自动创建默认 `config.yaml`（若文件不存在）。

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

## 运行与交互

### TUI 内置命令

- `/help`
- `/reload`
- `/log [on|off]`
- `/clear`
- `/adapters`
- `/ask <prompt>`
- `/resumes` / `/history`
- `/resume <uid>`
- `/llm ...`
- `/whitelist ...`
- `/quit` / `/exit`

快捷键：`Tab` 自动补全，`Up/Down` 历史命令，`PgUp/PgDn/Home/End` 日志滚动。

### OneBot v11 外部命令

- 外部 `/help` 受 `onebot-v11.whitelist` 白名单控制
- 外部 `/ask` 使用配置中的 `llm.command_prefix`（默认 `/ask`）

## 常用环境变量

- `LY_RUNTIME_TARGET`：`cli` / `web` / `tauri2` / `docker` / `cli-web` / `docker-web`
- `LY_WORKERS`、`LY_INGRESS_QUEUE`、`LY_WORKER_QUEUE`
- `LY_LOG_MODE`、`LY_LOG_LEVEL`、`LY_LOG_TZ`
- `LY_LOG_TS_FORMAT`、`LY_LOG_TS_PATTERN`
- `LY_ADAPTERS_PATH`、`LY_ADAPTERS_JSON`
- `LY_RESUME_STORE_PATH`、`LY_TUI_RESUME_MAX_SESSIONS`、`LY_TUI_RESUME_MAX_SIZE_MIB`
- `LY_LLM_ENABLED`、`LY_LLM_PROVIDER`、`LY_LLM_BASE_URL`
- `LY_LLM_MODEL`、`LY_LLM_TIMEOUT_SECONDS`、`LY_LLM_COMMAND_PREFIX`
- `LY_LLM_API_KEY`、`LY_LLM_API_KEYS`
- `LY_HELP_WHITELIST_DEBUG`、`LY_OB11_LOG_IMAGE_SUMMARY`

## 开发与测试

```bash
cargo test
```

当前仓库含 Rust 侧集成测试，覆盖配置迁移、适配器传输、会话路由、插件管理、日志格式与生命周期等关键模块。

## 项目结构

- `src/core/`：运行时核心（事件分发、生命周期、进程管理）
- `src/adapter/`：WebSocket / SSE / HTTP 适配层
- `src/session/`：会话事件与规则路由
- `src/observability/`：日志与可观测性
- `src/bootstrap/`：配置加载与环境变量解析
- `src/tui/`：终端 UI 与控制台交互
- `src/plugin/`：插件抽象与加载管理

## 注意事项

- `config.yaml`、`llm-config.yaml` 已在 `.gitignore` 中，建议继续保持密钥本地化。
- `runtime/log` 的底层参数变更后，`/reload` 可能无法完全热生效，建议重启进程。


[Liteyuki6.0]: https://img.shields.io/badge/Liteyuki-6.0-blue?style=for-the-badge

[banner]: https://socialify.git.ci/Nanaloveyuki/RsLiteyukiBot/image?description=1&font=Source+Code+Pro&forks=1&issues=1&logo=https%3A%2F%2Fcdn.liteyuki.org%2Flogos%2Fbot.svg&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto
