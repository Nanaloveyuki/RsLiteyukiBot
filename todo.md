# RsLiteyukiBot 核心框架路线图（dev/core）

## 目标
- 把当前 Rust 核心从“可运行”推进到“可扩展、可交付、可维护”。
- 优先补齐文档已声明但尚未完整落地的能力：插件系统、Docker/Tauri2 目标、热重载语义、LLM provider 抽象。

## 里程碑总览
- M1（P0）：插件系统 V1 可执行化（Native Runtime）
- M2（P0）：运行目标落地（Docker / Tauri2）
- M3（P1）：配置热重载契约与重启策略
- M4（P1）：LLM Provider 抽象层
- M5（P1）：文档与工程化收口

---

## M1（P0）插件系统 V1 可执行化
### 任务
- [ ] 明确 `plugin.json` 最小字段和校验规则（id、version、runtime、entrypoint、abi）
- [ ] 实现 Native runtime 插件执行链路（不仅注册，还可真实处理事件）
- [ ] 增加插件生命周期钩子（load/start/stop/unload）和错误码
- [ ] 增加 ABI/Host API 版本协商与不兼容提示
- [ ] 补齐插件 E2E 测试（发现、加载、调用、失败回退）

### 验收标准（DoD）
- [ ] 至少 1 个示例 Native 插件可被发现并处理事件
- [ ] 插件加载失败时可定位到 manifest 字段级错误
- [ ] `cargo test` 中包含插件端到端成功/失败路径

---

## M2（P0）运行目标落地（Docker / Tauri2）
### 任务
- [ ] 明确 `LY_RUNTIME_TARGET` 各模式行为矩阵（Cli/Web/Tauri2/Docker/CliWeb/DockerWeb）
- [ ] 补齐 Docker 运行路径（镜像构建、启动参数、配置挂载）
- [ ] 补齐 Tauri2 启动路径（能力开关、资源路径、日志路径）
- [ ] 建立每个 target 的 smoke test（最小启动 + 退出）

### 验收标准（DoD）
- [ ] `docker` 与 `tauri2` 目标都可最小启动
- [ ] README 中的 target 说明与实际行为一致
- [ ] CI 至少覆盖 1 条 Docker smoke 流程

---

## M3（P1）配置热重载契约与重启策略
### 任务
- [ ] 定义字段级热重载等级：`hot` / `soft-restart` / `cold-restart`
- [ ] `/reload` 输出变更摘要与不可热生效字段提示
- [ ] 对 `soft-restart` 字段实现受控重启（可回滚）
- [ ] 增加 reload 行为测试（配置变更前后对比）

### 验收标准（DoD）
- [ ] 用户可明确知道“哪些配置已生效、哪些需重启”
- [ ] 热重载失败不会破坏运行态

---

## M4（P1）LLM Provider 抽象层
### 任务
- [ ] 抽象 Provider trait（构建请求、鉴权、错误映射、健康检查）
- [ ] 将当前 OpenAI 兼容实现迁移到 Provider 框架
- [ ] 支持 provider 注册与配置校验（包含 `provider_urls`/`base_url` 一致性）
- [ ] 完善 provider 失败回退和诊断日志

### 验收标准（DoD）
- [ ] 主流程不再写死单一 provider 判断
- [ ] provider 配置错误时给出可操作提示

---

## M5（P1）文档与工程化收口
### 任务
- [ ] 更新 `README.md` Todo 状态与运行说明
- [ ] 扩展 `RUST_CORE.md`（插件 ABI、target 行为矩阵、reload 语义）
- [ ] 增加 `docs/` 下开发者指南（插件开发、部署、排障）
- [ ] 整理 CI：只保留 Rust 主链路必要工作流

### 验收标准（DoD）
- [ ] 新贡献者可按文档完成本地启动、测试、最小插件开发
- [ ] 文档描述与代码行为一致，无明显过时项

---

## 推荐执行顺序（两周冲刺视角）
- Week 1：M1（插件执行链路 + ABI 协商 + 基础测试）
- Week 2：M2（Docker/Tauri2 最小落地 + smoke test）
- Week 3：M3（reload 契约）+ M4（provider 抽象骨架）
- Week 4：M4 收尾 + M5 文档工程化收口

## 当前下一步（马上可做）
- [ ] 先起草插件系统 V1 的 manifest/ABI 规范草案（M1）
- [ ] 基于现有 `src/plugin/*` 补第一个可执行 Native 示例插件（M1）
- [ ] 同步新增插件 E2E 测试模板，避免后续回归（M1）
