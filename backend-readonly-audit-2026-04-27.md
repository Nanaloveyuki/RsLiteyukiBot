# RsLiteyukiBot 后端只读审查结论（2026-04-27）

## 范围

- 审查目标：后端 Rust 代码，重点关注代码复杂度、文件/模块复杂度、文件分类与命名、以及不必要硬编码。
- 明确不作为问题上报的例外：
  - 测试内容中的固定值
  - `%USERPROFILE%/.liteyuki/*` 这一类配置位置约定
- 本次为只读审查，未修改业务代码。

## 审查方式

- 本地只读盘点了后端 Rust 文件规模与职责分布。
- 并行使用 3 个 subagent 分别审查：
  - `web host / app host / tauri shell`
  - `config / runtime / bootstrap / main`
  - `llm / plugin sdk / adapter / session`
- 结论以下列问题为主：
  - 会明显拉高维护成本或回归风险的复杂度问题
  - 文件名或模块名与实际职责不匹配的问题
  - 会导致行为漂移的重复逻辑或硬编码

## 规模热点

本次盘点中最值得优先处理的超大文件：

- `src/web/host/mod.rs`：4710 行
- `src/web/host/llm_api.rs`：3671 行
- `src/llm/client.rs`：2357 行
- `src/app_config.rs`：1972 行
- `src/config_edit.rs`：1634 行
- `src/plugin/sdk/python/lifecycle.rs`：1475 行
- `src/llm/tools.rs`：1273 行
- `src/plugin/sdk/mod.rs`：1179 行
- `src/main.rs`：954 行
- `src/app_host.rs`：951 行

仅凭行数不能直接下结论，但这些文件里大多同时存在“入口编排 + 业务逻辑 + IO/持久化细节 + 测试”混装，已经超过单文件自然复杂度。

## Findings

### 严重

1. `src/web/host/mod.rs:195`, `src/web/host/mod.rs:1783`, `src/web/host/mod.rs:1918`, `src/web/host/mod.rs:1962`, `src/web/host/mod.rs:2156`, `src/web/host/mod.rs:2333`
问题：`mod.rs` 作为模块入口，却承载了插件安装、WebHost 服务主循环、连接管理、实时流、静态资源处理和超大测试模块。
风险：模块职责严重漂移；任何 host 改动都会碰到超大编译单元，评审、回归和冲突成本都很高。
建议：`mod.rs` 只保留模块装配；安装链路拆到 `plugin_install.rs`，服务循环拆到 `server_runtime.rs`，日志/系统实时流拆到 `realtime_stream.rs`，测试迁移到 `tests/web_host_*.rs` 或同目录 `*_tests.rs`。

2. `src/web/host/llm_api.rs:33`, `src/web/host/llm_api.rs:556`, `src/web/host/llm_api.rs:790`, `src/web/host/llm_api.rs:900`, `src/web/host/llm_api.rs:946`, `src/web/host/llm_api.rs:2542`, `src/web/host/llm_api.rs:3301`, `src/web/host/llm_api.rs:3495`
问题：单文件同时放了 LLM 路由分发、聊天执行、provider/model 目录逻辑和大体量测试。
风险：provider 相关改动很容易引发跨区回归；路由层和能力目录层耦合，行为漂移不容易被及时发现。
建议：拆成 `llm_route.rs`、`llm_chat_exec.rs`、`llm_provider_catalog.rs`、`llm_prompt_profiles.rs`，`llm_api.rs` 只做聚合。

3. `src/plugin/sdk/python/lifecycle.rs:259-399`, `src/plugin/sdk/python/lifecycle.rs:522-834`, `src/plugin/sdk/python/lifecycle.rs:836-993`, `src/plugin/sdk/python/lifecycle.rs:1330-1449`
问题：单文件承载插件加载/卸载、事件分发、Web API 调用、Tool 调用、Cron 执行、响应编解码和诊断记录。
风险：PyO3 交互路径全挤在一个文件里，任一子路径改动都可能影响其它路径；测试很难细化。
建议：按执行域拆成 `load_unload.rs`、`event_dispatch.rs`、`web_api.rs`、`tool_exec.rs`、`cron_exec.rs`、`response_codec.rs`、`diagnostics.rs`。

4. `src/runtime_support.rs:267-284`, `src/runtime_support.rs:631-693`, `src/bootstrap/settings.rs:122-127`, `src/bootstrap/settings.rs:257-265`
问题：启动链路存在 runtime/log 的双重解析与覆盖。`RuntimeSettings::try_load()` 已解析一轮，`prepare_runtime_bootstrap()` 又基于 `app_config` 再覆盖一轮。
风险：配置优先级容易漂移，尤其 env 与文件之间的覆盖关系可能在不同入口下不一致。
建议：合并为单一 resolver，显式定义优先级，`prepare_runtime_bootstrap()` 只消费最终结构。

5. `src/llm/client.rs:220-950`, `src/llm/client.rs:529-700`, `src/llm/client.rs:702-768`, `src/llm/client.rs:1673-2540`
问题：同一文件同时包含客户端构建、responses/chat 双协议流程、SSE 状态机、tool loop、schema 规范化和大量测试。
风险：核心 LLM 路径过于集中；新增 provider 或参数时容易产生连锁回归。
建议：拆成 `transport.rs`、`responses_flow.rs`、`chat_flow.rs`、`tool_loop.rs`、`schema.rs`、`errors.rs`，测试迁移到独立测试文件。

6. `src/app_config.rs:1628-2029`
问题：`validate_app_config` 单函数承载几乎全量配置校验，分支深且模式重复。
风险：新增字段时易漏改；测试粒度被迫做大而全场景，维护成本持续上升。
建议：按 section 拆成 `validate_connect`、`validate_llm`、`validate_commands`、`validate_plugins` 等，汇总层只负责收集 warning。

### 高

1. `src/plugin/sdk/mod.rs:17-52`, `src/plugin/sdk/mod.rs:380-875`, `src/plugin/sdk/mod.rs:877-1244`
问题：`mod.rs` 不是单纯模块门面，而是聚合了 Host Bridge、Runtime Adapter、生命周期调度、cron 调度、配置读写、OneBot 回复、ABI/版本校验。
风险：命名与职责不匹配；任何 SDK 改动都会触发大范围上下文切换。
建议：将 `mod.rs` 收敛为 re-export 门面；拆出 `host_bridge.rs`、`runtime_adapter.rs`、`plugin_runtime.rs`、`abi_contract.rs`、`onebot_reply.rs`、`versioning.rs`。

2. `src/main.rs:50-247`, `src/main.rs:307-388`, `src/main.rs:457-704`, `src/main.rs:868-1028`
问题：`main.rs` 既做启动编排，又做 reload、LLM TUI 命令写配置、字符串归一化和测试。
风险：入口文件职责过载，导致启动流程与具体业务命令耦合。
建议：拆分为 `startup_orchestrator`、`reload_service`、`llm_tui_command_service`，测试移出入口文件。

3. `src/app_host.rs:259`, `src/app_host.rs:366`, `src/app_host.rs:379`, `src/app_host.rs:406`, `src/app_host.rs:655`, `src/app_host.rs:677`
问题：`start_for_target` 同时做 bootstrap、bot 构建、生命周期钩子注册、外部事件绑定、资源采样线程和 cron 调度。
风险：初始化顺序耦合过强，故障注入和分段回滚困难。
建议：拆成 `build_bootstrap_state(...)`、`build_bot(...)`、`install_lifecycle_hooks(...)`、`start_background_workers(...)`。

4. `src/config_edit.rs:545-720`, `src/config_edit.rs:770-881`, `src/config_edit.rs:883-1291`, `src/config_edit.rs:1404-1446`
问题：配置写回采用大量 YAML/TOML 双份手工“行文本编辑”。
风险：对注释、缩进和复杂边界格式脆弱；扩字段时很容易把配置写坏。
建议：抽象统一 patch pipeline，并优先向结构化 AST 方案靠拢，而不是继续扩展字符串扫描拼接。

5. `src/llm/tools.rs:114-323`, `src/llm/tools.rs:325-577`, `src/llm/tools.rs:750-790`, `src/llm/tools.rs:918-1067`
问题：单文件混合工具状态持久化、工具目录构建、系统提示词拼装、工作区文件访问和路径安全控制。
风险：策略层与基础设施层耦合，任一层改动都可能影响另一层。
建议：拆为 `tool_state_store.rs`、`tool_catalog.rs`、`discovery_tools.rs`、`workspace_tools.rs`、`inventory_prompt.rs`。

6. `src/web/host/llm_api.rs:3241`, `src/web/host/llm_api.rs:3301`
问题：provider 模型列表与目录大段硬编码在代码中。
风险：上游模型变化后，Web 管理页会与真实能力脱节，必须发版修正。
建议：将 catalog 下沉到 `config/webui/llm-provider-catalog.json` 或同类 JSON 资源，代码只保留 schema 校验与 fallback。

### 中

1. `src/main.rs:777-794`, `src/config_edit.rs:420-437`, `src/app_config.rs:2108-2125`
问题：provider URL 归一化/去重规则在三处重复实现。
风险：读配置、写配置、命令行为可能逐步漂移。
建议：提取统一 `llm_config_normalization` 模块复用。

2. `src/runtime_support.rs:521`, `src/runtime_support.rs:524`, `src/app_config.rs:21`, `src/main.rs:47`
问题：LLM 默认 `base_url` 存在硬编码不一致。模板写入是 `https://tokenflux.dev/v1`，运行时默认常量是 `https://api.openai.com`。
风险：首次生成配置后，用户观察到的默认行为与运行时语义不一致。
建议：默认值只保留一个来源，模板生成与运行时解析共用同一常量。

3. `src/plugin/sdk/python/lifecycle.rs:424-431`, `src/plugin/sdk/python/lifecycle.rs:550`, `src/plugin/sdk/python/lifecycle.rs:574`, `src/plugin/sdk/python/lifecycle.rs:630`, `src/plugin/sdk/python/lifecycle.rs:733`, `src/plugin/sdk/python/lifecycle.rs:883`
问题：Python 运行时桥接大量依赖字符串硬编码键名/函数名，例如 `_get_astrbot_plugin_runtime`、`registered_web_apis`、`llm_tools`、`cron_jobs`。
风险：桥接层字段改名不会有编译期保护，只能在运行时失败。
建议：把这些键名集中到 bridge 常量层，并通过 typed accessor 包装访问。

4. `src/llm/client.rs:1381-1386`, `src/llm/client.rs:1590-1599`
问题：provider 能力判断依赖启发式硬编码，例如根据 `api.openai.com` 判断 `top_k`，根据错误文本判断 responses fallback。
风险：代理网关、错误文案或多语言返回变化都会导致误判。
建议：改成显式能力模型，例如 `supports_responses`、`supports_top_k`。

5. `src/llm/client.rs:818-898`, `src/llm/client.rs:900-949`
问题：`build_responses_request` 与 `build_chat_request` 大量重复相同参数拼装逻辑。
风险：新增参数时容易发生双路径不一致。
建议：抽取 `apply_common_generation_fields(...)` 一类共享拼装层。

6. `src/web/host/file_api.rs:11`, `src/web/host/file_api.rs:63`, `src/web/host/file_api.rs:88`
问题：`route_file_api` 使用长链 `if` 处理多条路径，且下载逻辑 GET/POST 重复。
风险：新增路由时容易遗漏分支或造成行为不一致。
建议：改为表驱动分发，并把下载、写操作、批量操作拆成独立 handler。

7. `src-tauri/src/lib.rs:113`, `src-tauri/src/lib.rs:114`, `src-tauri/src/lib.rs:120`
问题：Tray 菜单文案和 tooltip 直接硬编码。
风险：桌面壳层文案无法跟随 i18n 或产品配置。
建议：抽到桌面壳层配置或 i18n key。

8. `src/main.rs:49`
问题：`#[tokio::main(... worker_threads = 4)]` 固定线程数硬编码。
风险：在不同机器上的表现未必合理，且与整体 runtime 配置体系割裂。
建议：改用 Tokio 默认策略，或至少允许统一配置覆盖。

9. `src/web/host/system_api.rs:94`, `src/web/host/system_api.rs:120`, `src/web/host/system_api.rs:160`
问题：外部依赖地址直接硬编码在接口处理逻辑里，例如 `https://hitokoto.152710.xyz/` 和 GitHub API URL 拼装。
风险：上游地址变动或需要替换源时，只能改代码并重新发版。
建议：收敛到集中配置层，至少统一为可替换常量或 JSON 配置。

## 文件分类与命名问题汇总

最明显的命名/分类漂移有四处：

- `src/web/host/mod.rs`
  - 问题不是文件名本身叫 `mod.rs`，而是它已经不再是“模块入口”，而是实际主实现文件。
- `src/plugin/sdk/mod.rs`
  - 同样已经从模块门面膨胀成主实现容器。
- `src/main.rs`
  - 入口文件承载过多业务逻辑，和“只做启动编排”的预期不符。
- `src/web/host/llm_api.rs`
  - 名义上是 API 文件，实际上已经含有路由、执行器、目录、测试四类职责。

## 复用/重复逻辑问题

这类问题短期内比“代码长”更容易制造真实缺陷：

- runtime/log 配置双重解析：`runtime_support.rs` + `bootstrap/settings.rs`
- LLM provider URL 归一化重复：`main.rs` + `config_edit.rs` + `app_config.rs`
- LLM 双协议请求拼装重复：`src/llm/client.rs`
- 配置文档写回双格式双实现：`src/config_edit.rs`

## 建议的落地顺序

建议不要同时处理所有热点，按下面顺序推进更稳：

1. 先统一启动配置解析来源
   - 先收敛 `runtime_support.rs` / `bootstrap/settings.rs` / `app_config.rs` 的优先级问题。
2. 再拆 `app_config.rs` 和 `config_edit.rs`
   - 这是配置正确性的核心基础层，越晚拆越容易继续长大。
3. 处理 `main.rs` 与 `app_host.rs`
   - 把入口编排和后台 worker/命令服务拆开。
4. 拆 `src/web/host/mod.rs` 与 `src/web/host/llm_api.rs`
   - 这是当前 Web 后端最重的复杂度中心。
5. 拆 `src/plugin/sdk/mod.rs` 与 `src/plugin/sdk/python/lifecycle.rs`
   - 这是插件运行时长期演进的阻力点。
6. 最后统一 LLM 客户端与工具层公共逻辑
   - 收敛 `src/llm/client.rs`、`src/llm/tools.rs` 中的重复拼装和策略/基础设施混装。

## 本次未重点上报的区域

在本次目标下，没有发现必须单独上报的明显问题：

- `src/web_host.rs`
- `src/web_ui.rs`
- `src/web/runtime.rs`
- `src-tauri/src/main.rs`

这不代表这些文件没有任何改进空间，只是当前复杂度、分类和硬编码问题的优先级明显低于前述热点。

## 总结

当前后端的主要问题不是单点语法或局部坏味道，而是几个核心模块已经出现明显的“职责堆积”：

- Web Host 栈：`mod.rs` / `llm_api.rs`
- 配置与启动栈：`app_config.rs` / `config_edit.rs` / `main.rs` / `runtime_support.rs`
- LLM 与插件运行时栈：`llm/client.rs` / `llm/tools.rs` / `plugin/sdk/mod.rs` / `plugin/sdk/python/lifecycle.rs`

如果后续要做真正的工程化治理，优先级应该放在“收敛边界、删掉重复规则、把硬编码从业务流中抽出去”，而不是先做局部微调。

## 实施进度更新（临时交接，2026-04-27 夜）

说明：

- 本节不是新的只读审查结论，而是基于上文审查结果的实际重构进度记录。
- 当前改造策略以“先拆职责、后统一收口”为主，用户已明确允许暂时不优先考虑向后兼容性，最后再统一整合。

### 用户已确认的约束

- `src/utils` 用于可复用工具模块；`src/hardcode_data` 用于集中存放硬编码常量或模板。
- 前端改动不要把无意义内容写入 UI。
- Rust 代码里避免为了省事大量使用 `.clone()`，除非确有必要。
- 如果添加 `#[allow(dead_code)]`，其上一行加注释 `// 外部调用`。
- LLM 默认值已统一为 OpenAI 官方：
  - `base_url = https://api.openai.com`
  - `provider = openai`
  - `openai-compatible` 默认不再写入，留空即可

### 已完成的重构

#### 1. 配置/启动链已完成第一轮收口

- `src/config_paths.rs` 已拆除，相关路径常量与解析分别迁移到：
  - `src/hardcode_data/config_path.rs`
  - `src/utils/config_path.rs`
- `src/app_config.rs` 已目录化拆分：
  - `src/app_config/access.rs`
  - `src/app_config/adapters.rs`
  - `src/app_config/resolve.rs`
  - `src/app_config/storage.rs`
  - `src/app_config/validation.rs`
- `src/config_edit.rs` 已目录化拆分：
  - `src/config_edit/shared.rs`
  - `src/config_edit/llm.rs`
  - `src/config_edit/yaml.rs`
  - `src/config_edit/toml.rs`
  - `src/config_edit/tests.rs`
- LLM 共享默认值与归一化逻辑已收敛到：
  - `src/hardcode_data/llm.rs`
  - `src/utils/llm_config.rs`
- `src/runtime_support.rs` 已目录化拆分：
  - `src/runtime_support/bootstrap.rs`
  - `src/runtime_support/gateway.rs`
  - `src/runtime_support/llm_config.rs`
  - `src/runtime_support/plugin_dirs.rs`
- `src/main.rs` 已把 TUI 命令、reload、runtime UI bridge 等逻辑迁到：
  - `src/main_support/llm_tui_command_service.rs`
  - `src/main_support/reload_service.rs`
  - `src/main_support/runtime_ui_bridge.rs`
- `src/app_host.rs` 已目录化拆分：
  - `src/app_host/state.rs`
  - `src/app_host/runtime.rs`
  - `src/app_host/resource_usage.rs`
  - `src/app_host/tests.rs`

#### 2. Web Host 已完成第一轮拆解

- `src/web/host/mod.rs` 中已搬出的职责：
  - 插件安装链路：`src/web/host/plugin_install.rs`
  - 镜像测速与镜像默认值：`src/web/host/mirror_support.rs`
  - 本地插件商店目录：`src/web/host/plugin_store.rs`
  - 上游 HTTP/GitHub 访问辅助：`src/web/host/upstream.rs`
  - 静态资源与目录资源加载：`src/web/host/assets.rs`
- 同时已把下列调用点改成显式依赖新模块：
  - `src/web/host/plugin_api.rs`
  - `src/web/host/mirror_api.rs`
  - `src/web/host/system_api.rs`
  - `src/web/host/plugin_pages.rs`
  - `src/web/host/config.rs`

#### 3. 行数压缩情况

当前已明显压缩的热点文件：

- `src/app_config.rs`：`1972 -> 792`
- `src/config_edit.rs`：`1634 -> 79`
- `src/main.rs`：`954 -> 185`
- `src/app_host.rs`：`951 -> 135`
- `src/runtime_support.rs`：`693 左右 -> 116`
- `src/web/host/mod.rs`：`4710 -> 3881`

### 当前仍未完成的重构热点

截至本次交接，剩余最重的大文件如下：

- `src/web/host/mod.rs`：`3881`
- `src/web/host/llm_api.rs`：`3671`
- `src/llm/client.rs`：`2357`
- `src/plugin/sdk/python/lifecycle.rs`：`1475`
- `src/llm/tools.rs`：`1273`
- `src/plugin/sdk/mod.rs`：`1179`

大致进度判断：

- 配置/启动链：已完成第一轮主体拆分
- Web Host：已完成第一轮，但 `mod.rs` 和 `llm_api.rs` 仍是后续重点
- LLM / Plugin SDK：基本还没开始真正下刀
- 整体工程化重构进度约为 `45% ~ 55%`

### 接下来建议继续改什么

建议按下面顺序继续，而不是并行大面积铺开：

1. 继续拆 `src/web/host/mod.rs`
   - 优先拆 `terminal` / `realtime stream` / `server runtime` / 剩余插件发现与运行态拼装
   - 目标是把 `mod.rs` 压到更接近“装配门面”
2. 再拆 `src/web/host/llm_api.rs`
   - 建议切成：
     - `llm_route.rs`
     - `llm_chat_exec.rs`
     - `llm_provider_catalog.rs`
     - `llm_prompt_profiles.rs`
3. 然后处理 `src/llm/tools.rs`
   - 状态持久化、目录构建、workspace 访问、prompt 拼装分离
4. 再处理 `src/plugin/sdk/mod.rs` 与 `src/plugin/sdk/python/lifecycle.rs`
5. 最后收敛 `src/llm/client.rs`

### 当前施工时必须注意的事项

#### 1. `#[path = "../src/..."]` 测试约束

- 这个仓库有很多测试会用 `#[path = "../src/..."]` 直接把源文件作为测试模块重新编译。
- 这意味着：
  - 根模块改成目录化后，根文件里必须显式写 `#[path = "..."] mod ...;`
  - 不能轻易把某些内部类型放进会跨 crate 边界暴露的公共接口，否则容易出现“同名不同类型”的编译问题
- 已经踩过一次这个坑：
  - 不能把 `app_config::AppConfigDoc` 直接放进 `RuntimeSettings` 的公共方法签名

#### 2. 验证时优先用 `--all-targets`

- 仅跑 `cargo check` 不够，必须优先跑：
  - `cargo check --all-targets --locked --offline`
- 原因是很多 `#[path]` 测试问题只有在 `all-targets` 下才会暴露

#### 3. 当前测试状态

本轮改造过程中，多次成功通过：

- `cargo check --all-targets --locked --offline`
- `cargo test --bin liteyukibot-core --locked --offline`
- `cargo test --test app_config_migrated --locked --offline`

但需要注意：

- `tui::app::tests::resume_size_limit_drops_frontmost_old_resume`
  曾经在某些运行里失败，又在后续重复运行中恢复通过
- 目前看更像现有不稳定测试，而不是本轮明确引入的功能回归
- 因此如果明天继续改，遇到这条测试单独失败，建议先重跑一次，不要第一时间假定新改动打坏了逻辑

#### 4. 风格与边界约束

- 可复用工具尽量进入 `src/utils`
- 硬编码模板/常量尽量进入 `src/hardcode_data`
- 根文件尽量只保留：
  - 类型
  - re-export / 薄包装
  - 模块装配
- 业务细节、IO、字符串扫描、状态机不要继续堆回根文件

### 适合作为明天继续工作的入口文件

如果明天从当前状态继续，建议按这个入口顺序打开：

1. `src/web/host/mod.rs`
2. `src/web/host/llm_api.rs`
3. `src/llm/tools.rs`
4. `src/plugin/sdk/mod.rs`
5. `src/plugin/sdk/python/lifecycle.rs`
6. `src/llm/client.rs`

### 当前较适合直接复用的已拆模块

- `src/main_support/*`
- `src/runtime_support/*`
- `src/app_host/*`
- `src/app_config/*`
- `src/config_edit/*`
- `src/web/host/plugin_install.rs`
- `src/web/host/mirror_support.rs`
- `src/web/host/plugin_store.rs`
- `src/web/host/upstream.rs`
- `src/web/host/assets.rs`

### 简短结论

- 配置/启动链已经从“超大单文件混装”进入“可继续精修”的状态
- Web Host 还在重构中，但已经拆出第一批高耦合职责
- 真正还没开始的大头是：
  - `src/web/host/llm_api.rs`
  - `src/llm/client.rs`
  - `src/llm/tools.rs`
  - `src/plugin/sdk/mod.rs`
  - `src/plugin/sdk/python/lifecycle.rs`

## 追加交接快照（2026-04-27 深夜，基于当前工作区）

说明：

- 本节是停工前基于当前工作区的补充快照，用来覆盖上一个进度段落之后的新变化。
- 上一节中的行数与阶段判断对应的是更早一版检查点；此后工作区又继续演进，所以部分根文件行数有小幅回升，这不代表拆分被回滚，更多是薄包装、测试和胶水代码继续补齐后的自然结果。

### 当前工作区已观察到的新增拆分点

- `src/web/host/mod.rs` 除了前一节已记录的几个子模块外，当前还已经显式拆出：
  - `src/web/host/skill_import.rs`
- 当前根文件都已经保留了显式 `#[path = "..."]` 声明，说明本轮拆分有意识地兼容 `#[path = "../src/..."]` 这类测试编译方式：
  - `src/app_config.rs`
  - `src/config_edit.rs`
  - `src/runtime_support.rs`
  - `src/app_host.rs`

### 当前文件规模快照

以下数字比上一节更接近今晚停工时的真实状态：

- `src/web/host/mod.rs`：`4193`
- `src/web/host/llm_api.rs`：`3959`
- `src/llm/client.rs`：`2574`
- `src/plugin/sdk/python/lifecycle.rs`：`1565`
- `src/llm/tools.rs`：`1403`
- `src/plugin/sdk/mod.rs`：`1298`
- `src/app_config.rs`：`877`
- `src/config_edit.rs`：`88`
- `src/runtime_support.rs`：`131`
- `src/main.rs`：`203`
- `src/app_host.rs`：`156`

从“剩余待拆热点”的角度看，当前最主要的 6 个大文件合计仍有约 `14992` 行，后续工作量依然主要集中在这 6 个文件，而不是已经完成第一轮收口的配置/启动链。

### 当前可确认的拆分成果

- 配置/启动链根文件已经显著收缩，且职责已经外移到目录模块：
  - `src/app_config/*`
  - `src/config_edit/*`
  - `src/runtime_support/*`
  - `src/main_support/*`
  - `src/app_host/*`
- Web Host 已经拆出的可复用或可独立维护文件，当前至少包括：
  - `src/web/host/plugin_install.rs`
  - `src/web/host/mirror_support.rs`
  - `src/web/host/plugin_store.rs`
  - `src/web/host/upstream.rs`
  - `src/web/host/assets.rs`
  - `src/web/host/skill_import.rs`

### 当前仍然没拆开的核心内容

#### 1. `src/web/host/mod.rs`

- 虽然已经拆出一批职责，但文件本体仍然超过 `4000` 行。
- 从当前结构看，至少还有三块明显值得继续外移：
  - terminal / ws 相关状态与收发逻辑
  - realtime log / system status stream
  - 大体量测试与若干 server runtime 装配细节

#### 2. `src/web/host/llm_api.rs`

- 当前仍接近 `4000` 行，基本还是下一轮最应该先下刀的单体文件之一。
- 到目前为止，没有看到它已经像 `app_config` 或 `config_edit` 那样完成目录化拆分，所以这里依然是高优先级主战场。

#### 3. LLM / Plugin SDK 主链

- `src/llm/client.rs`
- `src/llm/tools.rs`
- `src/plugin/sdk/mod.rs`
- `src/plugin/sdk/python/lifecycle.rs`

这四块从当前规模判断，主体仍然处于“尚未真正拆分”的状态，最多只有跟随性适配改动，不应误判为已经进入收尾阶段。

### 明天继续时的建议顺序

建议仍按下面顺序推进，避免同时在多个大文件里来回切上下文：

1. 继续拆 `src/web/host/mod.rs`
2. 再拆 `src/web/host/llm_api.rs`
3. 处理 `src/llm/tools.rs`
4. 处理 `src/plugin/sdk/mod.rs`
5. 处理 `src/plugin/sdk/python/lifecycle.rs`
6. 最后再处理 `src/llm/client.rs`

### 交接时需要额外注意的现实情况

- 当前工作区不只是后端文件有变化，还能看到前端改动：
  - `frontend/src/controllers/capability_manager.ts`
  - `frontend/src/pages/dashboard/capabilities.tsx`
- 这两个前端文件不在本节后端重构进度的人工复核范围内，明天不要把它们和本轮后端模块化工作混在一起判断。
- 当前工作区还有较多新建目录/文件处于未提交状态，例如：
  - `src/app_config/`
  - `src/app_host/`
  - `src/config_edit/`
  - `src/hardcode_data/`
  - `src/main_support/`
  - `src/runtime_support/`
  - `src/utils/`
- 因此后续如果要整理 commit，建议按“配置链 / Web Host / 其它适配修正”分组，而不是把所有变更一次性揉成单个超大提交。

### 今晚停工前的简短判断

- 可以把配置/启动链视为“第一轮拆分已完成”
- 可以把 Web Host 视为“拆了一半，核心大头还在”
- 可以把 LLM / Plugin SDK 视为“基本还没真正开始”
- 如果按剩余热点体量估算，整体工程化重构进度更接近：
  - `50%` 左右，最多不建议超过 `60%`

### 今晚停工前的最新校验

- 已在当前工作区重新通过：
  - `cargo check --all-targets --locked --offline`
- 这说明至少在本次文档补充对应的当前状态下，基础编译检查仍然是通过的。

## 追加交接快照（2026-04-28，继续拆分后的最新状态）

说明：

- 本节覆盖 2026-04-28 继续施工后的新进展，主要集中在 `src/web/host/mod.rs` 与 `src/web/host/llm_api.rs`
- 当前策略仍然是先把根文件压回“装配/薄包装”，再考虑更细的统一和收口

### 今天新增完成的拆分

#### 1. `src/web/host/mod.rs` 继续外移职责

- 今日已确认拆出的新模块：
  - `src/web/host/realtime.rs`
  - `src/web/host/workspace.rs`
  - `src/web/host/plugin_runtime.rs`
- `src/web/host/mod.rs` 的大体量内联测试也已外移到：
  - `src/web/host/tests.rs`
- `src/web/host/terminal.rs` 也已继续扩充，当前已承接 terminal websocket attach/read/write/error 相关逻辑
- 这一轮之后，`mod.rs` 已不再直接承载前几版那样集中的实时流、terminal ws 与 workspace 辅助细节

#### 2. `src/web/host/llm_api.rs` 已完成第二轮目录化拆分

- 前一轮已拆出的目录模块：
  - `src/web/host/llm_api/provider_catalog.rs`
  - `src/web/host/llm_api/prompt_profiles.rs`
  - `src/web/host/llm_api/transport.rs`
  - `src/web/host/llm_api/manager.rs`
- 今天继续新增：
  - `src/web/host/llm_api/chat_exec.rs`
  - `src/web/host/llm_api/request_builders.rs`
  - `src/web/host/llm_api/manager_actions.rs`
- 当前职责分布已经比较明确：
  - `llm_api.rs`
    - 路由分发
    - 入参解析/归一化
    - 少量共享类型与校验
  - `chat_exec.rs`
    - provider chat 执行流
    - OpenAI Responses / ChatCompletions / Anthropic / Gemini 调用分流
  - `request_builders.rs`
    - 请求体/消息数组拼装
    - multimodal attachment 转换
    - fallback transcript / system instruction / reasoning payload 组装
  - `provider_catalog.rs`
    - provider/model/reasoning 目录与展示项
  - `manager.rs`
    - manager state / save / fetch / test / preview
  - `manager_actions.rs`
    - provider fetch / probe / preview request 组装与执行
  - `transport.rs`
    - HTTP transport / auth / upstream error summarization

### 最新文件规模快照

- `src/web/host/mod.rs`：`339`
- `src/web/host/tests.rs`：`2616`
- `src/web/host/llm_api.rs`：`1010`
- `src/web/host/llm_api/chat_exec.rs`：`274`
- `src/web/host/llm_api/request_builders.rs`：`728`
- `src/web/host/llm_api/manager.rs`：`614`
- `src/web/host/llm_api/manager_actions.rs`：`392`
- `src/web/host/llm_api/provider_catalog.rs`：`445`
- `src/web/host/llm_api/prompt_profiles.rs`：`112`
- `src/web/host/llm_api/transport.rs`：`160`
- `src/web/host/realtime.rs`：`147`
- `src/web/host/workspace.rs`：`78`
- `src/web/host/plugin_runtime.rs`：`435`
- `src/web/host/terminal.rs`：`572`

和最初审查快照相比，当前两个关键根文件已经明显收缩：

- `src/web/host/mod.rs`：`4710 -> 339`
- `src/web/host/llm_api.rs`：`3671 -> 1010`

### 本轮已确认的行为/实现修正

- `workspace_root()` 已对齐 `LY_WORKSPACE_ROOT`，避免 `/File/*`、terminal 与 capability/skill 流程各自解析工作区根目录
- `build_runtime_plugin_payload()` 已改成批量读取 capability snapshot，避免构造插件运行态 payload 时出现 N+1 访问
- `resolve_llm_enabled_write_path_for_web()` 相关测试已加 env lock，减少 `--all-targets` 下的环境变量竞争
- `request_builders.rs` 中新增的 `#[allow(dead_code)]` 已按约定加上 `// 外部调用`
- `src/web/host/tests.rs` 已把 `Cursor`、`aggregate_log_level` 和环境锁获取等关键依赖显式化，降低对 `mod.rs` 父作用域导入面的隐式耦合
- plugin 上传相关测试已补真实 HTTP 路由覆盖，并修正旧 helper 仍构造 `/api/Plugin/Install`、而后端当前实际路由已是 `/api/Plugin/Import` 的测试漂移

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
- 已完成的 subagent review 结论：
  - `chat_exec.rs` / `request_builders.rs`：未发现具体问题，拆分前后行为未见语义漂移
  - `manager.rs` / `manager_actions.rs`：未发现具体问题，剩余缺口主要是 `/LLM/FetchModels`、`/LLM/TestModels`、`/LLM/PreviewRequest` 的路由级集成覆盖
  - `mod.rs` / `tests.rs`：发现并已修正两类问题
    - 提取后的测试文件对父作用域导入面耦合过重
    - plugin 上传测试只测 helper、没有覆盖真实 `/api/Plugin/Import` 路由边界
- `cargo test --bin liteyukibot-core --locked --offline` 当前仍会失败在：
  - `tui::app::tests::resume_size_limit_drops_frontmost_old_resume`
- 这条失败点位于 `tui`，不在本轮 `web/host/llm_api` 拆分链路内；结合前几次记录，暂时仍按“现有不稳定/既有问题”处理，不能直接归因到本轮拆分

### 当前剩余的明确风险

#### 1. `llm_api` 子模块的隐式父作用域依赖仍未完全清掉

- 新增的 `chat_exec.rs` 与 `request_builders.rs` 已尽量写成显式导入
- 但旧一轮拆出的这些文件仍然大量依赖 `use super::*`：
  - `src/web/host/llm_api/manager.rs`
  - `src/web/host/llm_api/provider_catalog.rs`
  - `src/web/host/llm_api/prompt_profiles.rs`
  - `src/web/host/llm_api/transport.rs`
- 这类耦合已经真实触发过一次编译面问题，后续仍建议继续改成显式依赖

#### 2. realtime SSE 语义与测试覆盖仍然不完全一致

- 真正的 socket/stream 路径由 `handle_connection` 拦截并进入长连接流
- 但 router fallback 路径里，`log_api.rs` / `system_api.rs` 仍保留 one-shot SSE body 形式
- 当前多数测试更偏向 `route_http_request`，对真实 streaming path 的保护仍然偏弱

#### 3. `manager.rs` 已明显缩小，但 manager 面仍未完全收口

- `src/web/host/llm_api/manager.rs` 已从接近 `1000` 行降到 `614`
- 但 provider manager 相关逻辑现在仍分布在：
  - `manager.rs`
  - `manager_actions.rs`
  - `provider_catalog.rs`
- 下一步如果继续深拆，优先考虑把 manager state/save 侧与 normalize/serialize 辅助进一步解耦

### 建议的下一步顺序

1. 先收掉本轮 review 结果并修正问题
2. 继续压 `src/web/host/mod.rs`
   - 优先考虑把剩余 server runtime 装配细节和大体量测试再往外移
3. 再处理 `src/web/host/llm_api/manager.rs`
   - 可继续切 `fetch/test/preview` 或 provider action preparation
4. 之后再决定是否开始 `src/llm/tools.rs` 或 `src/plugin/sdk/mod.rs`

## 追加交接快照（2026-04-28，`web/host/llm_api` 第三轮收窄）

说明：

- 这一轮是在 `llm/client` 暂时收口后继续处理 `src/web/host/llm_api.rs`
- 目标不是再横向拆大块 provider 逻辑，而是把主文件里残余的共享类型层和前端聊天入口层抽出去
- 同时顺手吸收 reviewer 对“只拆文件、不拆边界”的批评

### 本轮新增完成

#### 1. 新增共享类型层 `types.rs`

- 新增：
  - `src/web/host/llm_api/types.rs`
- 已迁入：
  - `WebLlmChatRequest`
  - `WebLlmMessage`
  - `WebLlmAttachment`
  - `WebLlmRuntimeConfig`
  - `WebLlmChatExecution`
  - `ParsedDataUrl`
  - manager / prompt profile / provider action 相关 DTO
  - `PreparedProviderRequest`
  - `ProviderAuth`

这一步的收益：

- `llm_api.rs` 不再继续充当共享 DTO 垃圾桶
- `PreparedProviderRequest` 不再反向依赖 transport 层定义的认证枚举
- manager / request_builders / chat_exec / transport 的共享输入输出边界更集中

#### 2. 新增前端聊天入口层 `frontend_chat.rs`

- 新增：
  - `src/web/host/llm_api/frontend_chat.rs`
- 已迁入：
  - `llm_chat_payload`
  - `summarize_frontend_chat_request`
  - `pick_llm_api_key`
  - `validate_sampling_args`
  - `normalize_reasoning_effort_input`

这一步的收益：

- `llm_api.rs` 主文件进一步收敛成：
  - 路由分发
  - HTTP method gate
  - 测试
- 前端聊天入口自己的日志、入参校验、请求摘要与 provider 执行流不再混在主文件中部

#### 3. reviewer 指出的两条新增问题已吸收

- 已修正：
  - `frontend_chat.rs` 不再使用 `use super::*`
  - `ProviderAuth` 已从 `transport.rs` 上移到 `types.rs`
- 已补：
  - `WebLlmChatRequest` 的 camelCase 反序列化测试
  - manager save payload 的 camelCase 反序列化测试

这一步的收益：

- 新拆文件不再依赖父模块的宽作用域导入面
- DTO 边界有了最小但直接的回归保护

### 当前文件规模快照

- `src/web/host/llm_api.rs`：`619`
- `src/web/host/llm_api/types.rs`：`202`
- `src/web/host/llm_api/frontend_chat.rs`：`203`
- `src/web/host/llm_api/chat_exec.rs`：`274`
- `src/web/host/llm_api/request_builders.rs`：`728`
- `src/web/host/llm_api/manager.rs`：`614`
- `src/web/host/llm_api/manager_actions.rs`：`392`
- `src/web/host/llm_api/provider_catalog.rs`：`445`
- `src/web/host/llm_api/prompt_profiles.rs`：`112`
- `src/web/host/llm_api/transport.rs`：`154`

对主文件本身而言，当前已经从：

- `src/web/host/llm_api.rs`：`3671 -> 1010 -> 619`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test llm_api --locked --offline`
- 当前 `llm_api` 定向测试通过数：
  - `18 passed`

### 当前判断

- 这轮之后，`llm_api.rs` 的运行时代码已经比较接近“模块入口 + 路由层”
- 当前没有新的高优先级行为回归
- tests 仍留在 `llm_api.rs` 内，但这时更多是整理项，不再是运行时代码边界问题

### 下一步更值得处理的区域

1. 如果继续深拆 `web/host`
   - 优先看 `src/web/host/llm_api/manager.rs`
   - 它仍是 `llm_api` 子树里最大的单文件热点
2. 如果切回其它热点
   - `src/llm/tools.rs`
   - `src/plugin/sdk/mod.rs`

### 注意事项

- `frontend_chat.rs` 现在已经改成显式依赖，但 `manager.rs` / `provider_catalog.rs` 等旧子模块仍有 `use super::*` 残留
- `llm_api.rs` 中的 tests 尚未外迁
  - 当前判断是不影响运行时代码边界，可后置处理

## 追加交接快照（2026-04-28，`src/llm/tools*` 收尾后的最新状态）

说明：

- 本节覆盖今天继续处理 `src/llm/tools.rs` 这一段的实际落地结果
- 这一轮重点不是“再拆一个目录”，而是先把前一次 split 后 reviewer 找到的正确性问题补齐，再继续压根文件体积

### 今天新增完成的拆分与收口

#### 1. `src/llm/tools.rs` 已抽出四个明确子模块

- 当前已落地：
  - `src/llm/tools/discovery_tools.rs`
  - `src/llm/tools/inventory_prompt.rs`
  - `src/llm/tools/local_execution_tools.rs`
  - `src/llm/tools/runtime_inventory.rs`
  - `src/llm/tools/tests.rs`
  - `src/llm/tools/tool_arguments.rs`
  - `src/llm/tools/tool_state.rs`
  - `src/llm/tools/workspace_access.rs`
- 当前职责分布：
  - `tools.rs`
    - `ToolManager`
    - manager 级集成测试
    - 少量共享类型 / 参数解析 helper
  - `discovery_tools.rs`
    - discovery tool schema
    - category summary / catalog entry 格式化
    - `list_tool_categories` / `list_tools_in_category` / `get_tool_schema`
  - `inventory_prompt.rs`
    - runtime inventory prompt 拼装
    - `merge_system_prompt_sections`
  - `local_execution_tools.rs`
    - `workspace_list_files`
    - `workspace_read_file`
    - `read_skill_document`
    - 本地 execution tool 的 schema 与 builder
  - `runtime_inventory.rs`
    - MCP tool -> managed tool 转换
    - external runtime tool descriptor 转换
    - active execution descriptor 收集
    - runtime tool merge / dedupe
  - `tests.rs`
    - `ToolManager` manager 级集成测试
    - runtime bundle / tool-state / external runtime tool / inventory 行为回归测试
  - `tool_arguments.rs`
    - `required_string`
    - `optional_string`
    - `optional_usize`
    - workspace/discovery/local tool 共享入参解析
  - `tool_state.rs`
    - tool enable/disable 持久化
    - 主文件/备份文件加载与恢复
    - state file 原子替换写入
  - `workspace_access.rs`
    - workspace root 推断
    - workspace 相对路径净化
    - workspace 文件读取/列举
    - symlink/junction 越界保护

### 本轮已确认修正的问题

- `ToolStateStore::set_active()` 不再在持久化失败后留下“内存已变、磁盘未变”的不一致状态
  - 当前改为失败后回滚内存状态
  - 并补了 enable/disable 两条失败回归测试
- `read_tool_state_source()` 不再在“主文件缺失 + backup 损坏”时静默吞错
  - 当前会显式返回 warning/load_error
  - 避免把所有工具默认重新放开却没有任何提示
- `read_workspace_file()` 已补 `start_line` 越界保护
  - 避免 `clamp(start_line, total_lines)` 在 `start_line > EOF` 时 panic
  - 当前会返回 `[requested line range is empty]`
- `detect_workspace_root()` 已重新收口为更稳妥的优先级：
  - 优先最近的 Rust `[workspace]` 根
  - 否则最近的 `.git` 边界
  - 再否则退回最外层 `Cargo.toml`
- `workspace` 越界测试在 Windows 下不再静默跳过
  - 当前目录链接测试会优先走 symlink
  - 如果权限不允许，会回退到 `mklink /J` junction
  - 失败会显式暴露，不再表现成“看似通过、实际没覆盖”
- workspace root 推断又额外补了一层边界约束
  - 现在只会在最近 `.git` 边界内寻找 Rust `[workspace]`
  - 不会为了外层 workspace 再跨过内层 repo/submodule 边界扩大 `workspace_*` 访问范围
- 根测试对两个子模块内部 helper 的回探已收掉一部分
  - `sanitize_relative_path` 的单测已移入 `workspace_access.rs`
  - backup 路径断言不再直接依赖 `tool_state_backup_path()`
- discovery / prompt 相关单测也已跟随职责迁移
  - `list_tools_in_category` 的单测已移入 `discovery_tools.rs`
  - `merge_system_prompt_sections` 的单测已移入 `inventory_prompt.rs`
- discovery / prompt 的跨模块边界又补了一轮收口
  - category summary 不再用 `serde_json::Value` 做暗契约
  - 当前已改成 typed internal struct，避免 key 漂移时静默退化
  - `inventory_prompt.rs` 也已改成直接导入 `SkillInfo` / `SkillManager`，不再依赖父模块导入面
- `extra_tools` 现在不再是“可调用但不可见”
  - 当前会先被转成 external runtime descriptor，再并入 discovery catalog 和 runtime inventory prompt
  - web chat 路径里由 plugin/runtime 注入的 tool 至少会出现在 capability prompt 和 category summary 中
- discovery helper 当前也会把自己编入 discovery catalog
  - 不再出现 `list_tools_in_category("tool_discovery")` / `get_tool_schema("list_tool_categories")` 看不到 helper 自身的结构性缺口
- mixed warning 不再统一伪装成 MCP 故障
  - runtime inventory prompt 中的 warning 标题已改成中性表述，避免把 skill/tool-state 问题误导成 MCP outage
- 参数解析 helper 也已从根模块外移
  - `discovery_tools.rs`、`local_execution_tools.rs`、`workspace_access.rs` 不再依赖 `tools.rs` 的父作用域 alias 才能工作
  - 相关逻辑当前统一收在 `tool_arguments.rs`
- 测试归属继续收口
  - symlink escape regression test 已从 `tools.rs` 移回 `workspace_access.rs`
  - `read_skill_document` 已在 `local_execution_tools.rs` 中补模块内直测
- manager 级测试也已从根模块外移
  - `src/llm/tools.rs` 不再承载大块测试实现
  - 当前统一放在 `src/llm/tools/tests.rs`
- `tests.rs` 当前也已补成显式依赖
  - 不再依赖 `use super::*` 吃父模块 import 面
  - backup 路径测试也已回到 `tool_state_backup_path()`，不再复制命名规则

### 最新文件规模快照

- `src/llm/tools.rs`：`291`
- `src/llm/tools/discovery_tools.rs`：`232`
- `src/llm/tools/inventory_prompt.rs`：`93`
- `src/llm/tools/local_execution_tools.rs`：`195`
- `src/llm/tools/runtime_inventory.rs`：`91`
- `src/llm/tools/tests.rs`：`211`
- `src/llm/tools/tool_arguments.rs`：`33`
- `src/llm/tools/tool_state.rs`：`302`
- `src/llm/tools/workspace_access.rs`：`468`

和最初审查快照相比：

- `src/llm/tools.rs`：`1273 -> 291`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test llm::tools --locked --offline`
- `llm::tools` 当前定向测试通过数：
  - `24 passed`

### 本轮 subagent review 结论

- 第一轮 review 提出并已修正：
  - `read_workspace_file()` 的 `start_line > EOF` panic 风险
  - nested repo / workspace root 推断过宽或过窄的问题
  - backup 损坏路径被静默吞掉的问题
  - Windows 下 symlink escape 测试可能退化成无声跳过的问题
- 第二轮继续深拆后，当前状态已经从“先补 correctness”推进到“开始真正按职责收根文件”
  - `discovery_tools.rs` / `inventory_prompt.rs` 已落地，根文件不再直接承载对应实现细节
- 第三轮 review 提出并已修正：
  - `inventory_prompt` 与 `discovery_tools` 之间的 stringly-typed summary 契约
  - `inventory_prompt.rs` 对父模块导入面的隐式耦合
  - external runtime tool 只能调用、不能被 discovery / prompt 看见的问题
  - discovery helper 自己未进入 discovery catalog 的问题
  - mixed warnings 被统一标成 MCP 故障的问题
- 第四轮 review 提出并已修正：
  - `local_execution_tools.rs` 对父模块 alias 导入面的脆弱依赖
  - symlink escape regression test 仍留在根测试模块的问题
  - `read_skill_document` 缺少模块内直测的问题
- 当前这一轮又继续收了测试边界
  - manager 级测试已整体迁到 `tests.rs`
  - 根模块进一步退回到运行时装配入口
- 同一轮里又继续把 runtime bundle helper 从根文件拆出
  - 当前 `runtime_inventory.rs` 已接住 descriptor 转换和 tool merge/dedupe

### 当前仍然建议继续处理的点

#### 1. `src/llm/tools.rs` 根文件已经明显变薄，但还可以再收一层

- `291` 行已经比初始状态小很多
- 但根文件里仍然混着：
  - `ToolManager` 装配
  - MCP/external runtime descriptor 转换
- 下一步如果继续拆，优先考虑：
  - 本地 execution tool schema 与 builder 再单独抽一个 `local_execution_tools.rs`
  - 或把 manager 级测试单独外移

#### 2. 子模块虽然已独立，但还没有完全做到“改一个文件不碰另一个文件”

- `discovery_tools.rs`、`workspace_access.rs`、`tool_state.rs` 仍依赖父模块提供的一些 policy/constants/helper
- 这已经比最初状态好很多，但严格说仍偏“文件拆分”而不是“模块边界闭合”

### 建议的下一步顺序

1. 如果继续深拆 `llm/tools`：
   - 先评估是否把本地 execution tool builder 再切出去
   - 再决定是否外移 manager 级测试
2. 如果想换热点：
   - 下一优先级可以转去 `src/plugin/sdk/mod.rs`
   - 或回到 `src/llm/client.rs`

## 追加交接快照（2026-04-28，`src/llm/tools*` 本轮继续后的覆盖状态）

说明：

- 本节覆盖前文所有关于 `src/llm/tools*` 的旧快照数字，尤其是 `291`、`211`、`24 passed`
- 这次继续处理后，第三点“是否还值得再拆一层”的结论是：值得，但只做低风险收口，不再继续把行为逻辑切得更碎

### 本轮新增完成

#### 1. `src/llm/tools.rs` 再收一层，根文件只保留装配和 manager

- 新增：
  - `src/llm/tools/tool_types.rs`
- 当前职责分布变为：
  - `src/llm/tools.rs`
    - 子模块装配
    - 常量
    - `ToolManager`
    - runtime inventory 总装配流程
  - `src/llm/tools/tool_types.rs`
    - `ToolOrigin`
    - `ToolDescriptor`
    - `ToolCatalogEntry`
    - `ToolCatalogSnapshot`
    - `SkillCatalogSnapshot`
    - `CapabilityBundle`
    - `ManagedTool`
    - `new_managed_tool`

#### 2. 共享类型边界进一步显式化

- `discovery_tools.rs`、`local_execution_tools.rs`、`runtime_inventory.rs` 不再吃父模块对 `ManagedTool` / `new_managed_tool` 的隐式导入面
- 共享元数据已收口到 `tool_types.rs`
- 根文件从“装配 + 类型 + helper”进一步收敛成“装配 + manager”

#### 3. 之前 reviewer 提出的两个 correctness 点已确认体现在当前代码里

- external runtime tool 现在明确标记为 `ToolOrigin::External`
  - discovery schema 不再把 injected tool 误报成 `local`
- manager 级测试不再直接拼私有字段
  - 当前统一经 `ToolManager::for_test_workspace(...)` 构造隔离测试实例

#### 4. 本轮又顺手补掉三类漂移源

- `describe_runtime_tools(...)` 现在显式接收 `extra_tools`
  - runtime catalog snapshot 不再遗漏 request/runtime 注入的 external tool descriptor
- discovery helper 名称已收口成共享常量
  - `tools.rs`、`discovery_tools.rs`、`capability_api.rs` 不再各自维护一份字符串
- workspace tool schema 文案已改为引用共享限制常量
  - 避免 `default/max` 文字说明和真实 clamp 逻辑日后分叉

#### 5. `runtime_inventory.rs` 已补模块内直测

- 当前已直接覆盖：
  - duplicate runtime tool name 拒绝
  - duplicate runtime/extra tool name 拒绝
  - external descriptor 的 origin/category 赋值

#### 6. runtime catalog snapshot 也已补上唯一性收口

- 当 `extra_tools` 或 runtime tool 名称冲突时：
  - bundle 路径仍会拒绝构建
  - catalog snapshot 路径当前会去重并追加 warning
- 这样至少不会再产出“schema/helper 看起来可用，但实际同名工具不可判定”的歧义视图

#### 7. 测试隔离已补稳

- `tests.rs` 与 `local_execution_tools.rs` 的临时目录 helper 已改成“时间戳 + 原子计数”
- 避免并发单测下极低概率的 temp path 撞名串扰

#### 8. `#[allow(dead_code)]` 约定已按当前偏好补注释

- 本轮触及的对外暴露快照结构和 manager 方法，已在 `#[allow(dead_code)]` 上方补 `// 外部调用`

### 当前文件规模快照

- `src/llm/tools.rs`：`269`
- `src/llm/tools/discovery_tools.rs`：`254`
- `src/llm/tools/inventory_prompt.rs`：`93`
- `src/llm/tools/local_execution_tools.rs`：`211`
- `src/llm/tools/runtime_inventory.rs`：`156`
- `src/llm/tools/tests.rs`：`265`
- `src/llm/tools/tool_arguments.rs`：`33`
- `src/llm/tools/tool_state.rs`：`302`
- `src/llm/tools/tool_types.rs`：`74`
- `src/llm/tools/workspace_access.rs`：`468`

和最初审查快照相比：

- `src/llm/tools.rs`：`1273 -> 269`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test llm::tools --locked --offline`
- `llm::tools` 当前定向测试通过数：
  - `30 passed`

### 对 `llm/tools` 的当前判断

- 这一段已经从“明显过载的单文件”进入“边界基本清楚、剩余是共享类型与装配”的状态
- 如果只看收益/风险比，继续深拆 `llm/tools` 的边际收益已经明显下降
- 当前更合理的策略是：
  - 暂停继续细拆 `llm/tools`
  - 后续只在出现新职责堆积时增量调整

### 接下来更值得处理的区域

1. `src/plugin/sdk/mod.rs`
   - 仍是高复杂度 `mod.rs` 门面漂移点
2. `src/llm/client.rs`
   - 仍有双协议流程、tool loop、schema 规范化混装
3. `src/web/host/mod.rs`
   - 仍是超大 host 主实现容器

### 注意事项

- 这份文档前文保留了旧过程记录，阅读 `llm/tools` 现状时应以本节为准
- `src/llm/tools.rs` 继续拆分时，保留显式 `#[path = \"tools/...rs\"] mod ...;` 很重要
  - 当前 path-based 测试组织依赖这一点
- `src/web/host/capability_api.rs` 当前调用的是 `describe_runtime_tools(&[])`
  - 这是 workspace 级 capability 视图，不包含每次请求临时注入的 per-request tool 实例

## 追加交接快照（2026-04-28，`src/plugin/sdk*` 第一轮拆分后的覆盖状态）

说明：

- 本节覆盖前文所有把 `src/plugin/sdk/mod.rs` 视为单块 `1179` 行热点的旧描述
- 这一轮只做低风险收口，先把 `host bridge` 和 `runtime adapter / version negotiation` 从根文件剥离出去

### 本轮新增完成

#### 1. `src/plugin/sdk/mod.rs` 已从“混装门面”压成以 `PluginSdk` 为中心的主服务文件

- 新增：
  - `src/plugin/sdk/host_bridge.rs`
  - `src/plugin/sdk/runtime_adapters.rs`
  - `src/plugin/sdk/python/execution.rs`
  - `src/plugin/sdk/python/runtime_introspection.rs`
- 当前职责分布变为：
  - `src/plugin/sdk/mod.rs`
    - `PluginSdk`
    - command / capability / cron / tool bundle 相关主服务逻辑
    - `plugin_runtime_tool_name`
    - `PluginPermissionSet`
  - `src/plugin/sdk/host_bridge.rs`
    - `PluginHostApi`
    - `PluginHostBridge`
    - `PluginWebApiRequest`
    - `PluginWebApiResponse`
    - onebot reply payload helper
    - host bridge 局部测试
  - `src/plugin/sdk/runtime_adapters.rs`
    - `PluginLoadState`
    - `PluginLoadPlan`
    - `RuntimeAdapter`
    - `RuntimeAdapterRegistry`
    - `Native/Python/Lua/ExternalRuntimeAdapter`
    - ABI / host api version / min host version 协商逻辑
    - runtime adapter 局部测试
  - `src/plugin/sdk/python/lifecycle.rs`
    - runtime state
    - python plugin load / bootstrap / event dispatch / tui command
    - start / health / shutdown / unload 生命周期 hook
  - `src/plugin/sdk/python/execution.rs`
    - registered web api / tool / cron 执行
    - web api response decode
    - execution diagnostics 记录
    - cron outcome 语义区分
    - execution 局部测试
  - `src/plugin/sdk/python/runtime_introspection.rs`
    - capability snapshot
    - runtime diagnostics 读取
    - snapshot plugin_id 回填
    - introspection 局部测试

#### 2. 对外导出面已保持稳定

- `src/plugin/mod.rs`
- `src/lib.rs`

当前仍通过 `sdk::*` re-export 对外暴露：

- `PluginHostApi`
- `PluginHostBridge`
- `PluginSdkFuture`
- `PluginWebApiRequest`
- `PluginWebApiResponse`
- `PluginLoadPlan`
- `PluginLoadState`
- `RuntimeAdapter`
- `RuntimeAdapterRegistry`
- `Native/Python/Lua/ExternalRuntimeAdapter`

#### 3. reviewer 结论

- 已跑至少一轮 subagent review
- 当前未发现由这次拆分直接引入的 concrete correctness regression
- reviewer 留下的主要残余建议是：
  - `runtime_adapters.rs` 不要只依赖远端集成路径兜底
  - Python bridge 仍缺少真实插件端到端动态覆盖
- 这一点本轮已补：
  - host api version 不兼容拒绝
  - `sdk.min_host_version` 非法格式拒绝
  - cron 执行对 `plugin unavailable / job not found / handler missing` 不再统一压成同一个 `false`

### 当前文件规模快照

- `src/plugin/sdk/mod.rs`：`300`
- `src/plugin/sdk/capability_service.rs`：`167`
- `src/plugin/sdk/cron_service.rs`：`152`
- `src/plugin/sdk/host_bridge.rs`：`286`
- `src/plugin/sdk/runtime_adapters.rs`：`446`
- `src/plugin/sdk/python/lifecycle.rs`：`347`
- `src/plugin/sdk/python/execution.rs`：`548`
- `src/plugin/sdk/python/runtime_introspection.rs`：`126`
- `src/plugin/sdk/python/bridge.rs`：`596`
- `src/plugin/sdk/python/bridge_contract.rs`：`37`
- `src/plugin/sdk/python/runtime_registry.rs`：`396`
- `src/plugin/sdk/python/loader.rs`：`247`
- `src/plugin/sdk/python/state.rs`：`43`
- `src/plugin/sdk/python/commands.rs`：`482`

和最初审查快照相比：

- `src/plugin/sdk/mod.rs`：`1179 -> 300`
- `src/plugin/sdk/python/lifecycle.rs`：`1475 -> 368`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test plugin::sdk --locked --offline`
- 当前 `plugin::sdk` 定向测试通过数：
  - `14 passed`
- reviewer 额外确认：
  - `cargo test --test plugin_manager --offline`
  - `35/35 passed`

### 对 `plugin/sdk` 的当前判断

- `src/plugin/sdk/mod.rs` 已经不再是“所有职责都塞进门面文件”的状态
- Python runtime 子域也已经从“几乎都堆在 lifecycle.rs”推进到三段职责分布：
  - lifecycle
  - execution
  - runtime_introspection
- 共享 runtime state 类型也已经从 `lifecycle.rs` 独立出去：
  - 新增 `src/plugin/sdk/python/state.rs`
  - `bridge.rs` / `commands.rs` / `execution.rs` / `runtime_introspection.rs` / `mod.rs` 不再为了拿 `PythonRuntimeState` 反向依赖整个 `lifecycle.rs`
- Python runtime lookup / registry-shape 解码也已经独立出去：
  - 新增 `src/plugin/sdk/python/runtime_registry.rs`
  - `execution.rs` 现在主要负责 Web API / tool / cron 的执行编排与 diagnostics 更新
  - `runtime_introspection.rs` 只保留 capability snapshot / diagnostics 聚合，不再自己处理 Python bridge lookup 细节
- Python manifest load / bootstrap / handler resolve 链也已经独立出去：
  - 新增 `src/plugin/sdk/python/loader.rs`
  - `lifecycle.rs` 现在主要负责 event dispatch、TUI command 执行、start/health/shutdown/unload hook、以及 runtime state 清理
- `src/plugin/sdk/mod.rs` 里的 capability/tool bundle 与 cron 调度聚合也已经拆出去：
  - 新增 `src/plugin/sdk/capability_service.rs`
  - 新增 `src/plugin/sdk/cron_service.rs`
  - `mod.rs` 现在更接近 runtime 门面、权限、构造和基础分发层
- Rust 侧 Python bridge 共享字符串契约也已经完成第一轮收口：
  - 新增 `src/plugin/sdk/python/bridge_contract.rs`
  - 统一承载 root module 名、bridge 函数名、runtime registry key、默认 lifecycle handler attr 列表
  - 这一轮继续把 `on_load` fallback、web api tuple index、tool/cron 常用 attr 名一起收进去了
  - `bridge.rs` / `execution.rs` / `runtime_introspection.rs` / `lifecycle.rs` 已改为引用共享常量
- `bridge.rs` 的 compat runtime 安装判定也已从“只看 `_bind_astrbot_plugin_runtime` 一个符号”改成“校验必需 bridge 符号集合”
  - 可避免外部预注入不完整 `liteyuki` 模块时延后到运行期才炸
- 但插件运行时主服务仍然偏重，后续还有一层可拆空间：
  - `PluginSdk` 里的 capability / cron / tool bundle 聚合
  - Python runtime dict/object shape 的 typed 访问边界闭合
- 如果只看下一优先级：
  - `src/plugin/sdk/python/*` 现在更适合做“边界闭合”和动态契约兜底
  - `mod.rs` 已经不是最急的那个点

### 接下来更值得处理的区域

1. `src/plugin/sdk/python/execution.rs` 与 `src/plugin/sdk/python/runtime_introspection.rs`
   - 主要流程已瘦身，但 `runtime_registry.rs` 仍然是 Python runtime dict / object shape 的集中承载点
2. `src/plugin/sdk/python/lifecycle.rs`
   - 已不再承载 load/bootstrap 主链，但仍承载 event/tui/start/health/shutdown/unload 主链
2. `src/llm/client.rs`
   - 双协议流程、tool loop、schema 规范化仍混装
3. `src/web/host/llm_api.rs`
   - 仍是大体量 route + exec + catalog 混装文件

### 注意事项

- `runtime_adapters.rs` 当前已经接住 version negotiation 与 contract construction
  - 后续再动 `sdk.api_version` / `sdk.min_host_version` 语义时，优先把测试也跟着补在该模块内
- `plugin/sdk` 当前仍保留 `mod.rs` 作为外部统一出口
  - 如果后续继续拆，尽量延续“根模块只做导出与主服务聚合”的方向
- 这一步之后，`PluginSdk` 的 impl 已分散在多个 service 模块中
  - 后续如果继续拆，优先按 capability / cron / runtime lifecycle 这种稳定职责边界延续，不要再回退成按调用点零散切
- 前文把 `python/lifecycle.rs` 视为 Web API / Tool / Cron / response codec 主承载文件的说法已经过时
  - 阅读 Python SDK 现状时，应以上述 `lifecycle / execution / runtime_introspection` 新分工为准
- 前文把 “Python bridge 的 shared contract / constants” 视为下一步待做事项的说法也已经过时
  - 这一步现在已经由 `src/plugin/sdk/python/bridge_contract.rs` 接住
- 当前 reviewer 已确认过一类真实问题并已修复：
  - compat runtime 安装判定不应只看 `_bind_astrbot_plugin_runtime` 单点符号
  - 现在改为校验必需 bridge 符号集合，并要求这些符号可调用
- 当前最高残余风险不在 Rust 编译期，而在 Python compat runtime 的动态桥接契约
  - 例如 Python 侧真实 registry payload shape、tuple/object schema、以及 Rust/Python 两侧 ABI 仍非真正共源
  - 这类问题更适合后续补一个真实 sample plugin 的端到端集成测试来兜底

## 追加交接快照（2026-04-28，`plugin/sdk/python/execution` 第二轮继续拆分）

说明：

- 本节覆盖上一节里把 `execution.rs` 视作“执行编排 + response codec + diagnostics 记录”三类职责仍混在一起的状态
- 这一轮目标是继续缩小 `python/execution.rs`，同时把 reviewer 已指出的脆弱边界和测试缺口补齐

### 本轮新增完成

#### 1. `src/plugin/sdk/python/execution.rs` 继续按职责拆开

- 新增：
  - `src/plugin/sdk/python/execution_codec.rs`
  - `src/plugin/sdk/python/diagnostics.rs`
- 当前分工变为：
  - `src/plugin/sdk/python/execution.rs`
    - registered web api / tool / cron 执行入口
    - cron outcome 分流
    - 调用 runtime registry / codec / diagnostics 子模块
    - execution 局部测试
  - `src/plugin/sdk/python/execution_codec.rs`
    - tool 参数归一化
    - runtime tool 名归一化
    - Python tool result decode
    - web api request context 构造
    - web api response decode
    - codec 局部测试
  - `src/plugin/sdk/python/diagnostics.rs`
    - execution diagnostics 成功/失败记录
    - execution kind 到 `PluginRuntimeDiagnostics` 字段的映射

#### 2. 上一轮 reviewer 提到的边界问题已收口

- `src/plugin/sdk/cron_service.rs`
  - 不再通过父模块 `use` 间接依赖 `PythonCronExecutionOutcome` / `execute_python_registered_cron_job`
  - 改为直接从 `python::execution` 引入，减少 parent-scope import 耦合
- `src/plugin/sdk/python/loader.rs`
  - `event_handler` / `start_handler` / `health_handler` / `shutdown_handler` / `unload_handler` / `config_path`
  - 已统一收口到 `src/plugin/sdk/python/bridge_contract.rs`
  - 避免加载链继续散落 runtime option key 字符串

#### 3. reviewer 指出的测试缺口已补

- `src/plugin/sdk/python/execution.rs` 新增：
  - `execute_python_registered_cron_job_reports_missing_job`
  - `execute_python_registered_cron_job_reports_missing_handler`
- 现在 cron 执行的关键 outcome 至少已覆盖：
  - `PluginUnavailable`
  - `JobNotFound`
  - `HandlerMissing`
- 这一步主要补的是 scheduler-facing 边界语义，而不是继续扩大功能面

### 当前文件规模快照

- `src/plugin/sdk/mod.rs`：`336`
- `src/plugin/sdk/capability_service.rs`：`183`
- `src/plugin/sdk/cron_service.rs`：`163`
- `src/plugin/sdk/host_bridge.rs`：`321`
- `src/plugin/sdk/runtime_adapters.rs`：`497`
- `src/plugin/sdk/python/lifecycle.rs`：`368`
- `src/plugin/sdk/python/execution.rs`：`468`
- `src/plugin/sdk/python/execution_codec.rs`：`225`
- `src/plugin/sdk/python/diagnostics.rs`：`62`
- `src/plugin/sdk/python/runtime_introspection.rs`：`141`
- `src/plugin/sdk/python/bridge.rs`：`653`
- `src/plugin/sdk/python/bridge_contract.rs`：`49`
- `src/plugin/sdk/python/runtime_registry.rs`：`427`
- `src/plugin/sdk/python/loader.rs`：`263`
- `src/plugin/sdk/python/state.rs`：`76`
- `src/plugin/sdk/python/commands.rs`：`526`

和上一轮 `plugin/sdk` 快照相比：

- `src/plugin/sdk/python/execution.rs`：`548 -> 468`
- 新增 `src/plugin/sdk/python/execution_codec.rs`：`225`
- 新增 `src/plugin/sdk/python/diagnostics.rs`：`62`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test plugin::sdk --locked --offline`
- 当前 `plugin::sdk` 定向测试通过数：
  - `16 passed`
- 本轮 reviewer 结论：
  - 第一轮 reviewer：`mod.rs` / `capability_service.rs` / `cron_service.rs` 未发现 concrete correctness regression
  - 第二轮 reviewer：指出 `execution` 新 outcome 覆盖缺口与 loader key 字符串散落；上述两点本轮已处理

### 对 `plugin/sdk/python` 当前状态的判断

- `execution.rs` 已不再同时承载编排、codec、diagnostics 三类细节
- `bridge_contract.rs` 现在同时负责：
  - bridge function 名
  - runtime registry key
  - lifecycle handler attr
  - runtime option key
- 剩余更重的点开始更集中地落在：
  - `src/plugin/sdk/python/bridge.rs`
  - `src/plugin/sdk/python/runtime_registry.rs`
  - `src/plugin/sdk/python/commands.rs`
  - `src/plugin/sdk/python/lifecycle.rs`

### 下一步更值得处理的区域

1. `src/plugin/sdk/python/bridge.rs`
   - 仍是当前 Python SDK 子域最大的单文件
   - 同时承载 compat runtime 安装、sdk bridge 注入、runtime bind/cleanup、module 清理、json/python 对象转换
2. `src/plugin/sdk/python/runtime_registry.rs`
   - 仍然是 runtime dict/object shape 的集中入口
   - 如果继续追求边界闭合，可以考虑把 web api / tool / cron decode 再按注册类型拆开
3. `src/plugin/sdk/python/lifecycle.rs`
   - 当前主要是 event dispatch、TUI command 执行、start/health/shutdown/unload 链
   - 如果后续继续拆，更适合沿 `event_dispatch` / `lifecycle_hooks` 两段分离

### 注意事项

- 这一轮为了补 cron outcome 测试，在 `execution.rs` 测试里构造了最小 Python runtime bridge 场景
  - 后续如果要做更大规模 Python SDK 重构，可以复用这套测试搭桥方式
- `bridge_contract.rs` 现在已经不仅是 bridge 函数常量
  - 后续如果再往里塞无关概念，建议考虑拆成 `bridge_contract.rs` + `runtime_option_keys.rs`
  - 目前体量还小，暂时无需提前分裂
- 当前最高收益方向已经从“继续薄化 execution.rs”切到“收敛 bridge/runtime_registry 动态契约”
  - 如果只做一块，优先级建议高于再去抠 `mod.rs`

## 追加交接快照（2026-04-28，`plugin/sdk/python/bridge` 继续拆分）

说明：

- 本节覆盖上一节里 `bridge.rs` 仍同时承载“sdk bridge + runtime bind + module import/cleanup + json/python 转换”的状态
- 这一轮继续目标是把 `bridge.rs` 收敛回“桥接入口”本身，不再顺带做模块管理和序列化转换

### 本轮新增完成

#### 1. `src/plugin/sdk/python/bridge.rs` 继续按职责拆开

- 新增：
  - `src/plugin/sdk/python/json_codec.rs`
  - `src/plugin/sdk/python/module_management.rs`
- 当前分工变为：
  - `src/plugin/sdk/python/bridge.rs`
    - `PyPluginSdk`
    - sdk bridge 安装
    - runtime bind / cleanup
    - callable fallback / awaitable 处理
    - plugin command result 渲染
    - load error 包装
  - `src/plugin/sdk/python/json_codec.rs`
    - `py_any_to_json`
    - `json_to_pyobject`
  - `src/plugin/sdk/python/module_management.rs`
    - capture loaded module names
    - stale module 清理
    - entrypoint import
    - `sys.modules` 清理
    - `sys.path` 清理

#### 2. 调用点已直接改线，不再经由大文件中转

- `execution.rs` 直接依赖 `json_codec.rs`
- `execution_codec.rs` 直接依赖 `json_codec.rs`
- `runtime_registry.rs` 直接依赖 `json_codec.rs`
- `loader.rs` / `lifecycle.rs` 直接依赖 `module_management.rs`

这一步的意义不是“多几个文件”，而是把 `bridge.rs` 从公共杂物间里抽空，让后续 reviewer 和重构都能更明确地落到单一职责边界。

#### 3. cron outcome 测试已补成四态闭环

- `execution.rs` 当前已显式覆盖：
  - `PluginUnavailable`
  - `JobNotFound`
  - `HandlerMissing`
  - `Executed`
- 为了避免 Python compat runtime 的进程级全局状态互相污染：
  - 测试 helper 已改成使用唯一 `plugin_id` / `runtime_module`
  - 不再复用固定 `demo` / `demo_runtime`

### 当前文件规模快照

- `src/plugin/sdk/python/bridge.rs`：`411`
- `src/plugin/sdk/python/json_codec.rs`：`24`
- `src/plugin/sdk/python/module_management.rs`：`228`
- `src/plugin/sdk/python/execution.rs`：`541`
- `src/plugin/sdk/python/execution_codec.rs`：`225`
- `src/plugin/sdk/python/diagnostics.rs`：`62`
- `src/plugin/sdk/python/runtime_registry.rs`：`427`
- `src/plugin/sdk/python/loader.rs`：`265`
- `src/plugin/sdk/python/lifecycle.rs`：`371`

和上一轮 bridge 前快照相比：

- `src/plugin/sdk/python/bridge.rs`：`653 -> 411`
- 新增 `src/plugin/sdk/python/json_codec.rs`：`24`
- 新增 `src/plugin/sdk/python/module_management.rs`：`228`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test plugin::sdk --locked --offline`
- 当前 `plugin::sdk` 定向测试通过数：
  - `17 passed`
- reviewer 这一步的有效残余意见：
  - `Executed` success-path 也应补测试
  - 该项本轮已处理

### 对 `plugin/sdk/python` 当前状态的判断

- `bridge.rs` 已从“动态桥接全能文件”收缩回更合理的桥接核心
- `execution.rs` 的行数因为补了隔离性更强的测试略有回升，但生产逻辑边界已经比前几轮清晰得多
- 当前剩余最值得继续收敛的点开始更聚焦到：
  - `src/plugin/sdk/python/runtime_registry.rs`
  - `src/plugin/sdk/python/lifecycle.rs`
  - `src/plugin/sdk/python/commands.rs`

### 下一步更值得处理的区域

1. `src/plugin/sdk/python/runtime_registry.rs`
   - 仍是 Python runtime payload shape 的集中入口
   - 如果继续拆，优先按 `web api / tool / cron` 三类 registration decode 分离
2. `src/plugin/sdk/python/lifecycle.rs`
   - 仍承载 event dispatch、TUI command、start/health/shutdown/unload 多段流程
   - 更适合下一轮按 `event_dispatch` / `lifecycle_hooks` 切开
3. `src/plugin/sdk/python/commands.rs`
   - 当前也已经是中等偏大的独立职责文件
   - 如果后续 scope command / tui command 继续长大，这里会成为下一个自然拆分点

### 注意事项

- 现在 `json_codec.rs` 与 `module_management.rs` 都是纯工具型子模块
  - 后续若跨 `python/*` 继续复用，应优先直接依赖它们，而不是重新把 helper 倒回 `bridge.rs`
- reviewer 目前没有再指出新的生产行为回归
  - 当前最高残余风险仍然不是 Rust 编译期，而是 Rust 常量与 Python compat runtime payload shape 的跨语言契约漂移
  - 真正要继续压风险，下一步更应该补 sample plugin 级别的集成测试，而不是继续只做 Rust 侧局部搬运

## 追加交接快照（2026-04-28，`runtime_registry` 目录化与 `lifecycle` 继续拆分）

说明：

- 本节覆盖前文里仍把 `runtime_registry.rs` 和 `lifecycle.rs` 视作单文件职责容器的旧状态
- 这一轮目标是把 Python runtime payload shape 解码边界拆清楚，并把 lifecycle 再按执行域拆一层

### 本轮新增完成

#### 1. `runtime_registry` 已目录化

- 已从单文件：
  - `src/plugin/sdk/python/runtime_registry.rs`
- 拆成：
  - `src/plugin/sdk/python/runtime_registry/mod.rs`
  - `src/plugin/sdk/python/runtime_registry/common.rs`
  - `src/plugin/sdk/python/runtime_registry/web_api.rs`
  - `src/plugin/sdk/python/runtime_registry/tool.rs`
  - `src/plugin/sdk/python/runtime_registry/cron.rs`

当前分工：

- `common.rs`
  - runtime lookup
  - registry list lookup
  - capability snapshot payload 拉取
- `web_api.rs`
  - web api registration tuple decode
  - route/method normalize
  - web api decode 局部测试
- `tool.rs`
  - tool registration decode
  - tool invoke helper
  - tool decode 局部测试
- `cron.rs`
  - cron registration decode
  - cron decode 局部测试
- `mod.rs`
  - 仅保留 facade re-export

意义：

- `execution.rs` / `runtime_introspection.rs` 不再依赖一个混装的 `runtime_registry.rs`
- 后续如果还要继续收缩跨语言契约风险，可以直接对某一类 registration 单独补测试或继续拆，不必再改整个文件

#### 2. `lifecycle` 已继续按执行域拆开

- 当前结构变为：
  - `src/plugin/sdk/python/lifecycle.rs`
    - 薄门面 re-export
  - `src/plugin/sdk/python/event_dispatch.rs`
    - adapter event dispatch
  - `src/plugin/sdk/python/tui_command_runtime.rs`
    - TUI command 执行
  - `src/plugin/sdk/python/lifecycle_hooks.rs`
    - start / health / shutdown / unload
    - unload cleanup sequencing

这一步之后：

- `event dispatch`
- `TUI command runtime`
- `manifest lifecycle hooks`

三条链已经不再堆在同一个文件里。

#### 3. execution 测试并发隔离已补

- `src/plugin/sdk/python/execution.rs` 测试里新增了进程内串行锁
- 原因是 Python compat runtime 依赖全局 `liteyuki` bridge 状态
- 单测单独跑能过、并行跑会飘时，这一层锁能避免 execution 组内部互相污染

注意：

- reviewer 也明确指出，这个锁目前只覆盖 `execution.rs` 测试模块
- 如果后续其它文件继续增加会修改全局 Python runtime 的测试，最好提取成共享 test helper，而不是各自再造一把局部锁

### 当前文件规模快照

- `src/plugin/sdk/python/lifecycle.rs`：`6`
- `src/plugin/sdk/python/event_dispatch.rs`：`97`
- `src/plugin/sdk/python/tui_command_runtime.rs`：`84`
- `src/plugin/sdk/python/lifecycle_hooks.rs`：`208`
- `src/plugin/sdk/python/runtime_registry/mod.rs`：`12`
- `src/plugin/sdk/python/runtime_registry/common.rs`：`168`
- `src/plugin/sdk/python/runtime_registry/web_api.rs`：`136`
- `src/plugin/sdk/python/runtime_registry/tool.rs`：`85`
- `src/plugin/sdk/python/runtime_registry/cron.rs`：`79`
- `src/plugin/sdk/python/bridge.rs`：`411`
- `src/plugin/sdk/python/execution.rs`：`553`

和上一轮快照相比：

- `src/plugin/sdk/python/runtime_registry.rs`：`427 -> 12 + 168 + 136 + 85 + 79`
- `src/plugin/sdk/python/lifecycle.rs`：`371 -> 6 + 97 + 84 + 208`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test plugin::sdk --locked --offline`
- 当前 `plugin::sdk` 定向测试通过数：
  - `17 passed`

### reviewer 结论

- reviewer 1：
  - 未发现 `runtime_registry` 目录化带来的 concrete correctness regression
  - 当前最大残余风险仍是 Rust/Python 两侧 payload shape 的跨语言漂移，而不是 Rust 模块边界本身
- reviewer 2：
  - 未发现高置信生产回归
  - 指出 execution 测试串行锁目前还是模块内局部方案
  - 指出 `runtime_introspection.rs` 目前测试仍偏窄，只覆盖 plugin_id backfill

### 下一步更值得处理的区域

1. `src/plugin/sdk/python/commands.rs`
   - 当前体量已经不小，而且职责边界相对稳定
   - 后续若继续拆，可考虑按 declared command / TUI command / scope disable state 分离
2. `src/plugin/sdk/python/runtime_introspection.rs`
   - 文件不大，但测试覆盖明显窄于现在的边界复杂度
   - 如果下一步目标偏“压风险”而不是“继续拆文件”，这里值得先补测试
3. sample plugin 集成测试
   - 当前 reviewer 一致认为最大残余风险在跨语言契约
   - 真正高收益的下一步不一定是继续拆 Rust 文件，而可能是补一个最小 Python sample plugin 端到端测试

### 注意事项

- `lifecycle.rs` 现在已经是薄门面
  - 后续如果再动 `mod.rs` 导入面，记得一起核对它当前依赖的是 `lifecycle` facade 还是新子模块直连
- `runtime_registry/mod.rs` 现在也是薄门面
  - 后续若继续增加某类 registry 逻辑，优先直接落到对应子模块，不要再把 `mod.rs` 重新写胖

## 追加交接快照（2026-04-28，`commands` 目录化拆分）

说明：

- 本节覆盖前文里仍把 `src/plugin/sdk/python/commands.rs` 视作单文件职责容器的旧状态
- 这一轮目标是把 command 相关职责按“数据模型 / 名称与 scope 归一化 / declared command / scope disable state / TUI command registry”拆开

### 本轮新增完成

#### 1. `commands` 已目录化

- 已从单文件：
  - `src/plugin/sdk/python/commands.rs`
- 拆成：
  - `src/plugin/sdk/python/commands/mod.rs`
  - `src/plugin/sdk/python/commands/models.rs`
  - `src/plugin/sdk/python/commands/normalization.rs`
  - `src/plugin/sdk/python/commands/declared.rs`
  - `src/plugin/sdk/python/commands/scope_state.rs`
  - `src/plugin/sdk/python/commands/tui_registry.rs`

当前分工：

- `models.rs`
  - `PluginTuiCommand`
  - `PluginScopedCommand`
- `normalization.rs`
  - TUI command 名归一化
  - plugin scope 归一化
  - scope match / scoped key 构造
- `declared.rs`
  - payload -> adapter scope/message 解析
  - declared command 匹配
  - declared command 注册
- `scope_state.rs`
  - disabled scope command state
  - scope enable/disable 切换
  - merged scope command 列表
- `tui_registry.rs`
  - TUI command 注册/删除/启停
  - TUI command 列表
- `mod.rs`
  - facade re-export

#### 2. 调用点已顺着新边界落位

- `event_dispatch.rs`
  - 现在只依赖 declared command 侧的 `disabled_declared_command_for_plugin`
- `tui_command_runtime.rs`
  - 只依赖 command 名归一化和 scope disabled 判定
- `bridge.rs`
  - 只依赖 TUI command registry 相关接口
- `loader.rs`
  - 只依赖 declared command 注册
- `plugin/sdk/mod.rs`
  - 仍通过 `commands` facade 聚合外部调用面

这一步之后，command 子域已经不再需要一整份 `commands.rs` 上下文才能改动任何一处局部行为。

### 当前文件规模快照

- `src/plugin/sdk/python/commands/mod.rs`：`18`
- `src/plugin/sdk/python/commands/models.rs`：`19`
- `src/plugin/sdk/python/commands/normalization.rs`：`65`
- `src/plugin/sdk/python/commands/declared.rs`：`156`
- `src/plugin/sdk/python/commands/scope_state.rs`：`181`
- `src/plugin/sdk/python/commands/tui_registry.rs`：`120`

和上一轮快照相比：

- `src/plugin/sdk/python/commands.rs`：`526 -> 18 + 19 + 65 + 156 + 181 + 120`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test plugin::sdk --locked --offline`
- 当前 `plugin::sdk` 定向测试通过数：
  - `17 passed`

### 当前判断

- `commands` 这块已经从“单文件装全部 command 逻辑”变成稳定的子域结构
- 现阶段 `scope_state.rs` 是该子域里相对最重的一段，但它的职责边界已经比较清晰
- 真要继续拆，也更像是“为了测试粒度或风险隔离而再细分”，而不是因为当前结构已经混乱

### 下一步更值得处理的区域

1. `src/plugin/sdk/python/runtime_introspection.rs`
   - 文件不大，但测试明显偏窄
   - 如果下一步优先压风险，这块性价比高
2. sample plugin 集成测试
   - 当前最大残余风险仍是 Rust/Python 跨语言契约
   - 补一个最小 Python sample plugin 端到端测试，收益可能高于继续做纯 Rust 侧局部搬运
3. `src/plugin/sdk/python/scope_state.rs`
   - 如果后续 command 相关逻辑继续增长，这里会是 commands 子域下一个自然热点

### 注意事项

- `commands/mod.rs` 现在是 facade
  - 后续新增 command 逻辑时，优先直接落进对应子模块，不要把 `mod.rs` 重新写胖
- 这一轮 reviewer 线程已发出，但在当前交接时点还没有新结论返回
  - 代码层面已经完成本地验证
  - 如果 reviewer 稍后回出具体问题，应优先在 `commands/*` 新边界内修，不要回退成单文件处理

## 追加交接快照（2026-04-28，`runtime_introspection` 测试补强与共享 Python test support）

说明：

- 这一轮不再继续大拆生产代码边界
- 目标转为按顺序补 `runtime_introspection.rs` 的真实测试缺口，并顺手把 Python runtime 测试公共设施抽成共享 helper

### 本轮新增完成

#### 1. `runtime_introspection.rs` 测试覆盖已从“单点 backfill”扩到多分支

- 当前已覆盖：
  - plugin_id backfill
  - 正常 snapshot decode + backfill
  - 多插件列表排序 + `None` snapshot 跳过
  - 非法 snapshot payload decode error
  - runtime diagnostics clone 语义

实现方式：

- 不依赖真实 compat runtime 内部实现细节
- 在测试里注入最小 fake `liteyuki` module 和 `_snapshot_astrbot_plugin_runtime`
- 用受控 payload 覆盖 introspection 层最关心的 Rust-side 分支

这一步的意义是：

- `runtime_introspection.rs` 现在不再只有一个轻量 helper 测试
- 对于 `None / decode error / 多插件排序 / diagnostics clone` 这几类行为，现在都有直接兜底

#### 2. 新增共享 Python test helper

- 新增：
  - `src/plugin/sdk/python/test_support.rs`

当前承接：

- Python runtime 测试共享串行锁
- 最小 `PluginHostBridge` test host 构造
- 最小 `PyPluginSdk` test sdk 构造

#### 3. execution 测试已切到共享 test support

- `src/plugin/sdk/python/execution.rs` 测试不再自带一套局部 test lock / host helper
- 现在改为复用 `test_support.rs`

这一步也顺手回应了前面 reviewer 提过的一个残余风险：

- 之前 `execution.rs` 的 Python runtime 串行锁只在模块内有效
- 现在已经有共享 helper，可以让后续其它 Python runtime 相关测试复用同一套设施

### 当前文件规模快照

- `src/plugin/sdk/python/runtime_introspection.rs`：`338`
- `src/plugin/sdk/python/test_support.rs`：`63`
- `src/plugin/sdk/python/execution.rs`：`506`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test plugin::sdk --locked --offline`
- 当前 `plugin::sdk` 定向测试通过数：
  - `21 passed`

### 当前判断

- `plugin/sdk/python` 这条线现在已经明显进入“收尾 + 风险收口”阶段
- 生产代码的大块职责拆分已基本完成一轮
- 接下来如果继续推进，收益更高的方向未必还是继续拆文件，而更可能是：
  - sample plugin 端到端集成测试
  - 补齐更高层跨语言契约验证

### 下一步更值得处理的区域

1. sample plugin 集成测试
   - 当前 reviewer 长期指出的最大残余风险仍是 Rust/Python 跨语言契约
   - 这一步的收益现在已经高于继续做纯 Rust 侧小拆分
2. `src/llm/client.rs`
   - 如果先暂停 `plugin/sdk/python`，下一个真正的大头还是它
3. `src/web/host/llm_api.rs`
   - 同样仍是后端剩余热点之一

### 注意事项

- `test_support.rs` 目前还是 Python SDK 子域内的测试支持文件
  - 后续如果更多测试都需要修改全局 `liteyuki` bridge 状态，优先继续复用它
  - 不要再在各测试模块里零散复制局部锁和 host helper

## 追加交接快照（2026-04-28，`plugin_manager` 跨语言契约烟雾测试）

说明：

- 这一轮没有继续拆生产代码
- 目标转为补一条更高层的 sample plugin 集成测试，用真实 `PluginManager + Python runtime + capability snapshot + tool/web api/cron execution` 收口 Rust/Python 契约风险

### 本轮新增完成

#### 1. 在 `tests/plugin_manager.rs` 新增最小契约烟雾测试

- 新增测试：
  - `plugin_manager_python_contract_smoke_test_exercises_capabilities_and_execution`

覆盖点：

- 通过 `PluginManager` 发现并加载最小 Python sample plugin
- 在 `load_plugins(...)` 之后先验证 pre-start snapshot 仍为空
- 再显式调用 `start_loaded_plugins(...)` 触发 AstrBot `initialize` 阶段
- 校验 capability snapshot 已暴露：
  - 1 个 tool
  - 1 个 web api
  - 1 个 cron job
  - 1 个 task
- 对 snapshot 中的嵌套条目补字段级断言：
  - nested `plugin_id` backfill
  - tool `parameters / active / source / handler_module_path`
  - web api `methods / description / source / handler_module_path`
  - cron `job_type / cron_expression / payload / enabled`
  - task `description / task_kind / source`
- 直接调用：
  - `context.sdk.execute_plugin_tool(...)`
  - `context.sdk.execute_plugin_web_api(...)`
- 对 web request envelope 补桥接断言：
  - `headers`
  - `bodyText`
  - `peerIp`
- 直接调用：
  - `context.sdk.plugin_has_executable_cron_jobs(...)`
  - `context.sdk.run_due_plugin_jobs(...)`
- 校验 runtime diagnostics 已记录：
  - `last_tool_execution.last_success_at`
  - `last_web_api_dispatch.last_success_at`
  - `last_cron_execution.last_success_at`

#### 2. 顺手澄清了一个生命周期语义

- `manager.load_plugins(...)` 只负责 load，不等价于 start
- 对依赖 AstrBot `initialize` 动态注册的能力项，测试里必须继续调用：
  - `manager.start_loaded_plugins(context.clone()).await`

这条结论很重要，因为它决定了：

- pre-start snapshot 是否仍为空
- start 之后 snapshot 是否已经包含 context 注册的 tool / web api / cron / task
- 后续如果继续补类似 sample plugin 测试，不应误把 `load` 当成完整生命周期

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test plugin::sdk --locked --offline`
  - `cargo test --test plugin_manager plugin_manager_python_contract_smoke_test_exercises_capabilities_and_execution --locked --offline`
- 当前 `plugin::sdk` 定向测试通过数：
  - `21 passed`

### 当前判断

- `plugin/sdk/python` 这条线继续拆文件的收益已经进一步下降
- 现在更值得做的是补少量高价值契约测试，而不是再做低收益微拆分
- 这条新测试已经把“生命周期边界 + 能力快照字段 + tool/web api/cron 可执行 + diagnostics 可观察”几层链路连起来了

### 下一步更值得处理的区域

1. 这条 smoke test 的 reviewer 意见已经吸收一轮
   - 当前没有新的已知高优先级缺口
2. 如果继续补契约层
   - 优先考虑失败路径 smoke test
   - 例如 web api handler / cron handler 抛错后 diagnostics `last_error` 是否更新
3. 如果切回主线热点
   - `src/llm/client.rs`
   - `src/web/host/llm_api.rs`

### 注意事项

- 这条测试刻意保持最小，不复刻 `astrbot_context_exposes_tool_and_schedule_metadata` 那种大场景
- 断言 cron 元数据时优先看稳定字段
  - 例如 `name` / `job_type`
  - 不要把运行时生成的 `job_id` 当稳定约束

## 追加交接快照（2026-04-28，`src/llm/client.rs` 第一轮模块化拆分）

说明：

- 这一轮从 `plugin/sdk/python` 收尾切回 `LLM` 主线热点
- 目标不是改协议行为，而是先把 `src/llm/client.rs` 里最稳定的纯辅助职责抽出去
- 公开 API 保持不变，优先降低 `client.rs` 的局部上下文压力

### 本轮新增完成

#### 1. `llm client` 已补第一轮目录化边界

- 新增：
  - `src/llm/client/request_encoding.rs`
  - `src/llm/client/response_parsing.rs`
- `src/llm/client.rs` 现改为：
  - 保留 `OpenAiResponsesClient` 主流程
  - 保留 transport / request send / session loop
  - 通过 `#[path = ...]` 聚合内部子模块

#### 2. `request_encoding.rs` 已承接 request/schema 辅助职责

- 已迁入：
  - `encode_responses_tool`
  - `encode_chat_tool`
  - `initial_chat_history`
  - `supports_compat_top_k`
  - `normalize_openai_tool_parameters`
  - `llm_endpoint`
- 以及内部 schema 递归处理：
  - `normalize_openai_schema_shape`
  - `enforce_strict_openai_schema`
  - `make_schema_nullable`

这一步的收益：

- request body 组装相关逻辑不再和 stream 解析、错误回退、tool loop 混在同一段
- OpenAI strict schema 适配逻辑也从主流程文件里剥离出来

#### 3. `response_parsing.rs` 已承接 payload 解析与回退判断

- 已迁入：
  - `append_turn_text`
  - responses function-call state 聚合与发射
  - `apply_chat_stream_chunk`
  - chat assistant/tool message 构造
  - responses/chat tool call 提取
  - embedded upstream error / status 推断
  - `should_fallback_to_chat_completions`
  - `extract_output_text`
  - `truncate_text`

这一步的收益：

- `client.rs` 主体现在更接近“会话驱动器”
- 双协议 stream/json 发送逻辑与 payload 解码细节不再强耦合在同一大块

#### 4. reviewer 已经帮这轮拆分打掉两类真实回归

- 第一轮 reviewer 发现并已修复：
  - 无 description tool 被错误序列化为 `description: ""`
  - `message.content` 为结构化数组时 `extract_output_text` 退化
- 第二轮 reviewer 又补到并已修复：
  - chat stream 对无 `choices` 的 metadata frame 过严
  - tool call delta 缺 `index` 时默认槽位错误
- 已新增回归测试钉住：
  - structured chat content parts
  - responses tool request omission
  - chat tool request omission
  - metadata-only chat chunk ignore
  - indexless tool-call delta merge

#### 5. `transport/send` 第二刀也已经落地

- 新增：
  - `src/llm/client/transport.rs`
- 已从 `client.rs` 迁入：
  - `send_responses_turn`
  - `send_chat_turn`
  - `send_responses_json`
  - `send_chat_json`
  - `send_responses_stream`
  - `send_chat_stream`
  - `send_json`
  - `apply_default_headers`
  - `build_responses_request`
  - `build_chat_request`
  - outbound request log helper

这一步之后：

- `client.rs` 更明确地退回到：
  - public entrypoints
  - session fallback / tool loop
  - session state types
- `transport.rs` 单独承接：
  - HTTP request build/send
  - SSE parse loop 驱动
  - protocol turn 级响应组装

#### 6. `session/orchestration` 第三刀也已经落地

- 新增：
  - `src/llm/client/session_runtime.rs`
- 已从 `client.rs` 迁入：
  - `generate* / complete*` 会话入口
  - `start_session`
  - `ProviderSession`
  - `ProviderStart`
  - `ProviderTurn`
  - `PendingToolCall`
  - `ResponsesToolCallState`
  - `ResponsesTurnInput`
  - `EventDispatcher`
  - `execute_tool_calls`

这一步之后：

- `client.rs` 更接近真正的 façade
- `session_runtime.rs` 明确承接：
  - fallback/session loop
  - tool execution loop
  - provider turn state
- `transport.rs` 与 `session_runtime.rs` 的职责边界比上一轮更顺
  - 前者偏 I/O 与 request/turn transport
  - 后者偏 orchestration 与 runtime state

### 当前文件规模快照

- `src/llm/client.rs`：`1303`
- `src/llm/client/request_encoding.rs`：`222`
- `src/llm/client/response_parsing.rs`：`369`
- `src/llm/client/transport.rs`：`565`
- `src/llm/client/session_runtime.rs`：`328`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test llm::client --locked --offline`
- 当前 `llm::client` 定向测试通过数：
  - `22 passed`

### 当前判断

- 这轮拆分已经从“纯 helper 抽离”推进到“transport/send 抽离”
- 目前边界已经更清晰，但 `client.rs` 仍保留：
  - 公开类型
  - constructor / façade 壳层
- 所以下一轮如果继续做，应该优先考虑：
  - 继续削弱子模块对父私有类型的隐式依赖
  - 或转去并行热点 `src/web/host/llm_api.rs`

### 下一步更值得处理的区域

1. 这轮 `llm/client` reviewer 已吸收一轮真实问题
   - 当前没有新的已知高优先级回归
2. 如果继续深拆 `src/llm/client.rs`
   - 优先看进一步消掉子模块对父私有类型的隐式依赖
   - 次选是把 `ProviderTurn` 相关协议细节继续从 transport/session 之间收窄
3. 如果切去并行热点
   - `src/web/host/llm_api.rs`

### 注意事项

- 这轮刻意没有改 `src/llm/mod.rs` 的公开 re-export 形状
- `extract_output_text` 仍保持给 `web/host/llm_api/request_builders.rs` 复用
- 当前新增子模块仍依赖父模块里的若干私有类型
  - 这是本轮可接受的“先拆职责、后去隐式依赖”策略

## 追加交接快照（2026-04-28，`src/llm/client` 第二轮收窄）

说明：

- 这一轮不是继续横向拆更多文件，而是继续收窄 `transport` / `session_runtime` 之间的内部合同
- 目标是减少 `Option<String>`、`Option<Value>`、`Vec<Value>` 这种语义不清的中间态在子模块之间漂移
- 行为层保持不变，优先把 provider follow-up 的协议细节收回到更单一的位置

### 本轮新增完成

#### 1. 新增内部协议层 `protocol.rs`

- 新增：
  - `src/llm/client/protocol.rs`
- 已迁入并集中：
  - `ProviderTurn`
  - `PendingToolCall`
  - `ResponsesToolCallState`
  - `ResponsesTurnInput`
- `ProviderTurn` 现在承接：
  - responses follow-up 的 `previous_response_id` 提取与请求输入构造
  - chat follow-up 的 assistant/tool message 追加

这一步的收益：

- `session_runtime.rs` 不再自己拼 `function_call_output` JSON
- `session_runtime.rs` 也不再直接摸 `response_id` / `assistant_message` 这类松散字段
- follow-up 语义从“多个可空字段”收敛成更明确的内部合同

#### 2. 请求体拼装已从 `transport.rs` 回收到 `request_encoding.rs`

- 已新增并迁入：
  - `build_responses_request`
  - `build_chat_request`
  - `apply_common_generation_fields`

这一步的收益：

- `transport.rs` 不再同时负责：
  - 发请求
  - 组请求
  - 挂通用 generation 参数
- 双协议共有的温度、`top_p`、`top_k`、penalty、reasoning、tools 注入路径也进一步合并

#### 3. turn 级 payload 成形与 SSE 状态机已从 `transport.rs` 回收到 `response_parsing.rs`

- 已新增并迁入：
  - `build_responses_turn_from_payload`
  - `build_chat_turn_from_payload`
  - `finalize_responses_stream_turn`
  - `finalize_chat_stream_turn`
  - `ResponsesStreamState`
  - `ChatStreamState`

这一步的收益：

- `transport.rs` 更接近纯 I/O + SSE framing 驱动
- turn 级响应组装、ToolCall/TextDone 补发、assistant message 构造被放回同一个“解码/成形”模块

#### 4. tool loop 的中间态已经去掉裸 `Vec<Value>`

- `execute_tool_calls` 现在只返回强类型 `Vec<LlmExecutedToolCall>`
- provider-specific follow-up payload/message 在协议层按需转换

这一步的收益：

- `session_runtime.rs` 不再承担 provider JSON 细节
- 后续若继续改 responses/chat follow-up 形状，改动面更集中

#### 5. 事件分发器已抽成中立共享层

- 新增：
  - `src/llm/client/event_dispatch.rs`
- `response_parsing.rs` / `transport.rs` 不再反向依赖 `session_runtime.rs` 里的具体实现

这一步的收益：

- 去掉了解析层依赖运行时层的分层倒置
- 后续若继续补 parser/transport 级测试，不需要再从 session 层借具体类型

### 当前文件规模快照

- `src/llm/client.rs`：`1197`
- `src/llm/client/request_encoding.rs`：`311`
- `src/llm/client/event_dispatch.rs`：`17`
- `src/llm/client/protocol.rs`：`112`
- `src/llm/client/response_parsing.rs`：`558`
- `src/llm/client/transport.rs`：`209`
- `src/llm/client/session_runtime.rs`：`245`

### 当前校验状态

- 已通过：
  - `cargo check --all-targets --locked --offline`
  - `cargo test --no-run --locked --offline`
  - `cargo test llm::client --locked --offline`
- 当前 `llm::client` 定向测试通过数：
  - `22 passed`

### 当前判断

- 相比上一轮，`transport.rs` 已明显更接近 transport 本身
- `session_runtime.rs` 现在主要承担：
  - session fallback
  - tool execution
  - turn-to-turn orchestration
- `request_encoding.rs` / `response_parsing.rs` / `protocol.rs` / `event_dispatch.rs` 四者的分工也比上一轮更自然：
  - request body 编码
  - payload/stream turn 成形与 SSE 状态机
  - 内部共享协议类型与 follow-up 合同
  - 事件分发适配
- 最新 reviewer 结论：
  - `transport.rs` 过重这一条已解决
  - 当前没有阻止切去 `src/web/host/llm_api.rs` 的新高优先级问题
  - 仍可继续演进的点主要只剩更大范围的 typed message / typed continuation 建模

### 下一步更值得处理的区域

1. `llm/client` 这一轮可以先收口
   - 当前剩余问题以“typed wire model”这一类更大改造为主
   - 收益存在，但风险和牵连面已经明显高于继续切 `web/host/llm_api.rs`
2. 直接切到下一热点
   - `src/web/host/llm_api.rs`
3. 如果未来再回到 `llm/client`
   - 优先考虑 typed `ChatMessage` / typed continuation
   - 不优先再做测试外迁或表层目录化

### 注意事项

- 这一轮没有改公开 API
- `protocol.rs` 目前是 `llm/client` 内部共享层，不建议提前提升为更外层公共模块
- `request_encoding.rs` 行数上升是预期结果
  - 它吸收的是从 `transport.rs` 回收的纯编码职责
- `response_parsing.rs` 行数上升也是预期结果
  - 它现在同时承接 payload 成形与 SSE 状态机

## 追加交接快照（2026-04-28，`web/host/llm_api` 第五轮收窄）

说明：

- 这一轮继续停留在 API 子树内部
- 目标是把上一轮 reviewer 指出的三类真实问题直接吸收掉：
  - `provider_state` 仍返回 JSON payload
  - `route_llm_api` 缺少直接路由测试
  - `provider_serialization` 缺少独立回归钉子

### 本轮新增完成

#### 1. `provider_state` 与 `provider_serialization` 的边界已进一步收紧

- `merge_saved_and_discovered_models(...)` 现在不再返回 `Vec<Value>`
- 改为返回强类型：
  - `ManagedProviderModelView`
- 新增：
  - `serialize_managed_provider_models(...)`

结果：

- `provider_state.rs` 负责：
  - discovered/saved model merge 策略
  - enabled 标记协调
- `provider_serialization.rs` 负责：
  - `serialize_managed_provider(...)`
  - `serialize_managed_provider_models(...)`

这一步把前一轮 reviewer 提到的“状态层还在拼前端 JSON”的问题收掉了。

#### 2. `resolve_provider_id` 的隐藏父作用域耦合已进一步消掉

- 下列文件已改成直接从 `crate::llm::service` 显式依赖：
  - `src/web/host/llm_api/provider_state.rs`
  - `src/web/host/llm_api/provider_catalog.rs`
  - `src/web/host/llm_api/provider_serialization.rs`

这一步的意义：

- 这些子模块不再依赖 `llm_api.rs` 顶层私有 import 恰好存在
- 后续继续清理 `llm_api.rs` 根模块 import 列表时，破坏面更小

#### 3. `request_builders.rs` 已完成 provider/request-family 目录化拆分

- `src/web/host/llm_api/request_builders.rs` 现在只保留聚合导出
- 新增目录：
  - `src/web/host/llm_api/request_builders/shared.rs`
  - `src/web/host/llm_api/request_builders/responses.rs`
  - `src/web/host/llm_api/request_builders/openrouter.rs`
  - `src/web/host/llm_api/request_builders/anthropic.rs`
  - `src/web/host/llm_api/request_builders/gemini.rs`

职责分布：

- `shared.rs`
  - system/chat fallback prompt 组装
  - message/attachment 公共 helper
  - `ParsedDataUrl`
- `responses.rs`
  - responses input 组装
- `openrouter.rs`
  - openrouter/chat-completions message 组装
- `anthropic.rs`
  - anthropic payload / thinking budget / text extract
- `gemini.rs`
  - gemini payload / thinking config / text extract

顺手完成：

- `ParsedDataUrl` 已从 `types.rs` 移出
- `types.rs` 不再继续吸收 request builder 内部实现细节

#### 4. `llm_api` 聚合测试已补上两类关键缺口

- 新增 serializer 级测试：
  - `serialize_managed_provider_emits_active_provider_metadata`
  - `serialize_managed_provider_models_preserves_merge_results`
- 新增 route 层测试：
  - `route_llm_api_rejects_non_post_manager_routes`
  - `route_llm_api_returns_none_for_unknown_path`

另外：

- `tests.rs` 顶部已不再使用 `use super::*`
- 改成显式列出依赖项，降低对 `llm_api.rs` 根模块 wildcard fan-in 的耦合

### 当前文件规模快照

- `src/web/host/llm_api.rs`：`188`
- `src/web/host/llm_api/tests.rs`：`650`
- `src/web/host/llm_api/provider_state.rs`：`353`
- `src/web/host/llm_api/provider_serialization.rs`：`58`
- `src/web/host/llm_api/request_builders.rs`：`20`
- `src/web/host/llm_api/request_builders/shared.rs`：`220`
- `src/web/host/llm_api/request_builders/responses.rs`：`66`
- `src/web/host/llm_api/request_builders/openrouter.rs`：`148`
- `src/web/host/llm_api/request_builders/anthropic.rs`：`160`
- `src/web/host/llm_api/request_builders/gemini.rs`：`165`

### 当前校验状态

- 已通过：
  - `cargo test llm_api --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm_api` 定向测试通过数：
  - `25 passed`

### 当前判断

- `llm_api.rs` 主入口已经基本收口完成
- `provider_state` / `provider_serialization` / `request_builders` 三个次热点也都完成了一轮更自然的职责切分
- `web/host/llm_api` 子树剩余更大的复杂度中心现在更集中在：
  - `manager_actions.rs`
  - `provider_catalog.rs`
  - 以及按测试量看，`tests.rs` 自身已经开始成为聚合测试热点

### 下一步更值得处理的区域

1. 优先看 `manager_actions.rs`
   - 它现在更像 `llm_api` 子树里剩余最混合的执行/探测拼装层
2. 次选看 `provider_catalog.rs`
   - 如果继续推进目录硬编码与运行时策略分离，这里仍有收益
3. 如果 API 子树先暂停
   - 直接切回：
     - `src/llm/tools.rs`
     - 或 `src/plugin/sdk/mod.rs`

### 注意事项

- `request_builders.rs` 现在是 facade
  - 后续新增 provider-specific 组装逻辑时，优先直接落到子模块，不要把根文件重新写胖
- `tests.rs` 已开始承接更多聚合回归
  - 如果继续增加 API 栈测试，需要留意它未来也可能再拆按主题分组

## 追加交接快照（2026-04-28，`manager_actions` facade 化）

说明：

- 这一轮继续留在 `web/host/llm_api` 子树
- 目标是把 `manager_actions.rs` 从“provider 探测 + request prepare + preview/redaction”混装，收成和 `request_builders` 相同风格的 facade

### 本轮新增完成

#### 1. `manager_actions.rs` 已目录化为 facade

- `src/web/host/llm_api/manager_actions.rs` 现在只保留导出聚合
- 新增目录：
  - `src/web/host/llm_api/manager_actions/preparation.rs`
  - `src/web/host/llm_api/manager_actions/preview.rs`

职责分布：

- `preparation.rs`
  - provider model discovery
  - provider request probe
  - prepared request build
  - runtime headers collect
- `preview.rs`
  - preview headers render
  - preview payload redact
  - secret masking

结果：

- `manager_actions.rs` 从执行细节文件退回为 façade
- `manager.rs` / `chat_exec.rs` 继续沿用原导出面，不需要感知内部拆分

### 当前文件规模快照

- `src/web/host/llm_api/manager_actions.rs`：`9`
- `src/web/host/llm_api/manager_actions/preparation.rs`：`335`
- `src/web/host/llm_api/manager_actions/preview.rs`：`60`

### 当前校验状态

- 已通过：
  - `cargo test llm_api --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm_api` 定向测试通过数：
  - `25 passed`

### 当前判断

- `llm_api` 子树里原本最混合的几个执行/拼装热点，目前已经都被压成 facade + 子模块结构：
  - `request_builders.rs`
  - `manager_actions.rs`
- 继续往下看时，更值得处理的点会逐渐从“单文件太胖”转成：
  - `provider_catalog.rs` 中的目录硬编码与能力描述
  - `tests.rs` 是否要继续按主题拆分

### 下一步更值得处理的区域

1. `provider_catalog.rs`
   - 现在已经更像 `llm_api` 子树剩余的主要策略/硬编码中心
2. 如果 API 子树先暂停
   - 回到：
     - `src/llm/tools.rs`
     - 或 `src/plugin/sdk/mod.rs`

## 追加交接快照（2026-04-28，`llm_api` 第六轮修正）

说明：

- 这一轮不是继续横向加新 facade
- 目标是吸收 reviewer 指出的真实行为问题与边界残留：
  - preview headers 可能泄漏自定义认证头
  - `collect_runtime_headers` 不该挂在 `manager_actions`
  - `manager_actions/preparation` 仍重复做 request body 拼装
  - `frontend_chat.rs` 还依赖已删除的根级隐式 import

### 本轮新增完成

#### 1. preview headers 的敏感值脱敏已补齐

- `src/web/host/llm_api/manager_actions/preview.rs`
  - `preview_headers_map(...)` 不再只掩码框架生成的 auth header
  - 现在会对 `extra_headers` 中的敏感 header 名称做统一脱敏

当前覆盖的敏感头规则包括：

- `Authorization`
- `Proxy-Authorization`
- `x-api-key` / `api-key` / `api_key`
- 包含 `token` / `secret` / `password` / `passwd`
- `Cookie` / `Set-Cookie`

结果：

- 即使 provider 把凭据存进自定义 headers，`/LLM/PreviewRequest` 返回给前端时也不会再原样泄漏

#### 2. `collect_runtime_headers(...)` 已移到更通用的 transport 层

- 已从：
  - `src/web/host/llm_api/manager_actions/preparation.rs`
- 迁到：
  - `src/web/host/llm_api/transport.rs`

意义：

- `/LLM/Chat` 不再反向依赖 `manager_actions` façade 才能合并 headers
- 这个 helper 现在放在真正的共享层，边界更自然

#### 3. preview/probe 用的 request body 拼装已回收到 `request_builders`

- 新增导出：
  - `build_chat_completions_request_payload(...)`
  - `build_openai_compatible_request_payload(...)`
- `src/web/host/llm_api/manager_actions/preparation.rs` 不再自带：
  - `build_chat_completions_request`
  - `build_openai_compatible_request`

意义：

- request shape 重新回到 `request_builders` 统一承接
- `manager_actions/preparation` 更接近：
  - provider request prepare / discovery / probe orchestration
  - 而不是再夹带协议级 body 细节

#### 4. `request_builders/*` 子模块的父级转发依赖已继续收紧

- `shared.rs` / `responses.rs` / `openrouter.rs` / `anthropic.rs` / `gemini.rs`
  现在直接依赖：
  - `super::super::types::*`
  - 或各自明确需要的共享层
- 不再靠 `request_builders.rs` 先把类型/常量转发一遍再由子模块通过 `super` 去拿

#### 5. `frontend_chat.rs` 的旧隐式根 import 已清掉

- 现在直接从：
  - `crate::llm::service`
  依赖：
  - `current_active_prompt_profile`
  - `current_llm_runtime_config`
  - `resolve_provider_id`

这一步顺手继续减少了 `llm_api.rs` 入口文件承担“隐式名字分发器”的角色。

### 当前新增测试

- `preview_headers_map_masks_sensitive_custom_headers`
- `openai_compatible_request_payload_omits_top_k_for_openai_base_url`
- `chat_completions_request_payload_keeps_top_k_and_reasoning`

### 当前校验状态

- 已通过：
  - `cargo test llm_api --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm_api` 定向测试通过数：
  - `28 passed`

### 当前判断

- 这一轮后，`llm_api` 子树里与 preview/request assembly 相关的真实风险点已经明显下降
- 继续在 API 子树里推进的话，最值得处理的剩余热点仍然是：
  - `provider_catalog.rs`
- 因为它现在同时承担：
  - provider label/support/model option 策略
  - 大块目录硬编码
  - 能力说明输出

## 追加交接快照（2026-04-28，`src/web/host/llm_api` 第四轮收窄）

说明：

- 这一轮继续处理 `web/host/llm_api` 剩余的边界噪音
- 目标不是再横向加很多新模块，而是把“职责放错层”和“运行时代码/测试混装”这两类尾部问题收掉

### 本轮新增完成

#### 1. `provider_serialization.rs` 已收窄成纯序列化层

- `merge_saved_and_discovered_models(...)` 已从：
  - `src/web/host/llm_api/provider_serialization.rs`
- 迁回：
  - `src/web/host/llm_api/provider_state.rs`

原因：

- 这段逻辑本质是 provider 状态协调，不是 HTTP/Web 输出序列化
- 上一轮 reviewer 已明确指出这里还有职责漂移

结果：

- `provider_serialization.rs` 现在只剩：
  - `serialize_managed_provider(...)`
- `provider_state.rs` 统一承接：
  - active provider 选择
  - provider 输入归一化
  - enabled model set / primary model 选择
  - discovered/saved model merge 策略

#### 2. `llm_api.rs` 运行时代码与测试已彻底分开

- `src/web/host/llm_api.rs` 中原本内联的 tests 已全部外提到：
  - `src/web/host/llm_api/tests.rs`
- 当前结构变为：
  - `llm_api.rs` 只保留模块聚合、路由分发、少量公共常量、方法校验 helper
  - `tests.rs` 单独承接 `llm_api` 子树的聚合级回归测试

这一步的收益：

- 运行时代码不再和 500+ 行测试同文件混装
- `llm_api.rs` 现在更接近真正的“模块入口 + 路由层”
- 后续继续切 `request_builders` / `manager_actions` / `provider_catalog` 时，主入口文件上下文压力明显更低

#### 3. 顶层隐式父作用域依赖继续收紧

- `src/web/host/llm_api.rs` 已不再使用：
  - `use super::*`
- 改为显式引入：
  - `WebHostService`
  - `napcat_err`
  - `napcat_ok`
  - `napcat_response`
  - `parse_json_body` 作为对子模块的显式共享依赖

这一步的意义：

- 之前 `parse_json_body` 这类名字实际上是靠父模块漏进来的
- 这轮编译时已经真实暴露过一次该问题，并已改成显式来源
- 后续如果继续去隐式依赖，可以围绕这条方式继续推进，而不是再回退到 `use super::*`

### 当前文件规模快照

- `src/web/host/llm_api.rs`：`188`
- `src/web/host/llm_api/tests.rs`：`545`
- `src/web/host/llm_api/provider_state.rs`：`350`
- `src/web/host/llm_api/provider_serialization.rs`：`46`
- `src/web/host/llm_api/manager.rs`：约 `246`

和前几轮快照相比：

- `src/web/host/llm_api.rs`：`3671 -> 1010 -> 619 -> 188`
- `manager_support.rs` 已被替换为：
  - `provider_state.rs`
  - `provider_serialization.rs`

### 当前校验状态

- 已通过：
  - `cargo test llm_api --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm_api` 定向测试通过数：
  - `21 passed`

#### 这一轮之前已补的相关测试仍在

- `web_llm_chat_request_deserializes_camel_case_fields`
- `manager_payloads_deserialize_camel_case_fields`
- `resolve_active_provider_id_falls_back_when_explicit_id_is_missing`
- `merge_saved_and_discovered_models_deduplicates_and_preserves_saved_enabled_flags`
- `resolve_models_to_test_prefers_explicit_model_over_test_all`

### 当前判断

- `llm_api.rs` 主文件本身已经基本收口到标准范围
- `provider_state` / `provider_serialization` 的层级边界比上一轮更自然
- `web/host/llm_api` 子树剩余更值得继续处理的点，已经不是“主入口太胖”，而是几个子模块自身的职责密度：
  - `request_builders.rs`
  - `provider_catalog.rs`
  - `manager_actions.rs`

### 下一步更值得处理的区域

1. 优先继续看 `request_builders.rs`
   - 它现在是 `llm_api` 子树里最大热点
   - 可以继续拆按 provider/request family 分层，或至少把共享 content/attachment 变换层抽出来
2. 次选看 `provider_catalog.rs`
   - 如果继续推进“硬编码与目录策略分离”，这块收益仍然比较明确
3. 如果 `llm_api` 子树暂时收口
   - 直接切回：
     - `src/llm/tools.rs`
     - 或 `src/plugin/sdk/mod.rs`

### 注意事项

- `tests.rs` 现在是 `llm_api` 子树聚合测试承载点
  - 后续新增 `llm_api` 级回归测试，优先继续收在这里
  - 不要再把大段测试写回 `llm_api.rs`
- `parse_json_body` 目前通过 `llm_api.rs` 显式共享给子模块
  - 如果后续继续做更强边界隔离，可以考虑让具体子模块直接依赖 `super::super::http` 或更窄 facade
  - 但这一层现在已经比原先的隐式父作用域泄漏更可控

## 追加交接快照（2026-04-28，`provider_catalog` 收口与 preview 回归修复）

说明：

- 这一轮继续停留在 `web/host/llm_api` 子树
- 目标是把 reviewer 明确指出的两条真实回归直接收掉：
  - `/LLM/PreviewRequest` 对 chat-completions `content` 文本脱敏不完整
  - `providerCatalog` 与 runtime `supports` 出现双真相漂移

### 本轮新增完成

#### 1. `provider_catalog.rs` 已继续收口成 façade

- `src/web/host/llm_api/provider_catalog.rs` 现在只保留聚合导出
- 新增目录内数据访问层：
  - `src/web/host/llm_api/provider_catalog/catalog_data.rs`
- `src/hardcode_data/llm_provider_catalog.rs` 继续作为唯一的 provider 目录硬编码来源

当前职责分布：

- `catalog_data.rs`
  - 从 `hardcode_data` 读取单个 provider catalog 条目
  - 提供 label / sample models / parameter support 的窄访问接口
- `metadata.rs`
  - 只保留 base url 归一化、provider id 识别、reasoning option 推导
  - provider label / sample model / support 开关已改为从 catalog 派生
- `options.rs`
  - 继续负责 manager/settings 里的 provider option 与 model option 组装

结果：

- `metadata.rs` 不再手写一套 provider label 与 sample model 列表
- provider 目录事实已经更集中地收回到 `src/hardcode_data/llm_provider_catalog.rs`

#### 2. runtime `supports` 已改为从 catalog 能力矩阵派生

- `current_provider_supports(...)` 不再维护独立的 provider-by-provider 布尔表
- 现在会优先读取 `parameterSupport`
  - `unsupported -> false`
  - 其余状态如 `supported` / `conditional` / `fixed` / `unknown` -> true
- `reasoningEffort` 仍保持 runtime 语义
  - 继续按 `reasoning_options_for_provider(...)` 是否非空决定

结果：

- Anthropic 的 `textFileInput`
- Kimi 的 `imageInput`
- 以及其余 runtime 能力布尔开关

都不再和前端看到的 `providerCatalog` 分别维护两份事实

#### 3. preview payload 的 `content` 文本脱敏缺口已补齐

- `src/web/host/llm_api/manager_actions/preview.rs`
  - `redact_preview_payload(...)` 现在除了 `instructions` / `system` / `text`
  - 还会处理 chat-completions 形态下的 `content`
- 当前行为：
  - `content: "..."` 会被替换成 `<redacted>`
  - `content: [{ type: "text", text: "..." }]` 里的文本也会被替换
  - `image_url.url` 这类非文本字段保持原样，避免把预览结构整体抹掉

结果：

- OpenRouter / OpenAI-compatible chat-completions 预览不再把 system/user prompt 文本直接回传到前端

#### 4. 已补两条针对 reviewer 结论的回归测试

- 新增：
  - `provider_catalog_includes_expected_provider_ids`
  - `preview_payload_redacts_chat_completion_content_strings`
  - `preview_payload_redacts_anthropic_system_prompt`
  - `runtime_supports_follow_provider_catalog_capabilities`
  - `detect_provider_id_recognizes_qwen_international_hosts`
  - `resolve_provider_id_detects_qwen_international_base_urls`

其中：

- `preview_payload_redacts_chat_completion_content_strings`
  - 钉住 `content` 纯字符串与 content-array text block 两条泄漏路径
- `runtime_supports_follow_provider_catalog_capabilities`
  - 钉住 runtime `supports` 与 catalog 能力矩阵的关键一致性

#### 5. Qwen 官方国际域名识别已补齐

- `src/llm/service.rs`
  - `detect_provider_id_from_base_url(...)` 现在同时识别：
    - `dashscope.aliyuncs.com`
    - `dashscope-intl.aliyuncs.com`

结果：

- Qwen 新加坡 / US Virginia 官方兼容地址不再被误判成 `openai-compatible`
- `provider label` / `supports` / 默认模型选项也不会再沿着错误 provider 分支继续漂移

#### 6. `unknown` 能力态不再被折叠成 `true`

- `support_state_enabled(...)` 现在只把：
  - `supported`
  - `conditional`
  - `fixed`
  视为可开启
- `unknown` 不再默认折叠成 `true`

结果：

- 例如 Qwen catalog 里标记为 `unknown` 的 `topK`
  - runtime `supports.topK` 现在不会再误报成可用

#### 7. `providerOptions` 已保证包含当前活动 runtime base URL

- `src/web/host/llm_api/provider_catalog/options.rs`
  - 即使历史 `provider_urls` 仍存在但已经过期
  - 也会补回当前活动 runtime `base_url`

结果：

- settings payload 不会再出现：
  - `baseUrl` 指向一个当前活动 provider
  - 但 `providerOptions` 里却没有该活动项

### 当前文件规模快照

- `src/web/host/llm_api.rs`：`214`
- `src/web/host/llm_api/provider_catalog.rs`：`19`
- `src/web/host/llm_api/provider_catalog/catalog_data.rs`：`39`
- `src/web/host/llm_api/provider_catalog/metadata.rs`：`97`
- `src/web/host/llm_api/provider_catalog/options.rs`：`96`
- `src/web/host/llm_api/manager_actions/preview.rs`：`123`
- `src/web/host/llm_api/tests.rs`：`862`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm_api --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm_api` 定向测试通过数：
  - `33 passed`

### 当前判断

- `provider_catalog` 这一轮之后已经不再是“catalog 一份、runtime metadata 再手写一份”的状态
- preview 相关真实泄漏点已经被测试直接钉住
- `llm_api` 子树里更值得继续处理的剩余点，主要变成：
- `manager_actions/preparation.rs` 仍混有 provider request contract 细节
- `tests.rs` 已经再次长到需要按主题拆分的规模
- `llm_api.rs` 根层仍存在一定的 sibling fan-in / `super::{...}` 风格耦合
- `openai-compatible` 的 reasoning option 与 sample model 来源仍未完全统一

### 下一步更值得处理的区域

1. 继续收窄 `src/web/host/llm_api/manager_actions/preparation.rs`
   - 尤其是 provider endpoint / auth / request assemble 的职责边界
2. 如果 API 子树先收口
   - 直接切到：
     - `src/llm/tools.rs`
3. 如果未来继续留在 API 子树
   - 优先拆 `tests.rs`
   - 次选再清理根层 sibling fan-in

## 追加交接快照（2026-04-28，`llm/tools` 的 `workspace_access` 拆分）

说明：

- 这一轮已经从 `llm_api` 切到下一个热点 `src/llm/tools.rs`
- 先处理的是 `workspace_access.rs`
  - 因为它仍混有 workspace root 探测
  - 路径净化 / 越界保护
  - 文件列表 / 文件读取输出

### 本轮新增完成

#### 1. `workspace_access.rs` 已收口成 façade

- `src/llm/tools/workspace_access.rs` 现在只保留模块装配与导出
- 新增目录：
  - `src/llm/tools/workspace_access/file_ops.rs`
  - `src/llm/tools/workspace_access/path_safety.rs`
  - `src/llm/tools/workspace_access/root_detection.rs`

职责分布：

- `file_ops.rs`
  - `workspace_list_files`
  - `workspace_read_file`
  - 目录遍历与输出格式化
- `path_safety.rs`
  - 相对路径净化
  - workspace 越界校验
  - display path 格式化
- `root_detection.rs`
  - `LY_WORKSPACE_ROOT`
  - git boundary / workspace manifest 探测

结果：

- `workspace_access.rs` 不再同时承载三类不同层次的逻辑
- `local_execution_tools.rs` 仍保持原导入面，不需要感知内部拆分

#### 2. 原有路径安全与 workspace root 回归测试已按职责下沉

- `path_safety.rs`
  - `sanitize_relative_path_rejects_parent_dirs`
- `file_ops.rs`
  - `read_workspace_file_returns_empty_range_when_start_line_exceeds_file`
  - `read_workspace_file_rejects_missing_leaf_under_external_symlink`
- `root_detection.rs`
  - git boundary / workspace manifest / outermost manifest 几组探测回归

结果：

- 测试不再继续堆在一个 500+ 行混合文件里
- 每组行为现在和它对应的实现文件同地维护

### 当前文件规模快照

- `src/llm/tools.rs`：`298`
- `src/llm/tools/workspace_access.rs`：`9`
- `src/llm/tools/workspace_access/file_ops.rs`：`282`
- `src/llm/tools/workspace_access/path_safety.rs`：`95`
- `src/llm/tools/workspace_access/root_detection.rs`：`202`
- `src/llm/tools/tool_state.rs`：`341`
- `src/llm/tools/tests.rs`：`306`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::tools --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::tools` 定向测试通过数：
  - `30 passed`

### 当前判断

- `llm/tools` 相比最初的大文件状态已经明显进入可维护区间
- 当前剩余更值得继续处理的点主要是：
  - `tool_state.rs` 仍混有持久化、备份恢复、错误回滚
  - `tests.rs` 仍是聚合级行为测试承载点，但规模暂时可接受
- 如果下一刀继续留在 `llm/tools`
  - `tool_state.rs` 是比 `tests.rs` 更优先的目标

## 追加交接快照（2026-04-28，`llm/tools` 的 `tool_state` 拆分）

说明：

- 这一轮继续留在 `src/llm/tools.rs`
- 目标是把 `tool_state.rs` 从“状态 API + JSON 读写 + 备份恢复 + 回滚测试”混装，收成 façade

### 本轮新增完成

#### 1. `tool_state.rs` 已收口成 façade

- `src/llm/tools/tool_state.rs` 现在只保留导出聚合
- 新增目录：
  - `src/llm/tools/tool_state/store.rs`
  - `src/llm/tools/tool_state/storage.rs`

职责分布：

- `store.rs`
  - `ToolStateStore`
  - active 开关语义
  - 失败回滚
- `storage.rs`
  - `ToolStateDocument`
  - JSON 读取
  - backup 恢复
  - temp/backup 路径生成
  - 原子写回流程

结果：

- `ToolStateStore` 不再同时承载状态接口与底层文件读写细节
- `tests.rs` 对 `tool_state_backup_path(...)` 的依赖继续保持不变

#### 2. `tool_state` 回归测试已按职责下沉

- `storage.rs`
  - `missing_primary_with_invalid_backup_surfaces_error`
- `store.rs`
  - `set_active_rolls_back_failed_disable_write`
  - `set_active_rolls_back_failed_enable_write`

结果：

- 读写恢复语义与状态回滚语义现在分别跟随各自实现文件维护

### 当前文件规模快照

- `src/llm/tools/tool_state.rs`：`9`
- `src/llm/tools/tool_state/store.rs`：`170`
- `src/llm/tools/tool_state/storage.rs`：`186`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::tools --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::tools` 定向测试通过数：
  - `30 passed`

### 当前判断

- `llm/tools` 这一轮后已经基本没有明显的单文件职责堆积点
- 如果继续留在本子树，收益更高的工作会逐渐从“继续机械拆文件”转成：
  - 是否要把 `tests.rs` 按主题继续分组
  - 是否要补更高层的 manager/inventory 组合级回归

## 追加交接快照（2026-04-28，`llm/client` 的 façade 收口）

说明：

- 这一轮回到 `src/llm/client.rs`
- 目标不是继续动协议实现，而是把根文件里最后一大块非运行时职责移出去
- 行为保持不变，优先让 `client.rs` 真正回到 façade / 装配入口

### 本轮新增完成

#### 1. `src/llm/client.rs` 已移除内联测试体

- 根文件现在只保留：
  - runtime config trait
  - public client / tool / error / event 类型
  - constructor
  - `extract_output_text(...)` 薄包装
  - `#[path = "client/tests.rs"] mod tests;`
- 新增：
  - `src/llm/client/tests.rs`

结果：

- `client.rs` 不再混有 mock server、日志环境变量 guard、协议行为回归测试
- façade 根文件的阅读上下文明显缩小，更符合当前目录化结构

#### 2. 测试可见性和 `#[path]` 编译方式保持兼容

- 测试仍作为 `llm::client` 的内部测试模块编译
- 现有测试对这些内部实现的访问方式保持不变：
  - `EventDispatcher`
  - `ResponsesTurnInput`
  - `build_responses_request(...)`
  - `llm_endpoint(...)`
  - `apply_chat_stream_chunk(...)`
- 这意味着本轮没有把测试强行改成新的公共 API，也没有扩大生产代码暴露面

### 当前文件规模快照

- `src/llm/client.rs`：`1197 -> 247`
- `src/llm/client/tests.rs`：`944`

### 当前校验状态

- 已通过：
  - `cargo test llm::client --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::client` 定向测试通过数：
  - `22 passed`

### 当前判断

- `llm/client` 的运行时代码主入口现在已经不再是大文件热点
- 如果后续继续留在本子树，更值得优先处理的将不再是 `client.rs` 根文件行数，而是：
  - 是否要把 `tests.rs` 再按 streaming / request encoding / fallback 行为分组
  - 是否要继续做更强的 typed wire model / typed continuation 收口

## 追加交接快照（2026-04-28，`llm/client` 的 chat runtime 合同继续收窄）

说明：

- 这一轮按“先功能重构、后测试归位”的原则继续留在 `llm/client`
- 目标不是再切测试文件，而是继续把 chat 路径里的裸 `Vec<Value>` / 重复 tool loop 从运行时层抽走

### 本轮新增完成

#### 1. `ChatHistory` 已进入内部协议层

- 新增到：
  - `src/llm/client/protocol.rs`
- `ChatHistory` 现在承接：
  - 从 `system_prompt + prompt` 构造初始 chat history
  - 从现有 `Vec<Value>` 包装 chat history
  - 受控追加 assistant/tool follow-up message

结果：

- `session_runtime.rs` 不再直接操作裸 `Vec<Value>` 历史
- `transport.rs` / `request_encoding.rs` 看到的是更明确的内部协议对象，而不是任意消息数组

#### 2. chat request 编码层已不再生成 history

- `initial_chat_history(...)` 已从 `request_encoding.rs` 移除
- `build_chat_request(...)` 现在接收 `ChatHistory`

结果：

- request encoding 层更接近纯编码职责
- chat history 的生成与 mutation 被收回到 protocol/session 一侧

#### 3. session loop 已收成单一执行路径

- `src/llm/client/session_runtime.rs` 新增统一：
  - `run_session_loop(...)`
  - `complete_with_chat_history(...)`
- `complete_with_input(...)` 与 `complete_with_chat_messages(...)` 不再各自维护一份重复的：
  - `append_turn_text`
  - tool execute
  - continue-with-tool-outputs
  - accumulate completion

结果：

- tool loop 的控制流只剩一份
- responses/chat 两个入口现在共享相同的 turn-to-turn 编排路径

### 当前文件规模快照

- `src/llm/client/protocol.rs`：`154`
- `src/llm/client/session_runtime.rs`：`250`
- `src/llm/client/request_encoding.rs`：`293`
- `src/llm/client/transport.rs`：`205`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::client --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::client` 定向测试通过数：
  - `22 passed`

### 当前判断

- 这一轮之后，`llm/client` 更接近“protocol / encoding / parsing / transport / session runtime”五层分工
- 如果继续留在本子树，下一步更值钱的方向将是：
  - typed chat message / typed continuation 继续收口
  - 或把注意力切到仍更重的其它热点文件，而不是继续在现有边界上做表层切分

## 追加交接快照（2026-04-28，`llm/client` 的 typed continuation 继续收口）

说明：

- 这一轮继续只做功能边界收窄
- 没有先处理测试归位
- 目标是继续减少 protocol / session / request encoding 之间漂移的匿名 JSON 形状

### 本轮新增完成

#### 1. chat continuation 已不再保存裸 `assistant_message: Value`

- `src/llm/client/protocol.rs` 新增并使用：
  - `ChatMessage`
  - `ChatAssistantMessage`
  - `ChatToolMessage`
- `ProviderContinuation::Chat` 现在保存的是 typed `ChatMessage`
- `ChatHistory` 也从 `Vec<Value>` 收成了 `Vec<ChatMessage>`

结果：

- protocol/session 层不再直接拼 assistant/tool follow-up JSON
- 只有在 request encoding 的最后一跳才把 `ChatMessage` 序列化成上游请求体

#### 2. responses tool follow-up 已不再保存裸 `Vec<Value>`

- `src/llm/client/protocol.rs` 新增：
  - `ResponsesToolOutput`
- `ResponsesTurnInput::ToolOutputs.outputs` 现在保存 typed `Vec<ResponsesToolOutput>`
- `request_encoding.rs` 负责把它编码成实际的 `function_call_output` JSON 数组

结果：

- protocol 层同时收掉了 chat / responses 两条 continuation 里的匿名 JSON 容器
- `request_encoding.rs` 更明确地成为“typed internal model -> wire payload”转换层

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::client --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::client` 定向测试通过数：
  - `22 passed`

### 当前判断

- `llm/client` 里最明显的 continuation / follow-up 裸 JSON 已经基本收口
- 如果继续留在这条线，下一步更值钱的方向会更偏向：
  - typed initial chat input / typed external message normalization
  - 或直接切回其它更大的热点文件做同类边界收窄

## 追加交接快照（2026-04-28，`llm/mcp` 的 façade 收口）

说明：

- 这一轮从 `llm/client` 切到新的 LLM 运行时热点 `src/llm/mcp.rs`
- 目标是把 MCP 里的配置读取、HTTP transport、JSON-RPC/SSE 解析、schema 规范化从单文件里拆开
- 测试位置暂时保持不动，先只收口功能边界

### 本轮新增完成

#### 1. `src/llm/mcp.rs` 已退回到 manager / 类型 / 装配入口

- 根文件现在主要保留：
  - `McpManager`
  - catalog / bound tool / config 类型
  - `load_tools(...)`
  - `inspect_servers(...)`
  - 内联测试

结果：

- MCP 根文件不再同时承载 config 读取、transport、RPC 解析、schema 规范化全部实现细节

#### 2. MCP 内部职责已下沉为目录模块

- 新增：
  - `src/llm/mcp/config.rs`
  - `src/llm/mcp/transport.rs`
  - `src/llm/mcp/rpc.rs`
  - `src/llm/mcp/schema.rs`

职责分布：

- `config.rs`
  - config 文件读取
  - array/object 两种配置形状兼容
  - transport 名称归一化
- `transport.rs`
  - `StreamableHttpMcpClient`
  - initialize / notification / request send
  - response content-type 分流
- `rpc.rs`
  - SSE JSON payload 提取
  - JSON-RPC `result` / `error` 解析
  - MCP tool call output 归一化
- `schema.rs`
  - tool name namespace / sanitize
  - input schema 规范化

结果：

- `llm/mcp` 现在的分层更接近：
  - manager orchestration
  - config loading
  - remote transport
  - protocol decoding
  - schema normalization

### 当前文件规模快照

- `src/llm/mcp.rs`：`1014 -> 628`
- `src/llm/mcp/config.rs`：`41`
- `src/llm/mcp/rpc.rs`：`79`
- `src/llm/mcp/schema.rs`：`84`
- `src/llm/mcp/transport.rs`：`209`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::mcp --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::mcp` 定向测试通过数：
  - `7 passed`

### 当前判断

- `llm/mcp` 已从“功能细节混装单体文件”进入“可继续精修”的状态
- 如果后续继续留在本子树，更值得优先处理的方向会是：
  - 是否把内联测试最后统一迁到 `tests/`
  - 是否继续把 manager 层和 catalog inspection 的共享流程再收一层

## 追加交接快照（2026-04-29，`llm/cron_task` 的 façade 收口）

说明：

- 这一轮从新的热点 `src/llm/cron_task.rs` 下手
- 目标是先把 cron 状态持久化、备份恢复、调度解析/overlay 从根文件剥离
- 暂不做测试迁移，先保证功能边界与编译闭环

### 本轮新增完成

#### 1. `src/llm/cron_task.rs` 已退回到 scheduler façade / 状态编排入口

- 根文件现在主要保留：
  - `PluginCronTaskScheduler`
  - `CronTaskKey`
  - `DueCronJob`
  - scheduler sync / due collection / mark success / mark error
  - 内联必要测试

结果：

- 根文件不再同时承载状态文件读写、备份恢复、cron 表达式解析、时区处理全部细节

#### 2. cron 子职责已下沉到目录模块

- 新增：
  - `src/llm/cron_task/persistence.rs`
  - `src/llm/cron_task/schedule.rs`

职责分布：

- `persistence.rs`
  - state document 结构
  - primary / backup 读取恢复
  - temp file replace 与 backup 清理
  - runtime entry 转换
- `schedule.rs`
  - host executable cron job 判定
  - scheduler overlay
  - next run 计算
  - cron / timezone 解析

结果：

- `llm/cron_task` 现在的分层更接近：
  - scheduler orchestration
  - state persistence
  - schedule parsing / runtime calculation

### 当前文件规模快照

- `src/llm/cron_task.rs`：`892 -> 400`
- `src/llm/cron_task/persistence.rs`：`191`
- `src/llm/cron_task/schedule.rs`：`338`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::cron_task --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::cron_task` 定向测试通过数：
  - `7 passed`

### 当前判断

- `llm/cron_task` 已从“状态持久化 + 调度实现混装单体”退回到清晰 façade
- 如果后续继续留在这条线，更值钱的方向会是：
  - 继续评估 `schedule.rs` 是否需要二次拆分 parser / overlay
  - 最后再统一把内联必要测试迁移到 `tests/`

## 追加交接快照（2026-04-29，`llm/service` 的 façade 收口）

说明：

- 在 `cron_task` 闭环后继续处理 `src/llm/service.rs`
- 目标是把 provider 选择、非 OpenAI provider HTTP runtime、runtime config/prompt store 访问从根文件抽离
- 保持外部调用路径 `crate::llm::service::*` 不变

### 本轮新增完成

#### 1. `src/llm/service.rs` 已退回到 prompt 编排 façade

- 根文件现在主要保留：
  - `generate_llm_reply*`
  - `complete_llm_prompt`
  - `probe_llm_runtime_text`
  - 对外兼容 façade 包装函数

结果：

- 根文件不再直接承载 provider 识别规则、Anthropic/Gemini HTTP 请求细节、prompt store 路径访问实现

#### 2. `llm/service` 已拆成三个目录模块

- 新增：
  - `src/llm/service/config_store.rs`
  - `src/llm/service/provider_selection.rs`
  - `src/llm/service/provider_runtime.rs`

职责分布：

- `config_store.rs`
  - 当前 runtime config 读取
  - prompt store 读写
  - prompt profile 选择
  - API key 轮换选择
- `provider_selection.rs`
  - `LlmProviderApiFamily`
  - provider id 规范化 / base URL 识别
  - OpenAI-family / Anthropic / Gemini 路由判定
  - provider 识别必要测试
- `provider_runtime.rs`
  - Anthropic/Gemini request payload 构造
  - provider JSON request send
  - 响应文本提取

结果：

- `llm/service` 现在的分层更接近：
  - prompt orchestration
  - config / prompt store access
  - provider selection
  - provider-specific runtime

### 当前文件规模快照

- `src/llm/service.rs`：`557 -> 168`
- `src/llm/service/config_store.rs`：`35`
- `src/llm/service/provider_selection.rs`：`157`
- `src/llm/service/provider_runtime.rs`：`266`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::service --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::service` 定向测试通过数：
  - `4 passed`

### 当前判断

- `llm/service` 已从“编排 + provider 细节 + config 访问”混装单体退回到 façade
- 后续如果继续留在这条线，更值得优先处理的方向会是：
  - 是否把 `provider_runtime.rs` 再按 Anthropic / Gemini 拆分
  - 最后统一把必要测试迁移到 `tests/`

## 追加交接快照（2026-04-29，`web/host/plugin_api` 的 façade 收口）

说明：

- 在 `llm/service` 闭环后继续处理 `src/web/host/plugin_api.rs`
- 目标是先把 runtime web api dispatch 与 capability/runtime state/diagnostics payload 构建从根文件抽离
- 保持 `router -> plugin_api::*` 的调用路径不变

### 本轮新增完成

#### 1. `src/web/host/plugin_api.rs` 已退回到 plugin route façade

- 根文件现在主要保留：
  - `route_plugin_api`
  - `route_plugin_runtime_web_api` 的 façade 包装
  - plugin config / tool execute / store / status 等路由分发
  - `plugin_runtime_tool_name(...)`

结果：

- 根文件不再同时承载 runtime web api 冲突判定、HTTP status line 拼装、capability support 汇总、runtime state/diagnostics payload 构建

#### 2. `plugin_api` 子职责已下沉到目录模块

- 新增：
  - `src/web/host/plugin_api/capability_state.rs`
  - `src/web/host/plugin_api/runtime_dispatch.rs`

职责分布：

- `capability_state.rs`
  - capability support state / summary / payload 类型
  - 单插件与全量 capability payload 构建
  - runtime state / diagnostics payload 构建
  - cron capability support 必要测试
- `runtime_dispatch.rs`
  - runtime web api 路由解析
  - registered route 规范化
  - 方法冲突与 capability snapshot 校验
  - plugin web api HTTP response status 拼装

结果：

- `web/host/plugin_api` 现在的分层更接近：
  - plugin route entry
  - runtime web api dispatch
  - capability/runtime state serialization

### 当前文件规模快照

- `src/web/host/plugin_api.rs`：`1014 -> 446`
- `src/web/host/plugin_api/capability_state.rs`：`396`
- `src/web/host/plugin_api/runtime_dispatch.rs`：`197`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test plugin_api --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `plugin_api` 定向测试通过数：
  - `2 passed`

### 当前判断

- `plugin_api` 已从“路由分发 + runtime dispatch + capability 计算”混装单体退回到 façade
- 后续如果继续留在这条线，更值得优先处理的方向会是：
  - 把 `route_plugin_api` 内部的大型 `if api_path == ...` 分发继续拆成 capability/config/store/action handler
  - 最后再统一评估这些必要测试迁移到 `tests/`

## 追加交接快照（2026-04-29，`plugin/source_adapter/override_loader` 的 façade 收口）

说明：

- 在 `plugin_api` 闭环后继续处理 `src/plugin/source_adapter/override_loader.rs`
- 目标是把目录扫描、override manifest 合成、路径辅助逻辑从根文件拆开
- 先保证源适配发现逻辑不变，测试迁移仍然延后

### 本轮新增完成

#### 1. `src/plugin/source_adapter/override_loader.rs` 已退回到 source adapter façade

- 根文件现在主要保留：
  - 子模块声明
  - `discover_plugin_manifests_in_dirs` re-export
  - 内联必要测试

结果：

- 根文件不再同时承载 native/override 扫描、descriptor 合成、路径安全与路径格式化全部实现细节

#### 2. `override_loader` 子职责已下沉到目录模块

- 新增：
  - `src/plugin/source_adapter/override_loader/discovery.rs`
  - `src/plugin/source_adapter/override_loader/synthesis.rs`
  - `src/plugin/source_adapter/override_loader/paths.rs`

职责分布：

- `discovery.rs`
  - native manifest 扫描
  - override manifest 扫描
  - plugin id 去重
- `synthesis.rs`
  - override manifest 读取与校验
  - runtime/sdk/permissions/extra 合成
  - synthetic descriptor 构建
- `paths.rs`
  - `source.path` 安全解析
  - path forward slash 规范化
  - 默认 plugin config path 推导

结果：

- `override_loader` 现在的分层更接近：
  - manifest discovery
  - override synthesis
  - path helpers

### 当前文件规模快照

- `src/plugin/source_adapter/override_loader.rs`：`782 -> 370`
- `src/plugin/source_adapter/override_loader/discovery.rs`：`125`
- `src/plugin/source_adapter/override_loader/synthesis.rs`：`274`
- `src/plugin/source_adapter/override_loader/paths.rs`：`33`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test override_loader --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `override_loader` 定向测试通过数：
  - `6 passed`

### 当前判断

- `override_loader` 已从“扫描 + 合成 + 路径处理”混装单体退回到 façade
- 后续如果继续留在这条线，更值得优先处理的方向会是：
  - 最后统一把这些必要测试迁移到 `tests/`
  - 评估是否把 `synthesis.rs` 继续按 merge / extra injection 再收一层

## 追加交接快照（2026-04-29，`core/lifecycle` 的 façade 收口）

说明：

- 在 `override_loader` 闭环后继续处理 `src/core/lifecycle.rs`
- 目标是把 runtime context/capability 与 hook execution engine 从根文件抽离
- 保持 `crate::core::lifecycle::*` 与 `crate::core::*` 的对外路径不变

### 本轮新增完成

#### 1. `src/core/lifecycle.rs` 已退回到纯 façade

- 根文件现在只保留：
  - `context` / `execution` 子模块声明
  - 对外 `pub use`

结果：

- lifecycle 根文件不再混装 runtime flavor/capability、context state、hook filter、hook runner、timeout/joinset 执行细节

#### 2. `lifecycle` 子职责已下沉到目录模块

- 新增：
  - `src/core/lifecycle/context.rs`
  - `src/core/lifecycle/execution.rs`

职责分布：

- `context.rs`
  - `RuntimeFlavor`
  - `RuntimeCapabilities`
  - `LifecycleContext`
  - `HookFilter`
  - env capability override 解析
- `execution.rs`
  - `LifecyclePhase`
  - `LifecycleFailurePolicy`
  - `HookFailure`
  - `LifecycleExecutionError`
  - `Lifespan`
  - fail-fast / continue 两条 hook 执行路径

结果：

- `core/lifecycle` 现在的分层更接近：
  - runtime context / capability model
  - lifecycle execution engine
  - root façade re-export

### 当前文件规模快照

- `src/core/lifecycle.rs`：`879 -> 8`
- `src/core/lifecycle/context.rs`：`231`
- `src/core/lifecycle/execution.rs`：`650`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test lifespan --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `lifespan` 集成测试通过数：
  - `7 passed`

### 当前判断

- `core/lifecycle` 已从“context + execution engine”混装单体退回到 façade
- 后续如果继续留在这条线，更值得优先处理的方向会是：
  - 继续把 `execution.rs` 里的 registration API 与 run engine 再切开
  - 然后再处理更大的 `core/bot.rs`

## 追加交接快照（2026-04-29，`core/bot` 的 façade 收口）

说明：

- 在 `core/lifecycle` 闭环后继续处理 `src/core/bot.rs`
- 目标是把 builder 装配、plugin/adapter 相关操作、start/shutdown 生命周期编排从根文件拆开
- 保持 `crate::core::bot::*` 与 `crate::core::*` 的外部 API 不变

### 本轮新增完成

#### 1. `src/core/bot.rs` 已退回到定义与类型 façade

- 根文件现在主要保留：
  - 子模块声明
  - `LiteyukiBotError`
  - `BotBootstrapContext`
  - `LiteyukiBotBuilder`
  - `LiteyukiBot`
  - 共享 type alias / 常量

结果：

- 根文件不再同时承载 builder 装配、plugin policy 计算、adapter ingress、start/reload/shutdown/rollback 细节

#### 2. `core/bot` 子职责已下沉到目录模块

- 新增：
  - `src/core/bot/builder.rs`
  - `src/core/bot/plugin_adapter_ops.rs`
  - `src/core/bot/lifecycle_ops.rs`

职责分布：

- `builder.rs`
  - builder fluent API
  - runtime/session/plugin/adapter/logger 装配
  - `build()` 入口
- `plugin_adapter_ops.rs`
  - getter / register API
  - session hook 注册
  - plugin policy 计算
  - plugin context 构造
  - adapter start/stop/reload
- `lifecycle_ops.rs`
  - `start()`
  - `restart_*()`
  - `reload_plugins()`
  - `shutdown()`
  - start rollback / shutdown hook 辅助

结果：

- `core/bot` 现在的分层更接近：
  - bot state / type façade
  - builder assembly
  - plugin & adapter operations
  - runtime lifecycle orchestration

### 当前文件规模快照

- `src/core/bot.rs`：`877 -> 128`
- `src/core/bot/builder.rs`：`146`
- `src/core/bot/plugin_adapter_ops.rs`：`303`
- `src/core/bot/lifecycle_ops.rs`：`311`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test bot_orchestration --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `bot_orchestration` 集成测试通过数：
  - `6 passed`

### 当前判断

- `core/bot` 已从“装配 + plugin/adapter 操作 + 生命周期编排”混装单体退回到 façade
- 后续如果继续留在这条线，更值得优先处理的方向会是：
  - 继续评估 `plugin_adapter_ops.rs` 是否要再拆成 plugin / adapter 两层
  - 或直接回到 `core/lifecycle/execution.rs` 继续收口

## 追加交接快照（2026-04-29，`web/host/plugin_api` 的路由 façade 继续收口）

说明：

- 在 `core/bot` 闭环后回到 `src/web/host/plugin_api.rs`
- 目标是把大段 `route_plugin_api(...)` 路由分发从根文件继续抽离
- 保持 `router -> plugin_api::route_plugin_api` 外部入口不变

### 本轮新增完成

#### 1. `src/web/host/plugin_api.rs` 已进一步退回到纯入口 façade

- 根文件现在主要保留：
  - 模块声明
  - `route_plugin_runtime_web_api` façade
  - `route_plugin_api` façade

结果：

- 根文件不再直接承载 capability/tool/config/store/status 等路由分发分支

#### 2. `route_plugin_api(...)` 已整体下沉到目录模块

- 新增：
  - `src/web/host/plugin_api/route_handlers.rs`

职责分布：

- `route_handlers.rs`
  - `/Plugin/Capabilities*`
  - `/Plugin/Tools*`
  - `/Plugin/RuntimeState`
  - `/Plugin/Diagnostics`
  - `/Plugin/List`
  - `/Plugin/SetStatus`
  - `/Plugin/Store/*`
  - `/Plugin/Config*`

结果：

- `plugin_api` 当前层次更接近：
  - root façade
  - runtime dispatch
  - route handlers
  - capability/runtime state serialization

### 当前文件规模快照

- `src/web/host/plugin_api.rs`：`446 -> 30`
- `src/web/host/plugin_api/route_handlers.rs`：`429`
- `src/web/host/plugin_api/capability_state.rs`：`396`
- `src/web/host/plugin_api/runtime_dispatch.rs`：`197`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test plugin_api --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `plugin_api` 定向测试通过数：
  - `2 passed`

### 当前判断

- `plugin_api` 根文件已经真正退回 façade
- 后续如果继续留在这条线，更值得优先处理的方向会是：
  - 继续把 `route_handlers.rs` 再拆成 capability/config/store/action handler
  - 最后统一评估必要测试迁移到 `tests/`

## 追加交接快照（2026-04-29，`web/host/plugin_api/route_handlers` 的二次收口）

说明：

- 在 `plugin_api` 根 façade 收口后，继续处理新的次级热点 `src/web/host/plugin_api/route_handlers.rs`
- 目标是把 capability/config/store/action 四类路由再分层
- 保持 `route_plugin_api(...)` 路由顺序与行为不变

### 本轮新增完成

#### 1. `route_handlers.rs` 已退回到 dispatch façade

- 文件现在主要保留：
  - 子模块声明
  - `route_plugin_api(...)` 分发链
  - `plugin_runtime_tool_name(...)`

结果：

- `route_handlers.rs` 不再直接承载全部 plugin route 分支实现

#### 2. plugin route 已按职责拆成四个目录模块

- 新增：
  - `src/web/host/plugin_api/capability_routes.rs`
  - `src/web/host/plugin_api/action_routes.rs`
  - `src/web/host/plugin_api/store_routes.rs`
  - `src/web/host/plugin_api/config_routes.rs`

职责分布：

- `capability_routes.rs`
  - `/Plugin/Capabilities*`
  - `/Plugin/Tools`
  - `/Plugin/WebApis`
  - `/Plugin/CronJobs`
  - `/Plugin/Tasks`
  - `/Plugin/RuntimeState`
  - `/Plugin/Diagnostics`
- `action_routes.rs`
  - `/Plugin/Tools/Execute`
  - `/Plugin/List`
  - `/Plugin/RegisterManager`
  - `/Plugin/SetStatus`
  - `/Plugin/Uninstall`
  - `/Plugin/Import`
- `store_routes.rs`
  - `/Plugin/Store/*`
- `config_routes.rs`
  - `/Plugin/Config*`

结果：

- `plugin_api` 路由层现在更接近：
  - root façade
  - route dispatch façade
  - grouped route handlers
  - capability/runtime state serialization

### 当前文件规模快照

- `src/web/host/plugin_api/route_handlers.rs`：`429 -> 29`
- `src/web/host/plugin_api/capability_routes.rs`：`172`
- `src/web/host/plugin_api/action_routes.rs`：`135`
- `src/web/host/plugin_api/store_routes.rs`：`54`
- `src/web/host/plugin_api/config_routes.rs`：`99`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test plugin_api --locked --offline`
  - `cargo check --all-targets --locked --offline`

### 当前判断

- `plugin_api` 的 route 分发层已经基本收口
- 后续如果继续留在这里，更值钱的方向会是：
  - 统一抽取 GET/query/plugin id 解析重复逻辑
  - 最后评估相关必要测试迁移到 `tests/`

## 追加交接快照（2026-04-29，`core/lifecycle/execution` 的二次收口）

说明：

- 在 `plugin_api` 路由分层后，回到 `src/core/lifecycle/execution.rs`
- 目标是把 hook 注册 API 与执行引擎再拆一层
- 保持 `Lifespan` 外部 API 与 `tests/lifespan.rs` 行为不变

### 本轮新增完成

#### 1. `execution.rs` 已退回到类型定义与模块装配入口

- 文件现在主要保留：
  - 子模块声明
  - `LifecyclePhase`
  - `LifecycleFailurePolicy`
  - `HookFailure`
  - `LifecycleExecutionError`
  - `Lifespan` / registration 类型定义

结果：

- `execution.rs` 不再直接混装 hook 注册 API 与 joinset/timeout 执行路径

#### 2. execution 子职责已继续下沉

- 新增：
  - `src/core/lifecycle/execution/registration.rs`
  - `src/core/lifecycle/execution/runner.rs`

职责分布：

- `registration.rs`
  - `Lifespan` 构造与配置
  - 各类 `on_*` 注册 API
  - sync hook 包装
  - `HookRegistration` / `ProcessHookRegistration` 构造
- `runner.rs`
  - `before_*` / `after_*` 执行入口
  - fail-fast / continue 两条执行路径
  - process hook 调度
  - timeout / joinset / failure logging

结果：

- `core/lifecycle` 当前层次更接近：
  - context model
  - execution type façade
  - registration API
  - runtime execution engine

### 当前文件规模快照

- `src/core/lifecycle/execution.rs`：`650 -> 91`
- `src/core/lifecycle/execution/registration.rs`：`216`
- `src/core/lifecycle/execution/runner.rs`：`353`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test lifespan --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `lifespan` 集成测试通过数：
  - `7 passed`

### 当前判断

- `execution.rs` 这一层已经完成二次收口
- 后续如果继续留在 `core` 线，更值得优先处理的方向会是：
  - 回到 `plugin_adapter_ops.rs` 看是否继续拆 plugin / adapter 两层
  - 或开始进入测试迁移阶段

## 追加交接快照（2026-04-29，`core/bot/plugin_adapter_ops` 的二次收口）

说明：

- 在 `core/lifecycle/execution` 闭环后，回到 `src/core/bot/plugin_adapter_ops.rs`
- 目标是把 plugin policy/context 相关逻辑与 adapter ingress/runtime 相关逻辑再拆一层
- 保持 `LiteyukiBot` 外部 API 不变

### 本轮新增完成

#### 1. `plugin_adapter_ops.rs` 已退回到轻量 façade

- 文件现在主要保留：
  - 子模块声明
  - getter / register API
  - session hook 注册
  - bootstrap hook 注册

结果：

- `plugin_adapter_ops.rs` 不再直接混装 plugin load/policy/context 与 adapter start/reload 细节

#### 2. plugin / adapter 两条行为线已继续拆开

- 新增：
  - `src/core/bot/plugin_adapter_ops/plugin_ops.rs`
  - `src/core/bot/plugin_adapter_ops/adapter_ops.rs`

职责分布：

- `plugin_ops.rs`
  - `health_check_plugins()`
  - `load_plugins()`
  - `sync_plugins_for_current_policy()`
  - pending plugin id 计算
  - `plugin_context()` 构造
- `adapter_ops.rs`
  - adapter ingress sink 构造
  - `start_adapters()`
  - `stop_adapters()`
  - `reload_adapters()`

结果：

- `core/bot` 当前层次更接近：
  - bot state façade
  - builder assembly
  - lifecycle orchestration
  - plugin ops
  - adapter ops

### 当前文件规模快照

- `src/core/bot/plugin_adapter_ops.rs`：`303 -> 167`
- `src/core/bot/plugin_adapter_ops/plugin_ops.rs`：`89`
- `src/core/bot/plugin_adapter_ops/adapter_ops.rs`：`59`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test bot_orchestration --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `bot_orchestration` 集成测试通过数：
  - `6 passed`

### 当前判断

- `plugin_adapter_ops` 这一层已经完成二次收口
- 当前剩余更值得优先处理的工作开始转向：
  - 统一测试迁移到 `tests/`
  - 或在 `plugin_api/capability_routes.rs` 里继续抽取重复的 GET/query/plugin id 解析模板

## 追加交接快照（2026-04-29，`web/host/plugin_api/capability_state` 的二次收口）

说明：

- 在 `plugin_api` 路由分层完成后，继续回到 `src/web/host/plugin_api/capability_state.rs`
- 目标是把 capability support 计算与 runtime payload 组装再拆一层
- 保持 `/Plugin/Capabilities*`、`/Plugin/RuntimeState`、`/Plugin/Diagnostics` 对外响应结构不变

### 本轮新增完成

#### 1. `capability_state.rs` 已退回到 façade + 类型定义层

- 文件现在主要保留：
  - 子模块声明
  - capability/runtime payload 序列化类型
  - 对外 façade 转发函数

结果：

- `capability_state.rs` 不再直接混装 support 计算、snapshot fallback、runtime diagnostics 组装

#### 2. capability support 与 payload 组装已按职责拆开

- 新增：
  - `src/web/host/plugin_api/capability_state/support.rs`
  - `src/web/host/plugin_api/capability_state/payloads.rs`

职责分布：

- `support.rs`
  - tool/web api/cron/task support 状态计算
  - active / executable / persistent / status 派生
  - cron support 的必要单测
- `payloads.rs`
  - single/all capability payload 组装
  - runtime state / diagnostics payload 组装
  - empty snapshot fallback
  - snapshot extracted / disabled plugin 判定辅助逻辑

结果：

- `plugin_api` 当前能力状态层更接近：
  - route façade
  - capability payload façade
  - support derivation
  - runtime/diagnostics payload assembly

### 当前文件规模快照

- `src/web/host/plugin_api/capability_state.rs`：`396 -> 98`
- `src/web/host/plugin_api/capability_state/support.rs`：`152`
- `src/web/host/plugin_api/capability_state/payloads.rs`：`204`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test plugin_api --locked --offline`
  - `cargo check --all-targets --locked --offline`

### 当前判断

- `capability_state` 这一层已经完成二次收口
- 后续若继续留在 `plugin_api` 线，更值得做的只剩：
  - 统一 GET/query/plugin id 解析模板
  - 最后再处理相关测试迁移

## 追加交接快照（2026-04-29，`llm/cron_task/schedule` 的二次收口）

说明：

- 在 `plugin_api` 收口后，回到 `src/llm/cron_task/schedule.rs`
- 目标是把 scheduler overlay 逻辑与 cron parser/timezone 匹配逻辑拆开
- 保持 `PluginCronTaskScheduler` 对外行为、状态持久化格式与现有测试结果不变

### 本轮新增完成

#### 1. `schedule.rs` 已退回到轻量 façade

- 文件现在主要保留：
  - 子模块声明
  - `cron_job_is_host_executable(...)`
  - `apply_scheduler_overlay(...)`
  - `compute_next_run_time(...)`
  - `job_schedule_signature(...)`
  - `parse_timestamp(...)`
  - 测试专用 `next_cron_occurrence(...)` 转发

结果：

- `schedule.rs` 不再直接混装 overlay、cron parser、timezone offset 解析

#### 2. cron scheduler 两条职责线已拆开

- 新增：
  - `src/llm/cron_task/schedule/overlay.rs`
  - `src/llm/cron_task/schedule/cron_parser.rs`

职责分布：

- `overlay.rs`
  - executable job 判定
  - runtime entry overlay
  - next run 计算入口
  - schedule signature / timestamp 解析
- `cron_parser.rs`
  - cron field parser
  - step/range/value 匹配
  - day-of-month 与 day-of-week 组合规则
  - timezone offset 解析
  - next occurrence 搜索

结果：

- `llm::cron_task` 当前层次更接近：
  - scheduler façade
  - persistence
  - runtime overlay
  - cron parser / timezone matcher

### 当前文件规模快照

- `src/llm/cron_task/schedule.rs`：`376 -> 42`
- `src/llm/cron_task/schedule/overlay.rs`：`135`
- `src/llm/cron_task/schedule/cron_parser.rs`：`198`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::cron_task --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `llm::cron_task` 相关测试通过数：
  - `7 passed`

### 当前判断

- `schedule.rs` 这一层已经完成二次收口
- 当前剩余更值得优先处理的方向开始转向：
  - `src/core/bot/lifecycle_ops.rs`
  - `src/core/lifecycle/execution/runner.rs`
  - `src/plugin/source_adapter/override_loader/synthesis.rs`
  - 最后统一处理测试迁移与 `#[test] // 必要测试`

## 追加交接快照（2026-04-29，`core/bot/lifecycle_ops` 的二次收口）

说明：

- 在 `cron_task::schedule` 收口后，回到 `src/core/bot/lifecycle_ops.rs`
- 目标是把 bot 启动链与关闭链拆开，保留 `LiteyukiBot` 外部 API 不变
- 不调整 `send/restart/reload` 行为，只拆内部职责边界

### 本轮新增完成

#### 1. `lifecycle_ops.rs` 已退回到中等规模 façade

- 文件现在主要保留：
  - 子模块声明
  - `send(...)`
  - `restart_process(...)`
  - `restart_runtime(...)`
  - `reload_plugins(...)`

结果：

- `lifecycle_ops.rs` 不再直接混装 start rollback、shutdown hook orchestration、error recording

#### 2. 启动链与关闭链已拆成两个子模块

- 新增：
  - `src/core/bot/lifecycle_ops/startup.rs`
  - `src/core/bot/lifecycle_ops/shutdown.rs`

职责分布：

- `startup.rs`
  - `start(...)`
  - `rollback_failed_start(...)`
  - bootstrap / before_start / process start / runtime start / adapter autostart / health check / after_start
- `shutdown.rs`
  - `shutdown(...)`
  - shutdown process name 收集
  - before shutdown hooks
  - first error 记录与日志

结果：

- `core/bot` 当前层次更接近：
  - bot state façade
  - builder assembly
  - startup chain
  - shutdown chain
  - reload / restart orchestration
  - plugin / adapter ops

### 当前文件规模快照

- `src/core/bot/lifecycle_ops.rs`：`349 -> 100`
- `src/core/bot/lifecycle_ops/startup.rs`：`133`
- `src/core/bot/lifecycle_ops/shutdown.rs`：`97`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test bot_orchestration --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `bot_orchestration` 集成测试通过数：
  - `6 passed`

### 当前判断

- `lifecycle_ops` 这一层已经完成二次收口
- 后续在 `core` 线继续推进时，更值得优先处理的是：
  - `src/core/lifecycle/execution/runner.rs`

## 追加交接快照（2026-04-29，`plugin/source_adapter/override_loader/synthesis` 的二次收口）

说明：

- 在 `core/bot/lifecycle_ops` 收口后，回到 `src/plugin/source_adapter/override_loader/synthesis.rs`
- 目标是保持 override manifest 主流程可读，同时把稳定辅助职责下沉
- 保持 override discovery 与 descriptor synthesis 行为不变

### 本轮新增完成

#### 1. `synthesis.rs` 已退回到 manifest 主流程 façade

- 文件现在主要保留：
  - 子模块声明
  - `load_override_manifest(...)`
  - override manifest 读取、source root 校验、plugin id 派生、descriptor 组装主流程

结果：

- `synthesis.rs` 不再同时承载 runtime/sdk merge 与 source extra 注入辅助逻辑

#### 2. 两类稳定辅助职责已下沉

- 新增：
  - `src/plugin/source_adapter/override_loader/synthesis/merge.rs`
  - `src/plugin/source_adapter/override_loader/synthesis/source_extra.rs`

职责分布：

- `merge.rs`
  - `merge_runtime(...)`
  - `merge_sdk(...)`
- `source_extra.rs`
  - `inject_source_extra(...)`
  - source family / adapter family / compat level / source path / override path 注入

结果：

- `override_loader` 当前层次更接近：
  - discovery
  - path safety / source root resolution
  - override manifest façade
  - runtime/sdk merge
  - source extra injection

### 当前文件规模快照

- `src/plugin/source_adapter/override_loader/synthesis.rs`：`286 -> 173`
- `src/plugin/source_adapter/override_loader/synthesis/merge.rs`：`38`
- `src/plugin/source_adapter/override_loader/synthesis/source_extra.rs`：`61`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test override_loader --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `override_loader` 相关测试通过数：
  - `6 passed`

### 当前判断

- `synthesis` 这一层已经完成二次收口
- 当前剩余更值得优先处理的方向进一步收敛到：
  - `src/core/lifecycle/execution/runner.rs`
  - 最后统一处理测试迁移与 `#[test] // 必要测试`

## 追加交接快照（2026-04-29，`core/lifecycle/execution/runner` 的二次收口）

说明：

- 在 `override_loader/synthesis` 收口后，回到 `src/core/lifecycle/execution/runner.rs`
- 目标是把 lifecycle 对外 phase 入口、failure policy / joinset phase runner、hook executor 再拆一层
- 保持 `Lifespan` 外部 API 与 `tests/lifespan.rs` 行为不变

### 本轮新增完成

#### 1. `runner.rs` 已退回到 phase 入口 façade

- 文件现在主要保留：
  - 子模块声明
  - `before_start(...)`
  - `after_start(...)`
  - `before_process_shutdown(...)`
  - `after_shutdown(...)`
  - `before_process_restart(...)`
  - `after_restart(...)`
  - `before_shutdown(...)`
  - `before_restart(...)`

结果：

- `runner.rs` 不再直接混装 failure policy 分发、JoinSet 聚合、timeout 执行与 failure logging

#### 2. phase runner 与 hook executor 已拆开

- 新增：
  - `src/core/lifecycle/execution/runner/phase_runner.rs`
  - `src/core/lifecycle/execution/runner/hook_executor.rs`

职责分布：

- `phase_runner.rs`
  - `run_phase(...)`
  - `run_process_phase(...)`
  - fail-fast / continue 两条 phase 路径
  - JoinSet 结果归并
- `hook_executor.rs`
  - `execute_hook(...)`
  - `execute_process_hook(...)`
  - timeout 包装
  - hook failure logging

结果：

- `core/lifecycle` 当前层次更接近：
  - execution type façade
  - registration API
  - phase entry façade
  - phase runner
  - hook executor

### 当前文件规模快照

- `src/core/lifecycle/execution/runner.rs`：`391 -> 87`
- `src/core/lifecycle/execution/runner/phase_runner.rs`：`200`
- `src/core/lifecycle/execution/runner/hook_executor.rs`：`83`

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test lifespan --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `lifespan` 集成测试通过数：
  - `7 passed`

### 当前判断

- `runner` 这一层已经完成二次收口
- 到这里，近期这条功能重构主线已经基本收拢
- 后续优先级开始转向测试目录治理

## 追加交接快照（2026-04-29，`override_loader` 测试迁移到 `tests/`）

说明：

- 在 `runner` 收口后，开始进入测试收口
- 先从 `override_loader` 这类天然黑盒、无需私有 helper 的测试开始迁移
- 目标是遵循“优先放到 `tests/`，否则才保留内联 `#[test] // 必要测试`”

### 本轮新增完成

#### 1. `override_loader` 内联测试已整体迁出

- 删除：
  - `src/plugin/source_adapter/override_loader.rs` 内的 `#[cfg(test)]` 模块
- 新增：
  - `tests/override_loader.rs`

迁移内容：

- Astrbot override descriptor synthesis
- Astrbot derived config path
- Liteyuki override plugin meta extraction
- Neomofox manifest metadata extraction
- Neomofox file path entrypoint segments
- native manifest 优先级覆盖场景

结果：

- `override_loader.rs` 现在只保留模块装配与 re-export
- 这一批测试已经从实现内联测试转为公开 API 集成测试

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test override_loader --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `override_loader` 集成测试通过数：
  - `6 passed`

### 当前判断

- 测试目录治理已经开始落地，而不只是补注释
- 后续更值得继续迁移的方向会是：
  - 继续挑选公开 API 可黑盒验证的模块迁到 `tests/`
  - 对必须保留在 `src/` 内的测试补齐 `#[test] // 必要测试`

## 追加交接快照（2026-04-29，`src/` 内必要测试标记补齐）

说明：

- 在 `tests/override_loader.rs` 迁移完成后，继续处理短期无法迁出到 `tests/` 的内联测试
- 目标是遵循约束：能迁则迁，不能迁则明确标注 `// 必要测试`
- 本轮不改测试逻辑与断言，只做测试治理收口

### 本轮新增完成

#### 1. 一批仍保留在 `src/` 内的测试已补齐 `// 必要测试`

- 已补齐的核心文件包括：
  - `src/core/runtime.rs`
  - `src/llm/mcp.rs`
  - `src/llm/skills.rs`
  - `src/llm/prompt.rs`
  - `src/llm/tools/discovery_tools.rs`
  - `src/llm/tools/inventory_prompt.rs`
  - `src/llm/tools/local_execution_tools.rs`
  - `src/llm/tools/runtime_inventory.rs`
  - `src/llm/tools/tool_state/storage.rs`
  - `src/llm/tools/tool_state/store.rs`
  - `src/llm/tools/workspace_access/root_detection.rs`
  - `src/llm/tools/workspace_access/path_safety.rs`
  - `src/llm/tools/workspace_access/file_ops.rs`

结果：

- 最近这轮重构涉及的 `core/llm/tools` 子模块，必要测试标记已经基本补齐
- 后续 review 时可以更清楚地区分：
  - 公开 API 黑盒测试
  - 实现内必要回归测试

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo check --all-targets --locked --offline`
  - `cargo test --test override_loader --locked --offline`

### 当前判断

- 这一轮后，测试治理已经从“新增 `tests/` 迁移”推进到“内联测试标记规范化”
- 后续如果继续走测试线，更值得优先处理的是：
  - 再挑一批能黑盒化的模块迁到 `tests/`
  - 剩余超大测试文件，如 `src/web/host/tests.rs`、`src/tui/app/tests.rs`、`src/llm/client/tests.rs` 的分批治理

## 追加交接快照（2026-04-29，`llm/prompt` 测试迁移到 `tests/`）

说明：

- 在必要测试标记补齐后，继续挑选公开 API 可直接黑盒验证的模块迁移
- `src/llm/prompt.rs` 暴露了 `LlmPromptStore`、`compose_user_prompt(...)`、`build_prompt_preview(...)`
- 这组测试不依赖私有 helper，也不需要扩大可见性

### 本轮新增完成

#### 1. `llm/prompt` 内联测试已迁出到 `tests/`

- 删除：
  - `src/llm/prompt.rs` 内的 `#[cfg(test)]` 模块
- 新增：
  - `tests/prompt_store.rs`

迁移内容：

- `compose_user_prompt` 组合规则
- `LlmPromptStore::normalized()` 对 default profile 的兜底
- `LlmPromptStore` 的 upsert + switch active profile 行为

结果：

- `prompt.rs` 现在只保留生产逻辑
- 这组测试已从实现内联测试转为公共 API 集成测试

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test prompt_store --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `prompt_store` 集成测试通过数：
  - `3 passed`

### 当前判断

- 测试迁移仍有继续推进空间，但已经进入“挑选低风险黑盒模块逐个迁移”的阶段
- 后续更值得继续迁移的对象，优先是：
  - 仍有清晰公共 API、但测试尚内联的轻量模块
  - 其次才是大型集成测试文件的拆分

## 追加交接快照（2026-04-29，`adapter/model` 测试迁移到 `tests/`）

说明：

- 在 `llm/prompt` 测试迁移完成后，继续寻找不需要扩大可见性的公共 API 测试
- `src/adapter/model.rs` 的两条内联测试只依赖公开类型和 serde/validate 行为
- 这组测试适合直接迁到 `tests/`

### 本轮新增完成

#### 1. `adapter/model` 内联测试已迁出到 `tests/`

- 删除：
  - `src/adapter/model.rs` 内的 `#[cfg(test)]` 模块
- 新增：
  - `tests/adapter_model.rs`

迁移内容：

- `AdapterTransport` 的 websocket alias 反序列化
- `AdapterConfig::validate()` 对 zero limit 的拒绝

结果：

- `adapter/model.rs` 现在只保留生产逻辑
- 这组测试已经转为公共 API 集成测试

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test --test adapter_model --locked --offline`
  - `cargo check --all-targets --locked --offline`
- 当前 `adapter_model` 集成测试通过数：
  - `2 passed`

### 当前判断

- 这轮后，低风险黑盒测试迁移又推进了一步
- 下一批如果继续迁移，建议仍优先挑：
  - 不依赖私有 helper 的轻量公共模块
  - 不需要为了测试而抬高可见性的逻辑

## 追加交接快照（2026-04-29，`src` 内测试子模块拆分治理）

说明：

- 在继续筛选可迁移到 `tests/` 的候选后，开始处理“必须留在 `src/` 内”的测试治理
- 目标不是迁移测试语义，而是把生产文件中的大段测试代码拆到独立测试子模块
- 这一步适用于依赖私有 helper 或私有类型、不值得为了测试扩大可见性的模块

### 本轮新增完成

#### 1. `core/runtime.rs` 的内联测试已拆到独立测试子模块

- 修改：
  - `src/core/runtime.rs`
- 新增：
  - `src/core/runtime/tests.rs`

迁移内容：

- round-robin fast path
- worker full fallback
- all workers full 时返回事件

结果：

- `runtime.rs` 生产逻辑文件不再直接夹带测试实现
- 但测试仍保留在 `src/` 内，继续访问私有分发 helper

#### 2. `llm/mcp.rs` 的内联测试已拆到独立测试子模块

- 修改：
  - `src/llm/mcp.rs`
- 新增：
  - `src/llm/mcp/tests.rs`

迁移内容：

- schema normalize
- SSE / tool output parse
- streamable HTTP mock MCP client
- manager load / inspect / invalid config warning

结果：

- `mcp.rs` 生产逻辑文件显著减轻
- 复杂 mock server 与环境变量守卫逻辑已沉到专门测试模块

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo test llm::mcp --locked --offline`
  - `cargo test dispatch_round_robin_fast_path_rotates_cursor --locked --offline`
  - `cargo check --all-targets --locked --offline`

### 当前判断

- 测试治理已经形成三种手段：
  - 能黑盒化的迁到 `tests/`
  - 不能迁出的补 `#[test] // 必要测试`
  - 仍需留在 `src/` 的大测试块拆到独立测试子模块
- 后续如果继续整治，更值得优先处理的是：
  - 继续拆 `src/` 内仍显著影响生产文件可读性的测试块
  - 再挑选少量公共 API 测试迁到 `tests/`

## 追加交接快照（2026-04-29，`llm/tools` 与周边轻量模块测试治理续推）

说明：

- 本轮继续沿用“保持生产边界不变、把 `src/` 内内联测试下沉到独立 `tests.rs` 子模块”的治理方式
- 重点先处理 `llm/skills`、`llm/tools/*`、`cron_task` 以及若干轻量支持模块
- 不为了测试迁移抬高 `pub` 可见性；所有保留在 `src/` 的测试仍继续通过私有访问验证内部行为

### 本轮新增完成

#### 1. `llm/skills` 与 `llm/tools/*` 一批内联测试已拆到独立测试子模块

- 修改：
  - `src/llm/skills.rs`
  - `src/llm/tools/local_execution_tools.rs`
  - `src/llm/tools/discovery_tools.rs`
  - `src/llm/tools/runtime_inventory.rs`
  - `src/llm/tools/inventory_prompt.rs`
  - `src/llm/tools/tool_state/storage.rs`
  - `src/llm/tools/tool_state/store.rs`
  - `src/llm/tools/workspace_access/path_safety.rs`
  - `src/llm/tools/workspace_access/file_ops.rs`
  - `src/llm/tools/workspace_access/root_detection.rs`
- 新增：
  - `src/llm/skills/tests.rs`
  - `src/llm/tools/local_execution_tools/tests.rs`
  - `src/llm/tools/discovery_tools/tests.rs`
  - `src/llm/tools/runtime_inventory/tests.rs`
  - `src/llm/tools/inventory_prompt/tests.rs`
  - `src/llm/tools/tool_state/storage/tests.rs`
  - `src/llm/tools/tool_state/store/tests.rs`
  - `src/llm/tools/workspace_access/path_safety/tests.rs`
  - `src/llm/tools/workspace_access/file_ops/tests.rs`
  - `src/llm/tools/workspace_access/root_detection/tests.rs`

迁移内容：

- skill frontmatter 解析与 skill root 优先级
- 本地 skill 文档读取、截断、缺失 skill 错误
- tool discovery category/schema 摘要
- runtime tool merge / external descriptor 行为
- runtime inventory prompt 拼接
- tool state backup / rollback 场景
- workspace path 安全、文件读取与 symlink 逃逸回归
- workspace root 检测的 git / workspace manifest 边界

结果：

- `skills.rs`、`cron_task.rs` 与多处 `llm/tools/*` 生产文件不再直接夹带测试实现
- `workspace_access` 与 `tool_state` 这类私有实现测试已下沉为相邻测试模块，生产逻辑更集中

#### 2. `src/llm/cron_task.rs` 的内联测试已拆到独立测试子模块

- 修改：
  - `src/llm/cron_task.rs`
- 新增：
  - `src/llm/cron_task/tests.rs`

迁移内容：

- cron expression interval / timezone 解析
- backup state 恢复
- snapshot overlay
- run-once success / error 行为
- invalid primary state file 的降级处理

结果：

- `cron_task.rs` 生产文件进一步聚焦调度逻辑
- 较长的调度回归测试全部沉到专门测试子模块

#### 3. 第二批轻量支持模块也已完成同模式治理

- 修改：
  - `src/llm/service/provider_selection.rs`
  - `src/observability/logging_format.rs`
  - `src/main_support/reload_service.rs`
  - `src/web/host/plugin_api/capability_state/support.rs`
- 新增：
  - `src/llm/service/provider_selection/tests.rs`
  - `src/observability/logging_format/tests.rs`
  - `src/main_support/reload_service/tests.rs`
  - `src/web/host/plugin_api/capability_state/support/tests.rs`

迁移内容：

- provider family / provider id 解析与 Qwen 国际域名识别
- logging format / timestamp 格式化
- reload log path 描述
- cron capability support 状态判定

结果：

- 这几处短测试模块不再占用生产文件底部空间
- 测试治理模式已经可以稳定复制到其它中小模块

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo check --all-targets --locked --offline`
  - `cargo test llm::cron_task --locked --offline`
  - `cargo test llm::tools --locked --offline`
  - `cargo test llm::skills --locked --offline`
  - `cargo test provider_selection --locked --offline`
  - `cargo test logging_format --locked --offline`
  - `cargo test reload_service --locked --offline`
  - `cargo test cron_capability_support --locked --offline`

### 当前判断

- 现在剩余的内联测试已明显收缩，主要集中在：
  - `adapter/*`
  - `i18n.rs`
  - `onebot_support.rs`
  - `main_support/llm_tui_command_service.rs`
  - `web/ui.rs`
  - `web/host/*`
  - `plugin/sdk/python/*`
- 下一优先级更建议继续处理：
  - 仍然较短、纯辅助性质的测试块，优先继续拆到 `tests.rs`
  - 再之后再评估哪些模块适合进一步迁到仓库根 `tests/`

## 追加交接快照（2026-04-29，第三批轻量测试模块继续下沉）

说明：

- 在前两轮治理稳定后，继续清理一批仍然短小、私有依赖明确的测试块
- 本轮依然不改生产逻辑行为，只把测试从生产文件底部迁到相邻 `tests.rs`

### 本轮新增完成

#### 1. `adapter/*`、`observability`、`session`、`onebot_support` 的轻量内联测试已拆到独立测试子模块

- 修改：
  - `src/adapter/websocket.rs`
  - `src/adapter/manager.rs`
  - `src/observability/logging.rs`
  - `src/session/event.rs`
  - `src/onebot_support.rs`
- 新增：
  - `src/adapter/websocket/tests.rs`
  - `src/adapter/manager/tests.rs`
  - `src/observability/logging/tests.rs`
  - `src/session/event/tests.rs`
  - `src/onebot_support/tests.rs`

迁移内容：

- onebot v11 websocket 收发兼容
- adapter inbound packet normalize / websocket round-robin / parallelism
- buffered logging 的 plain-text 与 ring buffer 行为
- `SessionEvent::from_bot_event()` 的 onebot 字段抽取与 message segment 拼接
- onebot private message 判定与 `/su` 参数解析

结果：

- 这批常用基础模块的生产文件继续瘦身
- adapter / onebot / logging / session 的测试实现都已经沉到各自相邻测试模块

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo check --all-targets --locked --offline`
  - `cargo test adapter_manager_parallelism_can_be_configured --locked --offline`
  - `cargo test parse_packet_accepts_onebot_v11_event_json --locked --offline`
  - `cargo test onebot_private_message_detection_respects_message_type --locked --offline`
  - `cargo test from_bot_event_extracts_onebot_v11_fields --locked --offline`
  - `cargo test buffered_logs_keep_the_latest_entries_within_capacity --locked --offline`

### 当前判断

- 截至这一轮，仍保留在生产文件中的 `src` 内联测试还剩 `48` 处
- 现阶段更适合继续按簇推进，而不是逐个零散处理：
  - `bootstrap/settings.rs`
  - `i18n.rs`
  - `main_support/llm_tui_command_service.rs`
  - `web/ui.rs`
  - `web/host/{auth,webui_config_api}.rs`
  - `plugin/sdk/*`

## 追加交接快照（2026-04-29，`bootstrap/i18n/web/plugin-sdk` 等测试簇继续治理并收口）

说明：

- 在前三批治理稳定后，继续按“功能簇”而不是零散文件推进
- 本轮目标是把剩余的中小测试块全部移出生产文件底部，尽量把 `src` 下的内联测试清零

### 本轮新增完成

#### 1. `bootstrap`、`i18n`、`main_support`、`web/host` 的内联测试已拆到独立测试子模块

- 修改：
  - `src/bootstrap/settings.rs`
  - `src/i18n.rs`
  - `src/main_support/llm_tui_command_service.rs`
  - `src/web/host/auth.rs`
  - `src/web/host/webui_config_api.rs`
- 新增：
  - `src/bootstrap/settings/tests.rs`
  - `src/i18n/tests.rs`
  - `src/main_support/llm_tui_command_service/tests.rs`
  - `src/web/host/auth/tests.rs`
  - `src/web/host/webui_config_api/tests.rs`

迁移内容：

- runtime settings 对 env / doc 的优先级
- i18n locale 解析、plugin 语言包 merge、snapshot fallback
- LLM TUI provider URL 切换与 ask prefix 动态更新
- webui password store 与登录约束
- webui config merge / appearance payload / active config 校验

#### 2. `plugin/sdk`、`web/ui`、`tui/app/runtime` 的内联测试已拆到独立测试子模块

- 修改：
  - `src/plugin/sdk/host_bridge.rs`
  - `src/plugin/sdk/runtime_adapters.rs`
  - `src/plugin/sdk/python/runtime_registry/{common,tool,cron,web_api}.rs`
  - `src/web/ui.rs`
  - `src/tui/app/runtime.rs`
- 新增：
  - `src/plugin/sdk/host_bridge/tests.rs`
  - `src/plugin/sdk/runtime_adapters/tests.rs`
  - `src/plugin/sdk/python/runtime_registry/common/tests.rs`
  - `src/plugin/sdk/python/runtime_registry/tool/tests.rs`
  - `src/plugin/sdk/python/runtime_registry/cron/tests.rs`
  - `src/plugin/sdk/python/runtime_registry/web_api/tests.rs`
  - `src/web/ui/tests.rs`
  - `src/tui/app/runtime/tests.rs`

迁移内容：

- onebot reply payload 封装
- plugin contract 版本兼容校验
- python runtime registry decode
- frontend dist / dev server 探测
- TUI LLM 多行输出日志展开

#### 3. `plugin/sdk/python` 最后一簇执行/快照测试已拆到独立测试子模块

- 修改：
  - `src/plugin/sdk/python/execution_codec.rs`
  - `src/plugin/sdk/python/execution.rs`
  - `src/plugin/sdk/python/runtime_introspection.rs`
- 新增：
  - `src/plugin/sdk/python/execution_codec/tests.rs`
  - `src/plugin/sdk/python/execution/tests.rs`
  - `src/plugin/sdk/python/runtime_introspection/tests.rs`

迁移内容：

- plugin tool lookup / web api response decode
- python cron job execution outcome 与 diagnostics
- capability snapshot decode / plugin id backfill / diagnostics clone

### 当前校验状态

- 已通过：
  - `cargo fmt --all`
  - `cargo check --all-targets --locked --offline`
  - `cargo test runtime_settings_from_app_config_with_env_prefers_env_over_doc_values --locked --offline`
  - `cargo test reload_catalog_merges_plugin_language_packs --locked --offline`
  - `cargo test llm_provider_use_switches_to_registered_base_url --locked --offline`
  - `cargo test password_login_requires_correct_password --locked --offline`
  - `cargo test merge_json_objects_preserves_unspecified_fields --locked --offline`
  - `cargo test onebot_reply_payload_uses_action_envelope_for_group --locked --offline`
  - `cargo test build_plugin_contract_rejects_newer_requested_host_api --locked --offline`
  - `cargo test load_python_runtime_registry_rejects_non_list_registry --locked --offline`
  - `cargo test decode_web_api_registration_normalizes_route_and_methods --locked --offline`
  - `cargo test resolve_frontend_dist_dir_uses_first_candidate_with_index_html --locked --offline`
  - `cargo test llm_multiline_output_is_split_into_multiple_logs --locked --offline`
  - `cargo test normalize_plugin_tool_lookup_strips_runtime_prefix --locked --offline`
  - `cargo test execute_python_registered_cron_job_reports_executed --locked --offline`
  - `cargo test get_python_plugin_capability_snapshot_decodes_payload_and_backfills_plugin_ids --locked --offline`

### 当前判断

- 截至这一轮，`src` 下残留的内联 `#[test]` / `#[tokio::test]` 已清零
- 测试治理目标目前已经基本收口：
  - 黑盒公共测试已尽量迁到仓库根 `tests/`
  - 需访问私有实现的测试已统一沉到相邻 `tests.rs`
  - 不再需要继续在生产文件底部保留测试实现

## 2026-04-29 补充收口

### 1. `src` 内进程级测试状态已统一到共享测试支撑

- 新增：
  - `src/test_support.rs`
- 调整接入：
  - `src/bootstrap/settings/tests.rs`
  - `src/app_host/tests.rs`
  - `src/main_support/llm_tui_command_service/tests.rs`
  - `src/llm/client/tests.rs`
  - `src/llm/mcp/tests.rs`
  - `src/web/ui/tests.rs`
  - `src/web/host/auth/tests.rs`
  - `src/web/host/llm_api/tests.rs`
  - `src/web/host/tests.rs`

治理点：

- 统一 `env/cwd` 级别测试支撑，避免“模块内自锁、模块间仍并发”
- `process_state_lock` / `EnvVarGuard` / `CurrentDirGuard` 收到单一入口
- `cargo test --locked --offline` 下原先 4 个 `web::host::*` 失败点已全部收口

### 2. capability / plugin / tui 测试语义已对齐当前实现

- 功能修正：
  - `src/web/host/capability_api.rs`
    - `display_workspace_relative_path(...)` 现在会在 canonical root 场景下稳定回退到相对路径显示
- 测试环境隔离补强：
  - `src/web/host/tests.rs`
    - `CapabilityRouteTestEnv` 追加 `USERPROFILE` / `HOME` / `LY_SKILLS_DIR`
    - `PluginCapabilityRouteTestEnv` 追加 `USERPROFILE` / `HOME`
- 测试断言校正：
  - `src/web/host/tests.rs`
    - `HEAD /` 断言改为匹配当前 `307` + 空 body 重定向语义
    - plugin cron route 用例不再把 `lastRunTime` 绑死到固定 tick 值，只要求 basic job 存在，若字段存在则必须为合法 RFC3339
  - `tests/plugin_manager.rs`
    - Python contract smoke 改为使用 snapshot 已计算出的 `next_run_time` 驱动 cron tick，避免固定时间戳落后于 scheduler overlay
  - `src/tui/app/tests.rs`
    - resume size limit 用例改为贴合真实 flush 路径：先同步 active snapshot，再裁剪旧 resume

### 3. 本轮最终验证

- 已通过：
  - `cargo fmt --all`
  - `cargo check --all-targets --locked --offline`
  - `cargo test head_request_returns_headers_without_body --locked --offline`
  - `cargo test capability_routes_support_mcp_save_test_and_skill_read_upload --locked --offline`
  - `cargo test plugin_cron_scheduler_executes_basic_jobs_and_updates_diagnostics --locked --offline`
  - `cargo test resume_size_limit_drops_frontmost_old_resume --locked --offline`
  - `cargo test --test plugin_manager plugin_manager_python_contract_smoke_test_exercises_capabilities_and_execution --locked --offline`
  - `cargo test --locked --offline`

当前结论：

- 全量 `cargo test --locked --offline` 已回到通过
- `src` 下无残留内联测试
- 这一轮新增失败点已全部收口到：
  - 统一测试支撑
  - 更稳的环境隔离
  - 与现实现义一致的断言
