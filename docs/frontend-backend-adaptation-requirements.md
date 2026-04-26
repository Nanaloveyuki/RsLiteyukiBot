# Frontend Backend Adaptation Requirements

## 目的

基于当前仓库后端与现有前端实现，整理一份前端改造清单，明确：

- 后端已经具备、但前端还没有正确承接的能力
- 前端想做、但当前后端仍缺字段或接口的能力
- 下一轮前端改造应该如何拆页面、拆面板、拆交互

这份文档只讨论当前仓库已经能验证到的事实，不做脱离代码的泛化设计。

## 已确认的后端基线

### 1. LLM / Prompt / Provider

后端已具备：

- Web LLM 设置与聊天接口：
  - `GET /api/LLM/GetSettings`
  - `POST /api/LLM/Chat`
- Provider / Model 管理接口：
  - `GET /api/LLM/GetManagerState`
  - `POST /api/LLM/SaveManagerState`
  - `POST /api/LLM/FetchModels`
  - `POST /api/LLM/TestModels`
  - `POST /api/LLM/PreviewRequest`
- Prompt profile 底层存储能力：
  - `src/llm/prompt.rs`
  - `src/llm/service.rs`
- Prompt 预览组合逻辑：
  - `build_prompt_preview(...)`
  - `compose_user_prompt(...)`
- Prompt store 默认文件：
  - `%USERPROFILE%/.liteyuki/configs/llm-prompts.json`

当前结论：

- 后端已经有 prompt profile 的真实存储、激活、增改删和预览逻辑。
- 但 Web Host 还没有暴露 prompt profile 的专用 HTTP API。
- 现有前端只能读到 `promptProfile` 名称，不能做真正的编辑/预览/保存。

### 2. 插件管理

后端已具备：

- 插件列表：
  - `GET /api/Plugin/List`
- 插件启用/禁用：
  - `POST /api/Plugin/SetStatus`
- 本地 zip 导入：
  - `POST /api/Plugin/Import`
- 插件商店只读清单：
  - `GET /api/Plugin/Store/List`
  - `GET /api/Plugin/Store/Detail/{id}`
- 显式插件配置读写：
  - `GET /api/Plugin/Config?id=<plugin_id>`
  - `POST /api/Plugin/Config`
- 插件扩展页面：
  - `/plugin/{plugin_id}/page/{page_path}`

后端当前还能确认到的事实：

- 插件元数据里已经有 `plugin_type`、`runtime.kind`、`sdk.api_version` 等结构。
- Python 兼容层已经保留 AstrBot 相关运行时注册信息：
  - LLM tools
  - web APIs
  - cron jobs
  - scheduled tasks
  - agents
- Web API 已经把这些 runtime capability 真正导出给前端：
  - `GET /api/Plugin/Capabilities`
  - `GET /api/Plugin/Capabilities/All`
  - `GET /api/Plugin/Tools`
  - `GET /api/Plugin/WebApis`
  - `GET /api/Plugin/CronJobs`
  - `GET /api/Plugin/Tasks`
- runtime web api 已经可以通过宿主路由真实执行：
  - `/api/Plugin/Runtime/WebApi/{plugin_id}/{registered_path...}`
- plugin tools 已经可以通过宿主执行与诊断接口被调用：
  - `POST /api/Plugin/Tools/Execute`
  - `GET /api/Plugin/RuntimeState`
  - `GET /api/Plugin/Diagnostics`
- 现有 `/api/Plugin/List` 返回字段仍偏“展示态”，还不足以区分：
  - Liteyuki 原生插件
  - Liteyuki Python Bridge 插件
  - AstrBot 兼容插件

### 3. Tools / MCP / Skills

后端当前已具备只读管理接口：

- `GET /api/tools`
- `GET /api/mcp/servers`
- `GET /api/skills`

后端当前已具备运行时能力：

- 本地 ToolManager
- repo-local skills 扫描与 inventory prompt 注入
- Streamable HTTP MCP 读取与工具适配
- 本地工具、MCP 工具、skills inventory 统一进入 LLM runtime

当前结论：

- 后端已经可以让前端做“只读盘点面板”。
- 但当前还没有完整的可写管理接口，例如：
  - tool enable/disable
  - MCP save/test
  - skill read/upload/update

## 当前前端最主要的问题

### 1. LLM 能力被放在 `debug` 下，产品层级不对

当前前端里：

- `frontend/src/pages/dashboard/debug/http/index.tsx`
  - 实际承担了 provider/model 管理
- `frontend/src/pages/dashboard/debug/websocket/index.tsx`
  - 实际承担了 Web LLM chat playground

这两个页面已经不只是调试页，而是实际的产品能力页。下一轮前端应把它们从 `debug` 区域提升为正式信息架构：

- `AI / 对话`
- `AI / Provider 与模型`
- `AI / Prompt 配置`
- `AI / Tools / MCP / Skills`

不建议继续把真实可用的 AI 能力藏在 `debug` 分组下。

### 2. Prompt 只有“当前档案名”，没有真正的管理闭环

当前前端只能看到：

- 当前使用的 `promptProfile`

但按后端现状，前端真正应该支持的能力是：

- profile 列表
- active profile 切换
- profile 新建
- profile 编辑
- profile 删除
- prompt 预览
- 保存后的回读

并且这些行为要符合后端已有规则：

- `default` profile 必须存在
- `default` profile 不能删除
- `active_profile` 必须始终指向一个存在的 profile
- preview 应区分：
  - `system_prompt`
  - `composed_user_prompt`
  - `combined_prompt`

### Prompt 面板建议

建议新增正式页面：

- `AI / Prompts`

页面结构建议：

- 左侧 profile 列表
- 中间 profile 编辑器
- 右侧 preview 面板

最低字段：

| 字段 | 来源 | 备注 |
| --- | --- | --- |
| `name` | prompt store | profile 名 |
| `soul` | prompt store | profile 主体内容 |
| `active` | prompt store | 当前是否启用 |
| `configPath` | 后端补充更佳 | 建议显示当前实际文件路径 |
| `preview.systemPrompt` | `build_prompt_preview` | 系统提示词 |
| `preview.composedUserPrompt` | `build_prompt_preview` | 组合后的用户提示 |
| `preview.combinedPrompt` | `build_prompt_preview` | 最终预览 |

### Prompt 相关后端缺口

前端这部分目前会被后端接口阻塞。建议后端补齐一组专用 API，前端再接正式 UI：

- `GET /api/LLM/PromptProfiles`
- `POST /api/LLM/PromptProfiles/Save`
- `POST /api/LLM/PromptProfiles/Delete`
- `POST /api/LLM/PromptProfiles/Use`
- `POST /api/LLM/PromptProfiles/Preview`

如果不补这组 API，前端最多只能做一个只读展示页，不可能完成“编辑/预览/保存”闭环。

### 3. 聊天页仍在前端本地推导 provider 能力，应该以后端返回为准

当前聊天页里，provider 能力存在前端本地推导逻辑。下一轮应收敛到后端返回的统一事实，避免前后端漂移。

前端应优先使用：

- `GET /api/LLM/GetSettings` 返回的：
  - `providerCatalog`
  - `supports`
  - `reasoningOptions`
  - `promptProfile`

不建议继续在前端重复维护：

- 哪个 provider 支持图片
- 哪个 provider 支持 reasoning
- 哪个 provider 支持 topP / topK / temperature

这些能力矩阵已经逐渐变成后端事实，前端再自算只会继续漂移。

### 4. 插件页只能做“启停 + 配置 + 扩展页”，不足以承接当前后端方向

当前前端插件页主要支持：

- 列表展示
- 启停
- 导入 zip
- 显式 config 弹窗
- 扩展页 iframe

但从当前后端方向看，插件页下一轮必须升级为“插件中心”，至少拆出下面几层：

- 已安装插件列表
- 插件详情页
- 插件配置页
- 插件扩展页入口
- 插件运行时能力页

### 插件列表页需要新增的维度

当前 `/api/Plugin/List` 只返回展示字段，不足以满足前端区分插件类型。前端下一轮需要这些 badge / 筛选项：

- `status`
  - `active`
  - `disabled`
  - `stopped`
- `runtimeKind`
  - `native`
  - `python`
  - `lua`
  - `external`
- `pluginType`
  - `application`
  - `service`
  - `module`
  - `unclassified`
  - `test`
- `sourceKind`
  - `liteyuki-native`
  - `liteyuki-python-bridge`
  - `astrbot-compatible`
- `hasConfig`
- `hasPages`
- `hasCapabilities`
  - tools
  - webApis
  - cronJobs
  - tasks

### “AstrBot 插件”和 “LiteyukiBot 插件” 的区分要求

这是本轮最重要的插件 UI 要求之一。

前端必须能一眼区分：

| 分类 | 期望判定方式 |
| --- | --- |
| Liteyuki 原生插件 | `runtime.kind = native` |
| Liteyuki Python Bridge 插件 | `runtime.kind = python` 且非 AstrBot compat |
| AstrBot 兼容插件 | `runtime.kind = python` 且带 compat / AstrBot capability 标记 |

问题在于：

- 当前 `/api/Plugin/List` 还没有把这些判定字段直接吐给前端。
- 仅靠现有 `name/id/version/description/author/status/hasConfig/hasPages`，前端无法稳定做出区分。

所以这里的正确顺序是：

1. 前端先按这三类设计 UI 结构与 badge 位
2. 后端补充区分字段
3. 再落实际筛选与详情页

### 插件详情页建议拆 Tab

建议详情页最少拆成四个 Tab：

- `Overview`
- `Config`
- `Pages`
- `Capabilities`

其中 `Capabilities` 用来承接后续后端补充的 runtime capability API，例如：

- Tools
- Web APIs
- Cron Jobs
- Tasks

当前后端已可直接对接：

- `GET /api/Plugin/Capabilities?id=<plugin_id>`
- `GET /api/Plugin/Tools?id=<plugin_id>`
- `GET /api/Plugin/WebApis?id=<plugin_id>`
- `GET /api/Plugin/CronJobs?id=<plugin_id>`
- `GET /api/Plugin/Tasks?id=<plugin_id>`
- `GET /api/Plugin/RuntimeState?id=<plugin_id>`
- `GET /api/Plugin/Diagnostics?id=<plugin_id>`

### 插件相关后端缺口

当前前端已经可以完成 capability / runtime state / diagnostics 面板，因为后端已正式暴露：

- `GET /api/Plugin/Capabilities?id=<plugin_id>`
- `GET /api/Plugin/Tools?id=<plugin_id>`
- `GET /api/Plugin/WebApis?id=<plugin_id>`
- `GET /api/Plugin/CronJobs?id=<plugin_id>`
- `GET /api/Plugin/Tasks?id=<plugin_id>`
- `GET /api/Plugin/RuntimeState?id=<plugin_id>`
- `GET /api/Plugin/Diagnostics?id=<plugin_id>`

仍然存在的后端缺口是：

- `/api/Plugin/List` 还没有足够稳定的 `sourceKind / compatKind` 判定字段
- cron / task 仍然没有真实 scheduler backend
- capability snapshot 仍是 runtime 态，不是未加载可见的持久快照

### 5. 扩展页应从“单独页面”升级为插件详情的一部分

当前已有：

- `frontend/src/pages/dashboard/extension.tsx`
- `/plugin/{plugin_id}/page/{page_path}`

这条链路已经可用，而且后端已经做了 iframe 页面鉴权桥接。

下一轮前端不应只保留一个全局扩展页中心，还应把扩展页入口挂回插件详情里：

- 插件卡片 -> 详情 -> `Pages`
- 插件详情里直接列出该插件所有 extension pages
- 支持：
  - 当前页 iframe 打开
  - 新窗口打开
  - 无页面时明确显示空态

这会比把所有扩展页完全平铺到全局 `Extension` 页更符合插件语义。

### 6. Tools / MCP / Skills 目前完全缺前端承接页

后端这部分已经有只读接口，但前端没有正式面板。

建议新增正式页面：

- `AI / Capabilities`

并拆成三个 Tab：

- `Tools`
- `MCP`
- `Skills`

### Tools 面板

直接对接：

- `GET /api/tools`

建议展示字段：

- `name`
- `description`
- `category`
- `origin`
- `whenToUse`
- `strict`
- `parameters`

这个面板当前定位应是：

- 运行时 inventory
- tool schema inspector
- 排障页

而不是第一轮就做“工具市场”。

### MCP 面板

直接对接：

- `GET /api/mcp/servers`

建议展示字段：

- `name`
- `transport`
- `url`
- `active`
- `toolCount`
- `toolNames`
- `warnings`

建议交互：

- 当前第一轮先做 inventory + diagnostics 主视图
- 在 MCP 面板里可以直接开放基础配置管理：
  - save
  - test
- 重点显示 warning，而不是只显示是否成功

因为当前后端设计就是：

- 配置失败要显式 warning
- unsupported transport 不应该静默消失

### Skills 面板

直接对接：

- `GET /api/skills`

建议展示字段：

- `name`
- `description`
- `path`

这一页当前应理解为：

- repo-local skill inventory
- 不是执行器
- 不是市场

### Tools / MCP / Skills 的当前后端缺口

这组能力相比最初盘点已经继续往前走了，后端现在已补齐基础管理接口：

- `POST /api/tools/toggle`
- `POST /api/mcp/save`
- `POST /api/mcp/test`
- `GET /api/skills/read?name=<skill_name>`
- `POST /api/skills/upload`

这意味着前端第一轮已经不必把所有可写按钮都做成 `coming soon`。更合理的收敛是：

- 先做稳定的 inventory / diagnostics 面板
- 同时开放基础表单型管理动作：
  - tool enable / disable
  - 但 `list_tool_categories` / `list_tools_in_category` / `get_tool_schema` 这三个 discovery helper 不应允许关闭
  - MCP config save / test
  - skill read / upload
- 暂时继续把更深层的管理能力留到后续：
  - tool policy / permission 分级
  - MCP 进程托管
  - skill update / delete

## 跨页面的前端实现约束

### 1. 一律走 runtime-aware 请求层

当前前端已经有：

- `frontend/src/utils/runtime.ts`
- `frontend/src/utils/auth.ts`
- `frontend/src/utils/request.ts`

下一轮前端新增 API 调用必须继续复用：

- `resolveApiUrl(...)`
- `resolveRuntimeHttpUrl(...)`
- `resolveRuntimeWebSocketUrl(...)`
- `serverRequest`
- `buildBearerAuthHeader(...)`

不应重新出现：

- 硬编码端口
- 裸 `fetch('/api/...')`
- 裸 `localStorage.getItem('token')` 解析

### 2. 插件页面与主页面必须分开理解

插件扩展页：

- 是 `/plugin/{plugin_id}/page/{page_path}` 页面资源

插件 runtime web APIs：

- 应该是未来的 `/api/Plugin/...` 能力接口

前端不能把这两个面混为一谈，否则后续 AstrBot 插件适配时一定会混乱。

### 3. 前端需要接受“某些能力目前只是 metadata，不是 executable”

尤其是 AstrBot 兼容能力部分，前端展示上必须允许出现以下状态：

- `registered_only`
- `deferred`
- `active`
- `disabled`
- `error`
- `unsupported`

这点非常关键，因为当前后端对很多兼容能力仍处于：

- cron / task 仍然是已保留元数据但未开放真实执行
- tools / web apis 已经是可执行能力，但前端仍应按后端状态字段渲染，而不是写死认为所有 compat 能力都“active”

如果前端直接显示成“已支持”，会误导用户。

## 推荐的信息架构调整

建议把当前页面做如下收敛：

| 现有页面 | 建议去向 |
| --- | --- |
| `dashboard/debug/http` | 升级为 `AI / Providers & Models` |
| `dashboard/debug/websocket` | 升级为 `AI / Chat` |
| 无 | 新增 `AI / Prompts` |
| 无 | 新增 `AI / Capabilities` |
| `dashboard/plugin` | 升级为 `Plugin Center / Installed` |
| `dashboard/extension` | 保留，但同时把入口挂回插件详情 |

## 推荐实施顺序

### P0

- 把 LLM chat 和 provider/model 管理从 `debug` 区域移出
- 新增 `AI / Capabilities` 只读页
- 升级插件中心信息架构，为插件类型区分预留 UI 位

### P1

- 后端补 prompt profile API 后，接 `AI / Prompts`
- 接插件 `Capabilities` / `RuntimeState` / `Diagnostics` Tab
- `/api/Plugin/List` 补 runtime/source/type 字段后，正式完成 AstrBot / Liteyuki 分类筛选

### P2

- 更完整的插件运行时诊断页
- cron / task 真正调度后的运行态页
- tools / mcp / skills 更深层的生命周期管理

## 最终结论

下一轮前端改造的重点不是“再堆几个页面”，而是把已经存在的后端能力重新整理成 4 条主线：

1. `AI / Chat`
2. `AI / Providers & Models`
3. `AI / Prompts`
4. `AI / Capabilities`

同时把插件区从“普通插件列表”升级成“插件中心”，并明确支持：

- AstrBot 插件与 LiteyukiBot 插件区分
- 扩展页入口回归插件详情
- runtime capability 面板预留

如果只做样式层改动，不先把这几条后端能力线承接起来，前端很快会再次出现：

- 页面归属混乱
- 前后端能力漂移
- 插件类型无法区分
- prompt 无法闭环管理
- tools / mcp / skills 后端已有、前端仍不可见
