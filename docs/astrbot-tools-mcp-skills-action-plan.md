# Tools / MCP / Skills 实施行动计划

## 目标

在当前仓库逐步落一套可演进的能力框架，参考 AstrBot 的分层，但按本项目现状收敛为：

1. 统一 Tool 抽象
2. 可向 LLM 暴露工具 schema
3. 支持本地工具与 MCP 工具并存
4. 支持 repo-local skills 作为说明书注入
5. 后续可扩展到 WebUI 和沙盒

## 原则

- 先把抽象和运行链跑通，再做 UI
- 先做最小闭环，再扩工具种类
- MCP 必须复用统一 Tool 抽象
- Skill 先只做 inventory + prompt 注入，不急着做复杂生命周期
- 所有高风险执行入口先做权限和路径约束

## Phase 1：定义最小抽象

目标：先把后面所有能力依赖的底座定下来。

产出：

- `ToolDefinition`
- `ToolCall`
- `ToolResult`
- `ToolManager`
- `ToolSchemaExporter`

建议内容：

- 定义统一工具元数据
  - `name`
  - `description`
  - `parameters`
  - `origin`
  - `active`
- 定义统一执行接口
  - 本地 handler
  - 远端 handler 适配入口
- 定义 OpenAI 风格 `tools` 导出方法

完成标准：

- 当前项目能注册 1 个本地 demo tool
- 能生成合法的 OpenAI `tools` 请求体

## Phase 2：接入 Tool Loop

目标：让模型返回的 tool call 能被真正执行并回写上下文。

产出：

- tool call 解析层
- tool executor
- tool result 回填逻辑

建议内容：

- 解析模型返回的工具名、参数、call id
- 根据名称查 `ToolManager`
- 执行本地 handler
- 将结果追加到对话上下文
- 允许下一轮继续推理

完成标准：

- 一个简单问答场景里，模型能调用本地工具并继续完成回答

## Phase 3：接入 MCP

目标：支持远端工具来源，但仍复用现有 Tool 抽象。

产出：

- `McpClient`
- `McpToolAdapter`
- `McpManager`
- MCP 配置文件

建议内容：

- 支持先做 HTTP/SSE MCP
- 后做 stdio MCP
- stdio 必须带 allowlist 和危险参数拦截
- `list_tools()` 结果统一适配成 `ToolDefinition`
- 调用失败时提供有限重连

完成标准：

- 能连接至少一个 MCP server
- 远端工具能以普通 tool 的方式出现在 LLM tool schema 中
- 远端工具能被 tool loop 正常执行

## Phase 4：接入 Skills

目标：把 repo-local skills 当成“说明书”而不是工具定义。

产出：

- `SkillManager`
- `SkillInfo`
- skills inventory prompt builder
- 本地 skills 目录规范

建议目录：

- `skills/<skill_name>/SKILL.md`

建议内容：

- 扫描 skills 目录
- 读取 frontmatter 的 `description`
- 生成简短 inventory prompt
- 明确要求模型先读 `SKILL.md` 再执行

完成标准：

- system prompt 中能稳定出现 skills inventory
- 模型能根据 skill 名称/描述决定是否读取 `SKILL.md`

## Phase 5：加执行环境约束

目标：让 skills 和工具执行受 runtime/权限控制，而不是裸奔。

产出：

- runtime 开关
  - `none`
  - `local`
  - 预留 `sandbox`
- 本地文件操作约束
- 命令执行白名单或前缀规则

建议内容：

- `runtime=none` 时，只给 skill inventory，不给执行能力
- `runtime=local` 时，限制 workspace 根目录
- shell / file edit / apply patch 要做边界检查

完成标准：

- 当 runtime 被禁用时，模型能看到 skill，但不能直接执行危险动作

## Phase 6：WebUI / 配置管理

目标：把底层能力变成可运维功能。

产出：

- tools 列表 API
- MCP server 管理 API
- skills 列表/上传 API

建议最小接口：

- `GET /api/tools`
- `POST /api/tools/toggle`
- `GET /api/mcp/servers`
- `POST /api/mcp/add`
- `POST /api/mcp/remove`
- `GET /api/skills`
- `POST /api/skills/upload`

完成标准：

- 不修改代码也能增删 MCP 配置和本地 skills

## 建议文件拆分

如果按当前仓库结构推进，建议先在后端侧预留这些模块：

- `src/llm/tools/mod.rs`
- `src/llm/tools/manager.rs`
- `src/llm/tools/schema.rs`
- `src/llm/tools/executor.rs`
- `src/llm/mcp/mod.rs`
- `src/llm/mcp/client.rs`
- `src/llm/mcp/manager.rs`
- `src/llm/skills/mod.rs`
- `src/llm/skills/manager.rs`
- `src/llm/skills/prompt.rs`

如果当前项目已有更合适的模块树，以现有结构为准，但职责最好保持分离。

## 推荐实施顺序

按优先级执行：

1. `ToolDefinition` / `ToolManager` / OpenAI schema 导出
2. tool loop 执行与结果回写
3. MCP 接入与 tool 适配
4. Skills inventory + `SKILL.md` 注入
5. runtime / 权限约束
6. WebUI 管理接口

不要倒过来做。先做 UI，后补抽象，最后大概率要重写。

## 第一轮建议交付

如果要尽快开始，我建议第一轮只做这 4 个东西：

1. 本地 tool 抽象和注册
2. OpenAI `tools` schema 导出
3. 基础 tool loop
4. skills inventory prompt 注入

先不要做：

- 复杂 MCP 管理页
- sandbox skill sync
- Neo 类技能生命周期

这样第一轮能尽快形成闭环，而且不会过早进入高复杂度状态。

## 验收标准

第一阶段可以用下面 5 条作为验收：

1. 模型能看到本地 tool schema
2. 模型能调用一个本地 tool 并拿到结果
3. 对话上下文里能保存 tool call 和 tool result
4. 模型能看到 skills inventory
5. 模型在匹配到 skill 时会先去读取 `SKILL.md`

## 借鉴优化清单

这一节只列值得从 AstrBot 直接借鉴的优化项，按优先级分层。

### P0 必做

- 统一 `Tool` 抽象。
  价值：后续本地工具、MCP、权限、UI 都能走一条链。
- tool loop 每步前做上下文处理。
  价值：不是入口清理一次，而是每轮推理前持续控长。
- turn 级截断 + token 级压缩双层机制。
  价值：先保底，再优化，复杂度可控。
- 截断后修复消息合法性。
  价值：避免 `assistant(tool_calls)` / `tool` 配对被截坏，减少 provider 兼容问题。
- `skills` 只做 inventory + 按需读取，不和 tools 混层。
  价值：明显节省上下文。

### P1 强烈建议

- `skills_like` 两阶段 tool schema。
  价值：工具多时非常有效，先只给 name/description，选中后再补参数 schema。
- token 计数包含多模态和 `tool_calls` JSON。
  价值：避免低估真实上下文成本。
- provider 侧清洗无效消息。
  价值：减少空 assistant 噪音和格式错误。
- MCP 统一适配成普通 `Tool`。
  价值：避免 MCP 和本地工具形成双轨制。

### P2 有价值但可后置

- LLM 摘要压缩旧历史。
  价值：长会话体验更稳，但实现和验证复杂度更高。
- sandbox skill sync。
  价值：skills 真正可执行时很有用，但不是第一轮闭环必需。
- SubAgent 分流工具域。
  价值：工具特别多时可以继续压主 Agent 上下文。

### P3 暂时不要急着做

- Neo skill lifecycle。
  原因：这是技能发布平台能力，不是当前最小可用框架。
- 复杂 MCP 管理页。
  原因：先把底层 Tool/MCP/runtime 跑通。
- 多 provider 全量适配。
  原因：先把一条 OpenAI 风格路径做稳。

### 建议落地顺序

1. `Tool` 抽象 + `ToolManager`
2. tool loop + 上下文修复/截断
3. skills inventory prompt
4. `skills_like` 两阶段 schema
5. MCP 适配
6. LLM 摘要压缩
7. sandbox / SubAgent / UI

## 暂不做

为了控制复杂度，这一轮建议明确不做：

- 多 provider schema 全覆盖
- 全功能 sandbox 生命周期
- skills 市场 / 下载中心
- 自动技能评估与发布流
- 大而全的权限系统

这些都可以在底座稳定后再补。
