# Flow Local Agent Integration Plan

## Goal

将当前项目集成为 `dev-docs/agent-main` 所需的本地执行端（Local Agent），通过 WebSocket 连接 Liteyuki Flow 云端，接收工具调用并返回结果。

参考文档：

- `dev-docs/agent-main/docs/integration.md`

本计划只覆盖当前仓库需要实现的客户端侧能力，不覆盖 `agent-main` 服务端实现。

## Current Status

截至 2026-05-01，Phase 0 已部分完成，当前代码状态如下：

- 已完成：
  - `flow_local_agent` 配置段已加入 app config
  - runtime bootstrap 已可携带 `flow_local_agent` 配置
  - `src/flow_local_agent/` 模块已接入编译
  - 协议消息类型、close code、运行时状态、客户端占位实现已存在
  - 最小单元测试已存在并通过
- 已验证：
  - `cargo check`
  - `cargo test --lib flow_local_agent`
- 尚未完成：
  - `main` / `web runtime` 的启动接线
  - Web Host 状态暴露
  - 共享只读 tool caller 的主链路复用说明
  - 审批 API
  - `write_file`
  - `run_command`

当前阶段的定位应视为：

- Phase 0: schema and skeleton
  - 已完成
- Phase 1 及以后
  - 尚未开始正式接线

## Integration Positioning

当前项目应扮演：

- Liteyuki Flow 的本地 Agent / 工具执行端
- 持有本地运行时、工作区访问能力、命令执行能力、审批能力

当前项目不应扮演：

- Flow 服务端
- Flow Web 前端
- `agent-main` 根 Agent 编排层

## Protocol Scope

首期需要支持的协议点：

- WebSocket 连接到 `/ws/local-agent`
- query 参数：`token`、`device_id`、`device_name`、`os`、`version`
- 服务端 `{"type":"ping"}` -> 客户端 `{"type":"pong"}`
- 工具请求：
  - `run_command`
  - `read_file`
  - `write_file`
  - `list_files`
- 结果回传：
  - `{"id":"...","result":"..."}`
  - `{"id":"...","error":"..."}`
- 危险操作确认：
  - 客户端发 `confirm_request`
  - 服务端回 `confirm_response`
- 断线重连
- close code 处理：
  - `4001` 无效 token，不重连
  - `4002` 同设备新连接挤掉旧连接，不重连
  - `4003` 设备被移除或 token 被吊销，不重连

## Design Principles

- 协议层、执行层、审批层、Web API 层分离，避免单模块膨胀
- 优先复用现有 workspace/tool/web-host 能力，不复制相同逻辑
- `run_command` 单独控风险，不和文件读写共用实现
- 前端不直接持有 Flow token，不直接发起到云端的 Agent 连接
- 默认保持最小权限，优先只读能力，逐步开放写入和命令执行

## Existing Reusable Modules

建议优先复用以下模块：

- `src/llm/tools.rs`
  - 现有本地工具目录与运行时 bundle 组织
- `src/llm/tools/local_execution_tools.rs`
  - 现有 workspace 只读工具定义
- `src/llm/tools/workspace_access/file_ops.rs`
  - 安全的文件列举和读取
- `src/llm/tools/workspace_access/path_safety.rs`
  - 路径逃逸防护
- `src/llm/shared_tool_caller.rs`
  - 共享 workspace 只读工具调用入口
- `src/web/host/mod.rs`
  - Web Host 生命周期、认证、状态挂载点
- `src/web/host/capability_api.rs`
  - capability 类 API 的现有路由风格
- `src/web/host/terminal.rs`
  - 可借鉴其 session/state 组织方式，但不直接复用为工具执行
- `src/runtime_support/bootstrap.rs`
  - 运行时启动组织入口

## Recommended Module Layout

建议新增独立模块：

- `src/flow_local_agent/mod.rs`
- `src/flow_local_agent/config.rs`
- `src/flow_local_agent/protocol.rs`
- `src/flow_local_agent/client.rs`
- `src/flow_local_agent/dispatcher.rs`
- `src/flow_local_agent/tools.rs`
- `src/flow_local_agent/approval.rs`
- `src/flow_local_agent/device.rs`
- `src/flow_local_agent/tests.rs`

职责建议如下：

- `config.rs`
  - 解析和归一化配置
  - 计算默认值
- `protocol.rs`
  - WebSocket 入站/出站消息类型
  - close code 语义常量
- `client.rs`
  - 建连、重连、心跳、接收循环、发送循环
- `dispatcher.rs`
  - 将协议请求映射为内部执行请求
- `tools.rs`
  - `run_command/read_file/write_file/list_files` 执行入口
- `approval.rs`
  - 危险命令识别、审批等待、会话级 always-approve 状态
- `device.rs`
  - `device_id`、`device_name`、`os`、`version` 计算与持久化

## Configuration Plan

建议在 `src/app_config.rs` 中新增独立配置段：

- `flow_local_agent.enabled`
- `flow_local_agent.base_url`
- `flow_local_agent.token`
- `flow_local_agent.device_id`
- `flow_local_agent.device_name`
- `flow_local_agent.auto_connect`
- `flow_local_agent.allowed_tools`
- `flow_local_agent.workspace_root`
- `flow_local_agent.command_timeout_seconds`
- `flow_local_agent.approval_policy`

补充约束：

- `device_id` 需要持久化
- `token` 只保存在后端配置，不下发到前端页面
- `allowed_tools` 允许后续按部署环境裁剪工具面
- `workspace_root` 默认可取当前工作区根目录

## Execution Model

### 1. Connection lifecycle

- 启动时读取配置
- `enabled=true` 且配置完整时启动 local agent runtime
- 建立到 Flow 的出站 WebSocket
- 接收到 `ping` 时立即回复 `pong`
- 普通断线 3 到 5 秒后重连
- `4001/4002/4003` 进入终止状态并记录原因

### 2. Request lifecycle

- 服务端下发 `{id, tool, args}`
- 协议层反序列化
- dispatcher 校验工具名是否允许
- 进入具体工具执行器
- 执行完成后返回 `{id, result}` 或 `{id, error}`

### 3. Approval lifecycle

- `run_command` 进入风险检查
- 若命中危险规则：
  - 本地发 `confirm_request`
  - 进入 pending 状态并等待 `confirm_response`
- 收到批准：
  - 一次性批准则只执行当前请求
  - `always=true` 则在当前连接会话内缓存批准状态
- 收到拒绝或超时：
  - 返回 error

## Tool Mapping Plan

### Shared read-only caller boundary

当前实现约束：

- 共享层仅负责 workspace 范围内的 `list_files` / `read_file`
- 共享层直接复用 `workspace_access`，不引入 `run_command` / `write_file`
- Flow Local Agent 已复用该共享层给本地 LLM workspace 工具使用，但其对外暴露给 Flow 的只读工具已单独实现为上游兼容语义
- 当前本地 agent 主链路不直接接入该共享 caller
- 后续如需在沙盒模式下复用，本地 agent 主链路应复用同一共享层，而不是重新实现一套文件只读逻辑

### `list_files`

当前实现：

- 共享 caller 仍直接复用 `workspace_access/file_ops.rs`
- Flow Local Agent 对外 `list_files` 单独实现，返回上游兼容的 JSON 数组条目：`[{name,type,size}]`
- Flow Local Agent 支持绝对路径、`~` 展开；相对路径优先基于 `flow_local_agent.workspace_root`，未配置时退回进程当前目录

### `read_file`

当前实现：

- 共享 caller 仍复用现有 workspace 安全读取逻辑
- Flow Local Agent 对外 `read_file` 单独实现，返回上游兼容的原始文本内容
- Flow Local Agent 支持绝对路径、`~` 展开，并对结果做 100KB 截断

### `write_file`

建议：

- 基于现有 `path_safety.rs` 增补安全写入能力
- 默认仅允许写入 workspace 内路径
- 后续如有需求再扩展更大范围

### `run_command`

建议：

- 独立实现，不复用 terminal session
- 显式处理：
  - `cwd`
  - 超时
  - 输出截断
  - stderr 合并策略
  - Windows/Unix 平台兼容
- 首期默认关闭或默认需审批

## Web Host Integration

建议通过 Web Host 暴露本地 Agent 状态和审批接口，而不是让前端直接操作 runtime 内部对象。

建议新增状态挂载：

- `WebHostService.flow_local_agent_state`

建议新增 API：

- `GET /api/flow-local-agent/status`
  - 当前连接状态、设备信息、最后错误、可用工具
- `GET /api/flow-local-agent/confirmations`
  - 待审批请求列表
- `POST /api/flow-local-agent/confirmations/{id}/approve`
- `POST /api/flow-local-agent/confirmations/{id}/reject`
- `POST /api/flow-local-agent/confirmations/{id}/always`

前端只展示：

- 连接状态
- 当前设备信息
- 待审批命令
- 路径、超时、风险标签

前端不展示：

- 系统 prompt
- 调试提示词
- token 原文

## Runtime Bootstrap Plan

建议在 runtime 启动链路中按以下顺序接入：

1. 解析 `flow_local_agent` 配置
2. 初始化 `device_id` 和持久化
3. 初始化 runtime state
4. 启动 WebSocket client 后台任务
5. 将状态句柄挂到 WebHost

推荐挂载点：

- `src/runtime_support/bootstrap.rs`
- `src/web/runtime.rs`
- `src/web/host/mod.rs`

## Phase Plan

### Phase 0: schema and skeleton

目标：

- 加入配置结构
- 加入模块骨架
- 加入协议类型
- 完成状态对象和 runtime 启动骨架

验收：

- 项目可编译
- 不启用时行为无变化

### Phase 1: read-only MVP

目标：

- 实现 WebSocket 连接、重连、ping/pong
- 抽出共享只读 tool caller
- 实现 `list_files`
- 实现 `read_file`
- 实现基础状态查询 API

验收：

- 可接入 Flow 并成功执行只读工具
- 路径逃逸被拒绝

### Phase 2: approval and web UI

目标：

- 实现 `confirm_request/confirm_response`
- 实现待审批状态管理
- 实现审批 API
- 前端增加审批面板和连接状态展示

验收：

- 危险命令不会直接执行
- 审批后执行路径完整
- 拒绝和超时行为正确

### Phase 3: safe write support

目标：

- 实现 `write_file`
- 限制写入范围在 workspace 内
- 增加必要的审计日志

验收：

- 正常写入成功
- 越权写入失败

### Phase 4: command execution

目标：

- 实现 `run_command`
- 超时、截断、cwd、安全规则生效
- 平台差异处理

验收：

- 安全命令可执行
- 危险命令需审批
- 超时和异常输出稳定

## Suggested Work Split

以下拆分仍可作为任务边界参考，但从当前阶段开始，不再作为默认执行顺序。
后续实现以串行为主，只有在写入范围完全不重叠、且不会影响主线联调时，才允许并行。

适合并行拆给多个 LLM 的任务包：

### Workstream A: config and runtime skeleton

- 配置结构
- bootstrap 挂载
- runtime state
- 文档化默认行为

### Workstream B: protocol and client

- message model
- ws client
- reconnect and close code policy
- client tests

### Workstream C: tool executor

- `list_files`
- `read_file`
- `write_file`
- `run_command`
- 安全策略测试

### Workstream D: approval pipeline

- pending confirmation state
- danger detection
- always-approve session logic
- approval API

### Workstream E: web UI

- 状态页或配置页入口
- 审批列表
- 连接状态展示
- 前后端接口联调

建议并行边界：

- A 与 B 可并行
- C 的只读部分可与 A/B 并行
- D 依赖 B 的协议消息通路
- E 依赖 D 的后端 API 定型

## Sequential Execution Policy

从 Phase 1 开始，默认采用严格串行推进：

1. 先完成 runtime 启动接线
2. 再完成只读工具执行链路
3. 再暴露状态查询 API
4. 再补审批状态与审批 API
5. 再做 `write_file`
6. 最后做 `run_command`

只有满足以下条件时，才允许局部并行：

- 写入文件集合完全不重叠
- 主线步骤不会被并行任务阻塞
- 不会引入需要大范围回滚或重构的接口漂移

## Testing Plan

至少补齐以下测试：

- 配置解析测试
- `device_id` 持久化测试
- `ping/pong` 测试
- close code 行为测试
- 工具映射测试
- workspace 路径逃逸测试
- 写入越权测试
- 命令超时测试
- 审批通过/拒绝/超时测试
- Web API 路由测试

建议测试文件：

- `tests/flow_local_agent_config.rs`
- `tests/flow_local_agent_protocol.rs`
- `tests/flow_local_agent_tools.rs`
- `tests/flow_local_agent_approval.rs`
- `tests/flow_local_agent_web_api.rs`

## Risks

### Risk 1: `run_command` 风险过高

说明：

- shell 执行面是整条链路里风险最高的部分

缓解：

- 最后实施
- 默认关闭或默认审批
- 限制 cwd 和 timeout

### Risk 2: 重复实现已有 workspace 能力

说明：

- 若跳过现有 `workspace_access`，容易引入行为分叉

缓解：

- 文件工具统一走现有 path safety 和 file ops

### Risk 3: UI 泄露敏感信息

说明：

- token、prompt、内部错误细节不应原样暴露

缓解：

- 前端只拿状态摘要和审批数据
- token 仅后端持有

### Risk 4: 模块耦合到 plugin sdk 或 llm api

说明：

- 该能力本质上是 runtime agent，不属于 plugin ABI，也不是聊天 API

缓解：

- 独立放在 `src/flow_local_agent/`

## Explicit Non-Goals

当前阶段不做：

- 复刻 `agent-main` 服务端设备数据库
- 复刻 `agent-main` 的 Web 前端管理页
- 非 workspace 范围的大面积文件系统访问
- 完整 sudo 密码缓存与跨会话保存
- 将 terminal WebSocket 直接包装成 `run_command`

## Recommended Implementation Order

当前推荐顺序更新为：

1. 配置结构和 runtime 骨架
   当前状态：已完成
2. `main` / `web runtime` 启动接线
   目标：让 `flow_local_agent` runtime 真正进入启动链，但先不开放工具执行
3. 协议模型和 WS client 接线
   目标：真正建立连接、处理 `ping/pong`、close code、重连策略
4. 只读工具 `list_files/read_file`
   目标：抽出共享只读 tool caller，并打通 Flow Local Agent 的第一条真实工具调用链
5. 状态查询 API
   目标：通过 Web Host 查看连接状态、设备信息、最近错误
6. 审批状态和审批 API
   目标：为危险操作确认建立后端通路
7. 前端审批入口
   目标：只展示状态和审批，不暴露 token 或内部提示词
8. `write_file`
9. `run_command`

如果后续实现与本文档顺序不一致，应先更新本文档，再继续改代码。

## Delivery Rule

后续每个实施任务都应满足：

- 新模块单一职责明确
- 不把大量逻辑塞进 `mod.rs`
- 单个函数尽量保持短小
- 每阶段先补测试再继续扩功能
- 前端不写入无意义内容或内部提示词
