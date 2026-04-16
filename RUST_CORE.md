# Rust Core Layout

This crate is organized by responsibility to reduce future refactors:

- `src/core/`: runtime core (`BotRuntime`, `BotEvent`, dispatcher/worker model)
- `src/observability/`: logging and diagnostics (`Logger`, timestamp formatting)
- `src/bootstrap/`: startup settings and environment parsing (`RuntimeSettings`)
- `src/lib.rs`: stable public exports
- `src/main.rs`: minimal executable wiring for local runtime verification

## Log Environment Variables

- `LY_LOG_MODE`: `color` (default) or `mono`
- `LY_LOG_LEVEL`: `info` (default), `debug`, `warn`, `error`
- `LY_LOG_TZ`: `local` (default) or `utc`
- `LY_LOG_TS_FORMAT`: `custom` (default), `rfc3339`, `epoch_s`, `epoch_ms`
- `LY_LOG_TS_PATTERN`: custom chrono pattern, used when `LY_LOG_TS_FORMAT=custom`

Default log line template:

`{YYYY-MM-DD HH:mm:ss} | {LEVEL} {MODULE} {MESSAGE}`

## Runtime Environment Variables

- `LY_WORKERS`: worker count (default `4`)
  - also used as adapter parallel pool size (WS forward / reverse worker sharding / SSE / HTTP fallback limit)
- `LY_INGRESS_QUEUE`: ingress queue size (default `1024`)
- `LY_WORKER_QUEUE`: per-worker queue size (default `256`)

## External Config Files

Runtime can load YAML/TOML config first, then apply environment variable overrides.

Load order:

1. `LY_CONFIG_PATH` (if set)
2. `rust-config.yaml`
3. `rust-config.yml`
4. `rust-config.toml`
5. `config/rust-core.yaml`
6. `config/rust-core.toml`
7. `config.yaml` (only if it contains compatible `core/runtime/log` sections)

Top-level schema:

- `runtime.worker_count`
- `runtime.ingress_queue`
- `runtime.worker_queue`
- `log.mode`
- `log.level`
- `log.timezone`
- `log.timestamp_format`
- `log.timestamp_pattern`
- `adapters[]` (可同时配置多个：`http` / `sse` / `web_socket_forward` / `web_socket_reverse`，并兼容 `websocket_forward` / `websocket_reverse`)
- `connect.websocket` (`mode: forward/reverse/both`，支持 `url` 或 `host+port+path`，支持 `forward`/`reverse` 分块同时启用，支持 `max_payload_size`、`max_connections`)
- `connect.tcp-http`（支持 `max_payload_size`、`max_connections`）
- `connect.sse`（支持 `max_payload_size`）
- `tui.resume.store_path`
- `tui.resume.max_sessions`
- `tui.resume.max_size_mib`
- `llm.enabled/provider/base_url/api_keys/api_key/model/timeout_seconds/system_prompt/command_prefix`

LLM 配置支持独立文件覆盖（建议用于密钥隔离）:

- 环境变量 `LY_LLM_CONFIG_PATH`（最高优先级）
- `llm-config.yaml`
- `llm-config.toml`

运行时会把该文件中的 `llm` 段覆盖到主配置上。

Or nested under `core`:

- `core.runtime.*`
- `core.log.*`
- `core.adapters[]`
- `core.tui.resume.*`
