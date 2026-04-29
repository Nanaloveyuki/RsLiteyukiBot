<div align="center">

[//]: # (<img  src="https://cdn.liteyuki.org/logos/bot.svg" style="align-content: center; width: 50%; margin-top:10%;" alt="a">)
![][banner]
<h2> <span style="color: #a2d8f4">轻雪</span> <span style="color: #d0e9ff">6 In Rust</span></h2>
<h4> <span style="color: #a2d8f4">✨ 轻量, 高效, 易于扩展 ✨</span></h4>

![][Liteyuki6.0]

</div>

## 关于

本项目是由 [LiteyukiBot-6](https://github.com/LiteyukiStudio/LiteyukiBot) 延申而来的 Rust 重构项目, 在保留原有轻量化特点的同时, 又迎合 AI 时代浪潮.

## 特点

- Rust 开发: 高效运行
- 多线程处理: 避免*一核有难, 七核围观*
- Websocket & SSE & TCP-Http 三段兼容: 总有一个接口适合你
- TUI & Docker & Tauri2-GUI: 想要什么风格都可以

## Todo

- [ ] AstrBot 插件完整支持
- [ ] ADK-Rust 兼容层
- [ ] Neo-Mofox 插件完整支持
- [ ] NoneBot Runtime 兼容层
- [ ] Liteyuki Agent Flow 兼容层

## 快速开始

### 环境要求

- Rust 工具链 (需支持 `edition = "2024"`)
- Python 3.11+
- 建议使用最新稳定版 `cargo`

### 启动

TUI:
```bash
cargo run
```

首次启动会检测 `config.yaml` 并自动加载, 如果没有会自动在 `%User%/.liteyuki/*` 下创建.

## 配置说明

### 主配置文件

配置加载顺序如下:

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

- `connect.websocket`: 支持 `forward` / `reverse` / `both`
- `connect.tcp-http`: TCP-HTTP 通道
- `connect.sse`: SSE 通道
- `urls` 支持多端点, 会自动展开为多个适配器实例

## 开发测试

- 前端 Dev Server: `http://127.0.0.1:1420/`
- Tauri 内嵌 runtime / 健康接口: `http://127.0.0.1:14500/api/health`
- Vite 开发服务器默认绑定 `0.0.0.0:1420`, 便于外部浏览器访问
- Tauri 后端仍会保留 `0.0.0.0:14500` 的 HTTP 入口, 用于健康检查和后续扩展
- Docker 测试端口参考自行设定的 `-p` 参数

开发启动:

Tauri App:
```bash
pnpm install
pnpm run cargo dev
```

如果本地 shell 对 `pnpm run cargo dev` 的参数转发有差异, 可退回:

```bash
pnpm run cargo:dev
```

前端单独构建:

```bash
pnpm build
```

Rust/Tauri 后端级别检查:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

TUI (Terminal User Interface):
```bash
cargo run
```

## Docker 部署

当前仓库提供的是仅 `Web` + `后端` 形式的 image.

### 1. 构建镜像

```bash
docker build -t liteyukibot-web .
```
~~你可能需要花上五六分钟来编译~~

如果当前环境无法直连 `docker.io`, 可以直接覆写基础镜像来源, 而不用修改 `Dockerfile`:

```powershell
docker build `
  --build-arg NODE_BASE_IMAGE=docker.m.daocloud.io/library/node:22-bookworm-slim `
  --build-arg RUST_BASE_IMAGE=docker.m.daocloud.io/library/rust:1-bookworm `
  --build-arg RUNTIME_BASE_IMAGE=docker.m.daocloud.io/library/debian:bookworm-slim `
  -t liteyukibot-web .
```

### 2. 启动容器

Linux / macOS:

```bash
docker run -d \
  --name liteyukibot-web \
  -p 14500:14500 \
  -v "$(pwd)/docker-data:/data" \
  --restart unless-stopped \
  liteyukibot-web
```

PowerShell:

```powershell
docker run -d `
  --name liteyukibot-web `
  -p 14500:14500 `
  -v "${PWD}/docker-data:/data" `
  --restart unless-stopped `
  liteyukibot-web
```

启动后可通过以下地址访问:

- WebUI: `http://127.0.0.1:14500/`
- 健康检查: `http://127.0.0.1:14500/api/health`

### 3. 使用 docker compose

仓库已附带 [`docker-compose.yml`](./docker-compose.yml), 可直接启动:

```bash
docker compose up -d --build
```

停止:

```bash
docker compose down
```

### 4. 数据目录说明

容器内所有运行时数据默认写入挂载卷 `/data/.liteyuki/`.

常用路径:

- 主配置: `/data/.liteyuki/configs/config.yaml`
- LLM 配置: `/data/.liteyuki/configs/llm-config.yaml`
- WebUI 密码: `/data/.liteyuki/configs/password.json`
- 插件目录: `/data/.liteyuki/plugins/`

如果你已经有现成配置, 可在启动前放到宿主机挂载目录中:

- `./docker-data/.liteyuki/configs/config.yaml`
- `./docker-data/.liteyuki/configs/llm-config.yaml`
- `./docker-data/.liteyuki/plugins/`

### 5. 容器默认行为

- 只启动共享 WebHost
- 默认运行目标为 `docker-web`
- 默认对外暴露 `14500` 端口
- 以非 root 用户运行
- 镜像内保留 Python 运行时, 便于当前 Python bridge / AstrBot 兼容层继续工作

### 6. 常用维护命令

查看日志:

```bash
docker logs -f liteyukibot-web
```

重启容器:

```bash
docker restart liteyukibot-web
```

删除容器:

```bash
docker rm -f liteyukibot-web
```

如果你的 OneBot / WebSocket / SSE 适配器需要额外对外端口, 请按实际配置追加 `-p`, 或在 `docker-compose.yml` 中补充 `ports` 映射.

## Image 发布

项目 Docker image 只发布到 GHCR, 不再推送 Docker Hub.

- Registry: `ghcr.io`
- Image: `ghcr.io/nanaloveyuki/rsliteyukibot-web`
- Workflow: [`.github/workflows/build-image.yml`](./.github/workflows/build-image.yml)

### 自动发布规则

- push 到 `main` 时, GitHub Actions 会构建并推送 `latest` 与 `sha-*` tag.
- push `v*` tag 时, GitHub Actions 会额外推送对应版本 tag, 例如 `v0.1.0`.
- 整个构建与 push 过程都在 GitHub Actions 上完成, 不依赖本地 Docker 网络环境.

### 拉取示例

拉取最新镜像:

```bash
docker pull ghcr.io/nanaloveyuki/rsliteyukibot-web:latest
```

拉取指定版本:

```bash
docker pull ghcr.io/nanaloveyuki/rsliteyukibot-web:v0.1.0
```

### 发布新版本

推荐使用仓库内脚本自动创建并推送 tag:

Linux / macOS:

```bash
./scripts/publish_new_version.sh 0.1.0
```

PowerShell / CMD:

```powershell
scripts\publish_new_version.bat 0.1.0
```

脚本会执行以下流程:

1. 规范化版本号为 `vX.Y.Z`.
2. 检查当前工作区是否干净.
3. 检查本地和远端是否已存在同名 tag.
4. 创建 annotated tag.
5. 推送 tag 到 `origin`, 触发 GHCR 自动构建与发布.

## 鸣谢

[@Snowykami](https://sfkm.me): Liteyuki 文档站支持和授权

[@NapcatQQ](https://github.com/NapNeko/NapCatQQ): 前端页面样式

## 版权&许可

<details><summary>Napcat LICENSE</summary>
<code>
Limited Redistribution License for NapCat

Copyright © 2024 Mlikiowa

1. Usage and Reproduction:
   - Unauthorized use, reproduction, modification, or distribution of this code is prohibited without explicit permission from the main author of the NapCat repository.
   
2. Redistribution:
   - Redistribution of this code is permitted, provided that the full text of this license is included, and the source and copyright information is clearly stated.
   - Minor modifications and extensions are allowed for redistribution purposes, but the modified code must not be publicly released.

3. Non-Commercial Use:
   - This code is not to be used for any commercial purposes.

4. Additional Permissions:
   - Any rights not explicitly addressed in this license must be requested from and granted by the main author of the NapCat repository.

5. Disclaimer:
   - This code is provided "as is," without any express or implied warranties, including but not limited to the implied warranties of merchantability and fitness for a particular purpose. In no event shall the author be liable for any damages or other liability arising from, out of, or in connection with the use or distribution of this code.
</code>
</details>

本项目采用混合所有制许可:
- 前端部分:
  - 部分代码 (使用 NapcatUI 源码部分) 采用 `Napcat LICENSE`
  - 部分代码 (二次创作后部分) 采用 [`LSO-Common.zh-CN v1.4 Modified`](./LICENSE)
- 后端部分:
  - 采用 [`LSO-Common.zh-CN v1.4 Modified`](./LICENSE)
- 插件兼容层实现:
  - 按照各插件本体平台协议进行二次许可

未涉及或未明晰内容, 如字体等版权由提供方许给的许可进行授权, 受制于篇幅不再详细描述.

## 友情项目

- [NapcatQQ](https://github.com/NapNeko/NapCatQQ): ~~一只猫~~非常完善的 Onebot v11 实现端
- [Astrbot](https://github.com/AstrBotDevs/AstrBot): 本项目的参考对象, 非常感谢 Astrbot 提供的 LLM 思路
- [Neo-Mofox](https://github.com/MoFox-Studio/Neo-MoFox): 插件兼容层的实现对象之一, 一个新兴的 Bot 框架


[Liteyuki6.0]: https://img.shields.io/badge/Liteyuki-6.0-blue?style=for-the-badge

[banner]: https://socialify.git.ci/Nanaloveyuki/RsLiteyukiBot/image?description=1&font=Source+Code+Pro&forks=1&issues=1&logo=https%3A%2F%2Fcdn.liteyuki.org%2Flogos%2Fbot.svg&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto
