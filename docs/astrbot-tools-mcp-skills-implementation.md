# AstrBot 中 Tools / MCP / Skills 的实现摘要

## 目的

这份文档只保留后续实现最需要的结论，基于 `dev-docs/AstrBot-master` 当前代码整理，回答三个问题：

1. `tools` 在 AstrBot 里如何定义和执行
2. `MCP` 如何接入并转成可调用工具
3. `skills` 如何注入 Agent，而不是和 tools 混在一起

更具体的落地安排已经单独拆到根目录：

- `astrbot-tools-mcp-skills-action-plan.md`

## 概念边界

- `Tool`：模型可直接调用的动作接口
- `MCP`：远端工具来源，最终会被适配成 `Tool`
- `Skill`：任务说明书，通常是 `SKILL.md`，不是 function calling schema

AstrBot 文档本身也是这样定义的：

- `docs/zh/use/skills.md`
- `docs/zh/use/computer.md`

一句话概括：

- Tool 是“做什么”
- Skill 是“什么时候做、怎么做”
- MCP 是“这些动作从哪里来”

## 总体分层

可以把 AstrBot 里的实现分成 4 层：

1. Tool 抽象层
   - 统一所有本地工具、插件工具、MCP 工具
2. Tool 管理层
   - 汇总工具、导出 schema、启停工具、挂接 MCP
3. Agent Runtime 层
   - 把工具发给模型，执行 tool loop，把结果回写上下文
4. Skill / Runtime 层
   - 给模型一份可按需读取的 `SKILL.md` 说明书，并决定它是否真能执行

## 一、Tools 的实现

### 1. 核心抽象

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/agent/tool.py`

核心类型：

- `ToolSchema`
  - `name`
  - `description`
  - `parameters`，使用 JSON Schema
- `FunctionTool`
  - 所有工具的统一抽象
  - 支持 `handler` 或覆写 `call()`
  - 有 `active` 状态
  - `handler_module_path` 用于记录插件归属
- `ToolSet`
  - 工具集合
  - 提供导出方法：
    - `openai_schema()`
    - `anthropic_schema()`
    - `google_schema()`

这个抽象很重要，因为 AstrBot 没把“工具来源”和“工具执行接口”绑死。

### 2. 内置工具注册

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/tools/registry.py`

机制：

- `@builtin_tool` 用于注册内置工具
- `_BUILTIN_TOOL_MODULES` 维护要懒加载的工具模块
- `ensure_builtin_tools_loaded()` 触发模块加载

这使得内置工具不是硬编码清单，而是模块自注册。

### 3. 工具管理中心

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/provider/func_tool_manager.py`

`FunctionToolManager` 负责：

- 汇总本地工具、插件工具、MCP 工具
- 维护启停状态
- 导出 provider 需要的 tool schema
- 管理 MCP 生命周期

关键方法：

- `get_full_tool_set()`
- `get_func(name)`
- `get_func_desc_openai_style()`
- `get_func_desc_anthropic_style()`
- `get_func_desc_google_genai_style()`
- `activate_llm_tool(name)`
- `deactivate_llm_tool(name)`

### 4. Provider 如何把 tools 发给模型

OpenAI 兼容 provider 入口：

- `dev-docs/AstrBot-master/astrbot/core/provider/sources/openai_source.py`

行为很直接：

1. 从 `ToolSet` 导出 OpenAI 风格 schema
2. 填进请求 payload 的 `tools`
3. 默认 `tool_choice=auto`
4. 调 `chat.completions.create(...)`

所以从 provider 视角看，AstrBot 的 tools 仍然是标准 function calling。

### 5. Tool loop 如何执行

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/agent/runners/tool_loop_agent_runner.py`

执行链：

1. 解析模型返回的 `tool_calls`
2. 根据名称找到对应 `FunctionTool`
3. 本地工具按 schema 过滤参数
4. MCP 工具没有本地 handler，直接透传参数
5. 执行工具
6. 把结果封装成 `tool` message 回写上下文
7. 进入下一轮推理

这说明 AstrBot 不是“一次工具调用即结束”，而是完整的多步 agent loop。

## 二、MCP 的实现

### 1. MCP 的角色

在 AstrBot 中，MCP 不是单独一套 Agent 运行时，而是：

- 一个远端工具来源
- 最终会被包装成 `MCPTool(FunctionTool)`

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/agent/mcp_client.py`

### 2. `MCPClient` 负责什么

`MCPClient` 负责：

- 建立连接
- 保存 `ClientSession`
- 列举远端工具
- 调用远端工具
- 自动重连
- 清理资源

支持的 transport：

- `sse`
- `streamable_http`
- `stdio`

关键方法：

- `connect_to_server(...)`
- `list_tools_and_save()`
- `call_tool_with_reconnect(...)`

### 3. MCP 如何挂进工具系统

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/provider/func_tool_manager.py`

MCP 初始化链：

1. `init_mcp_clients()` 读取 `data/mcp_server.json`
2. 为每个 active server 建连接
3. `list_tools_and_save()` 拉远端 tool 列表
4. 每个远端 tool 被包装成 `MCPTool`
5. 最终加入 `FunctionToolManager.func_list`

适配点在：

- `dev-docs/AstrBot-master/astrbot/core/agent/mcp_client.py`

其中 `MCPTool.call()` 会转发到远端 MCP server。

### 4. stdio 模式的安全限制

AstrBot 对 stdio MCP 做了明显收缩，仍在：

- `dev-docs/AstrBot-master/astrbot/core/agent/mcp_client.py`

关键限制：

- 禁 shell 元字符
- 命令必须在允许列表内
- 禁危险命令
- 禁 `python -c`
- 禁 `node -e`
- 禁高风险 docker 参数

如果当前仓库后面支持本地启动 MCP server，这部分建议直接借鉴。

### 5. WebUI 对 MCP 的管理

核心文件：

- `dev-docs/AstrBot-master/astrbot/dashboard/routes/tools.py`

主要接口：

- `/tools/mcp/servers`
- `/tools/mcp/add`
- `/tools/mcp/update`
- `/tools/mcp/delete`
- `/tools/mcp/test`

这层只做配置和生命周期操作，真正的连接细节还是交给 `FunctionToolManager`。

## 三、Skills 的实现

### 1. Skills 的角色

Skills 在 AstrBot 中不是 tools，而是可按需读取的任务说明书。

文档定义见：

- `dev-docs/AstrBot-master/docs/zh/use/skills.md`
- `dev-docs/AstrBot-master/docs/zh/use/computer.md`

本地存储形式通常是：

- `data/skills/<skill_name>/SKILL.md`

### 2. `SkillManager` 负责什么

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/skills/skill_manager.py`

职责：

- 扫描本地 skills
- 提取 frontmatter 中的 `description`
- 管理启停状态
- 管理 sandbox skill cache
- 安装/删除/更新 skill ZIP
- 生成注入 Prompt 的 skills inventory

关键方法：

- `list_skills(...)`
- `build_skills_prompt(skills)`
- `set_sandbox_skills_cache(skills)`
- `install_skill_from_zip(...)`

### 3. Skills 如何注入 Prompt

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/astr_main_agent.py`

主流程：

1. 读取 `computer_use_runtime`
2. `skill_manager.list_skills(active_only=True, runtime=runtime)`
3. `build_skills_prompt(skills)`
4. 把结果拼进 `req.system_prompt`

注入的不是完整 `SKILL.md`，而是：

- skill 名
- 描述
- 路径
- 触发规则
- “必须先读 `SKILL.md` 再执行”的规则

这就是 Progressive Disclosure 的真正实现点。

### 4. Skills 与 runtime 的关系

AstrBot 的 skills 是否真能执行，取决于 runtime：

- `none`
  - 模型能看到说明，但不能真正用 Shell/Python 执行
- `local`
  - 直接在本地环境执行
- `sandbox`
  - AstrBot 会尝试把本地 skills 同步到沙盒，再在沙盒执行

这意味着 skill 本身只是“说明书”，执行能力来自 runtime。

### 5. sandbox 技能同步

核心文件：

- `dev-docs/AstrBot-master/astrbot/core/computer/computer_client.py`

同步链：

1. 扫描本地 skills
2. 打包 zip
3. 上传到 sandbox
4. apply 阶段写入文件
5. scan 阶段回读 metadata
6. 更新本地 sandbox cache

AstrBot 特意把同步拆成 `apply` 和 `scan` 两阶段，方便定位失败点。

### 6. WebUI 对 skills 的管理

核心文件：

- `dev-docs/AstrBot-master/astrbot/dashboard/routes/skills.py`

主要接口：

- `/skills`
- `/skills/upload`
- `/skills/batch-upload`
- `/skills/download`
- `/skills/update`
- `/skills/delete`

skill 上传或删除后，AstrBot 还会尝试同步到当前活跃 sandbox。

## 四、Neo Skill Lifecycle 与普通 Skills 的区别

AstrBot 仓库里还有一套 `Shipyard Neo` 技能生命周期工具，核心文件：

- `dev-docs/AstrBot-master/astrbot/core/tools/computer_tools/shipyard_neo/neo_skills.py`

这类工具例如：

- `astrbot_create_skill_payload`
- `astrbot_create_skill_candidate`
- `astrbot_promote_skill_candidate`
- `astrbot_sync_skill_release`

它们不是普通 `SKILL.md` 注入机制，而是“技能发布和版本管理工具”。

要分清：

- 普通 Skills：本地/沙盒里的说明书
- Neo Skill Lifecycle：管理 skill release 的平台能力

## 五、对当前仓库最有用的结论

如果后面在当前仓库实现类似能力，最值得直接借鉴的是下面 5 点：

1. 先做统一 Tool 抽象，再做 MCP 和 Skills
2. 把 MCP 强制适配成统一 Tool，不要双轨运行
3. 把 Skill 保持为说明书，不要和 tool schema 混成一层
4. Skill 是否可执行要由 runtime 和权限系统决定
5. stdio MCP 必须做安全限制

## 关键文件索引

### Tools

- `dev-docs/AstrBot-master/astrbot/core/agent/tool.py`
- `dev-docs/AstrBot-master/astrbot/core/tools/registry.py`
- `dev-docs/AstrBot-master/astrbot/core/provider/func_tool_manager.py`
- `dev-docs/AstrBot-master/astrbot/core/provider/sources/openai_source.py`
- `dev-docs/AstrBot-master/astrbot/core/agent/runners/tool_loop_agent_runner.py`

### MCP

- `dev-docs/AstrBot-master/astrbot/core/agent/mcp_client.py`
- `dev-docs/AstrBot-master/astrbot/core/provider/func_tool_manager.py`
- `dev-docs/AstrBot-master/astrbot/dashboard/routes/tools.py`

### Skills

- `dev-docs/AstrBot-master/docs/zh/use/skills.md`
- `dev-docs/AstrBot-master/docs/zh/use/computer.md`
- `dev-docs/AstrBot-master/astrbot/core/skills/skill_manager.py`
- `dev-docs/AstrBot-master/astrbot/core/computer/computer_client.py`
- `dev-docs/AstrBot-master/astrbot/core/astr_main_agent.py`
- `dev-docs/AstrBot-master/astrbot/dashboard/routes/skills.py`
