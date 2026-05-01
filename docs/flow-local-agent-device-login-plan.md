# Flow Local Agent Device Login Plan

## Goal

为当前项目的 `Yuki Flow` 页面补齐与 Liteyuki Flow Local Agent 对齐的登录能力，优先实现设备码登录，并同时修正当前 token 暴露边界。

本计划是对现有 `docs/flow-local-agent-integration-plan.md` 的增量实施文档，聚焦认证与 WebUI 体验，不重复展开主链路运行时设计。

## Why This Step Exists

当前项目已经具备以下能力：

- 可保存 `flow_local_agent` 配置
- 可由本地 runtime 主动出站连接 Flow 的 `/ws/local-agent`
- 可在 WebUI 中展示连接状态与只读工具范围

但当前仍存在两个关键缺口：

- WebUI 只支持手工填写 token，不支持官方 local-agent 的登录流程
- `GetConfig` / `SetConfig` 会向前端返回明文 token，这与既定边界不一致

因此，这一步的目标不是重做连接层，而是补上认证前置链路，并把 token 收回到后端持有。

## Scope

本次纳入：

- 设备码登录
- token 脱敏与后端持有
- `Yuki Flow` 页面登录卡片与状态展示
- 最小必要测试

本次不纳入：

- 浏览器 callback 登录
- `run_command` / `write_file` 审批链路
- Flow Local Agent 主链路改为复用共享 caller
- 云端 Flow 服务端改动

## Upstream Alignment

参考上游实现：

- `dev-docs/agent-main/local_agent/src/auth.ts`
- `dev-docs/agent-main/server/routers/device_auth.py`

上游事实：

- 设备码登录通过 `POST /api/v1/auth/device/code` 获取 `device_code` 与 `user_code`
- 客户端轮询 `POST /api/v1/auth/device/token`
- 授权成功后返回的 token 是 `scopes="local-agent"` 的专用 token

这意味着本项目不应把“普通 API token 管理”与 “local-agent 登录”混为一谈。

## Design Decisions

### 1. 先做设备码登录，不先做浏览器登录

原因：

- 不需要本地临时 callback HTTP server
- 不依赖桌面浏览器自动打开是否成功
- 后端 API 与前端交互面更小，更适合当前项目的 WebUI 架构

### 2. 前端不再读取明文 token

调整后边界：

- 前端只知道 `hasToken` / `tokenPreview`
- token 的新增、替换、登录成功写入都只发生在后端
- `SetConfig` 不再要求 token 作为常规表单字段

### 3. 保留手工 token 写入，但改成专用接口

原因：

- 兼容已有高级用户场景
- 作为设备码登录失败时的 fallback
- 避免把 token 混入普通配置保存接口

## Backend Changes

### Config payload

将 `FlowLocalAgentWebConfigPayload` 改为不回传明文 token，新增：

- `has_token: bool`
- `token_preview: String`

其中：

- 无 token 时 `token_preview = ""`
- 有 token 时 `token_preview` 仅返回脱敏摘要，例如 `lys_abcd...wxyz`

### New API routes

在 `src/web/host/webui_config_api.rs` 新增：

- `POST /api/FlowLocalAgent/SetToken`
  - 手工写入 token
  - 请求体：`{ token: string }`
  - 成功后持久化到 `flow_local_agent.token`

- `POST /api/FlowLocalAgent/Auth/DeviceCode/Start`
  - 请求体：`{ baseUrl?: string }`
  - 代理调用上游 `/api/v1/auth/device/code`
  - 返回：`deviceCode`、`userCode`、`verificationUrl`、`expiresIn`

- `POST /api/FlowLocalAgent/Auth/DeviceCode/Poll`
  - 请求体：`{ baseUrl?: string, deviceCode: string }`
  - 代理调用上游 `/api/v1/auth/device/token`
  - 若状态为 `approved` 且返回 token，则直接持久化到本地配置
  - 返回：`status`、`hasToken`

### HTTP implementation notes

- 统一复用已有 `src/web/host/upstream.rs` 的 `reqwest::Client`
- base URL 继续沿用 `flow_local_agent.base_url` 的规范化逻辑
- 对上游错误做短文本归纳，不把原始 HTML / 大段 body 直接暴露给前端

## Frontend Changes

### `Yuki Flow` page

页面拆成三个意图清晰的区域：

- 连接配置
- 登录与凭据
- 运行状态 / 工具范围

### 登录区域

新增设备码登录卡片，包含：

- `开始设备码登录`
- `verificationUrl`
- `userCode`
- `复制验证码`
- `打开验证页面`
- `检查授权结果`

交互要求：

- 设备码登录成功后刷新配置与状态
- 前端不显示明文 token，仅显示“已保存 token”或脱敏摘要
- 手工 token 输入改为独立操作，不混在普通配置保存按钮里

### Existing page cleanup

顺手修正：

- 页面说明文案中混入的控制字符
- `run_command` 文案显示异常
- token 字段与普通配置表单耦合过重的问题

## Testing Plan

后端至少覆盖：

- `FlowLocalAgent/GetConfig` 不再返回明文 token
- `FlowLocalAgent/SetToken` 可持久化 token
- `FlowLocalAgent/Auth/DeviceCode/Poll` 在 approved 时写入 token
- `FlowLocalFileEntry` 可被 serde 反序列化，消除 IDE / 测试报错

前端至少验证：

- `pnpm build`

Rust 至少验证：

- `cargo test`

如遇已知环境性波动测试，应记录为非本变更引入。

## Delivery Order

1. 文档落地
2. 后端 token 脱敏与新认证 API
3. 前端 `Yuki Flow` 登录区改造
4. 测试修复与构建验证
