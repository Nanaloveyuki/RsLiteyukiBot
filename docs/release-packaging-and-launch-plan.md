# Release Packaging And Launch Plan

## Purpose

This document records the planned path for turning the current development-only runtime into a releaseable multi-platform product.

Repository state verified against the current worktree on 2026-04-26.

The target is not only "make CI produce files". The target is a coherent launch model:

- terminal command `liteyuki run` starts a single runtime with TUI + Web enabled
- opening the desktop app starts Tauri 2 + Web
- Docker starts a deployable Web runtime by default
- `config.yaml` can choose the preferred launch mode when the caller does not force one
- GitHub Actions produces Windows, macOS, GNU/Linux, AppImage or portable binaries, and Docker deployable images

## Target User Experience

### CLI

The installed command should be:

```bash
liteyuki run
```

Default behavior:

- starts `tui-web`
- runs one bot/runtime instance
- attaches the local TUI to that runtime
- starts the shared WebHost on the configured port
- prints or displays the local Web URL in the TUI

Explicit overrides should be supported:

```bash
liteyuki run --mode tui
liteyuki run --mode web
liteyuki run --mode tui-web
liteyuki run --mode docker-web
```

### Desktop

Double-clicking the desktop application should start:

```text
tauri-web
```

The Tauri shell should start the shared WebHost, inject `window.__LITEYUKI_RUNTIME_API_BASE__`, and open the React frontend in the webview.

Desktop launch should not try to start a TUI. If the config asks for `tui` or `tui-web` while launched from the desktop app, the desktop runner should either:

- ignore only the TUI part and start `tauri-web`
- or show a clear frontend warning that this mode is not valid for desktop launch

The first option is preferred for normal users.

### Docker

Docker should default to:

```text
docker-web
```

The container should:

- expose the WebHost port
- expose adapter ports when configured
- store configs, plugins, logs, and runtime data in mounted volumes
- run as a non-root user unless a specific plugin/runtime feature requires elevated permissions

## Current Baseline

Already present:

- `RuntimeTarget` includes `Cli`, `Web`, `Tauri2`, `Docker`, `CliWeb`, and `DockerWeb`.
- `src/bin/web.rs` starts `EmbeddedWebRuntime` for a Web host.
- `src-tauri/src/lib.rs` starts `EmbeddedWebRuntime` for `RuntimeTarget::Tauri2`.
- Tauri injects `window.__LITEYUKI_RUNTIME_API_BASE__` and `window.__LITEYUKI_LOCAL_TOKEN__` into the frontend.
- WebHost can serve `frontend/dist` or redirect non-API routes to the Vite dev server through `LY_WEB_DEV_SERVER`.
- A Docker image workflow exists at `.github/workflows/build-image.yml`.
- A root `Dockerfile` and `docker-compose.yml` now exist for the Web-only container runtime.

Missing or incomplete:

- `liteyuki run` does not exist as a product command.
- The root binary name is still tied to the Rust crate shape, not the intended user-facing CLI name.
- `CliWeb` exists as a target enum but is not implemented as a shared TUI + Web launcher.
- `config.yaml` does not currently own a launch-mode field.
- Tauri bundling is not enabled because `src-tauri/tauri.conf.json` has `"bundle": { "active": false }`.
- Release workflows for Windows, macOS, GNU/Linux, AppImage, and portable CLI archives are not present yet.

## Launch Mode Contract

Add a config section like this:

```yaml
core:
  launch:
    mode: tui-web
    web:
      host: 0.0.0.0
      port: 14500
```

Supported modes:

- `tui`: terminal-only TUI
- `web`: WebHost only
- `tui-web`: terminal TUI plus WebHost in one runtime
- `tauri-web`: desktop shell plus WebHost in one runtime
- `docker-web`: container-friendly WebHost runtime

Resolution priority:

1. CLI argument, for example `liteyuki run --mode tui-web`
2. Environment variable, for example `LY_RUNTIME_TARGET=tui-web`
3. `config.yaml` launch mode
4. binary default

Recommended defaults:

- CLI binary default: `tui-web`
- Tauri desktop default: `tauri-web`
- Docker image default: `docker-web`
- development `cargo run` can keep `tui` until the new launcher is ready, but release builds should expose `liteyuki run`

## Implementation Plan

### Phase 1: Normalize Launch Configuration

Work items:

- add `core.launch.mode` to the config schema
- add default config template entries for YAML and TOML
- implement parsing and validation for launch modes
- keep `LY_RUNTIME_TARGET` as an override for development and CI
- add regression tests for config priority and invalid modes

Main code areas:

- `src/app_config.rs`
- `src/config_edit.rs`
- `src/runtime_support.rs`
- `src/core/platform.rs`
- `tests/config_management.rs`
- `tests/app_config_migrated.rs`

### Phase 2: Add Product CLI

Work items:

- rename or add a release binary named `liteyuki`
- add a small command parser for `liteyuki run`
- support `--mode`, `--config`, and later `--web-port`
- preserve current `cargo run` behavior during transition if needed
- document installed-command behavior in `README.md`

Main code areas:

- `Cargo.toml`
- `src/main.rs`
- possible new module `src/cli.rs`
- possible new binary `src/bin/liteyuki.rs`

### Phase 3: Implement Single-Runtime `tui-web`

Work items:

- factor the current TUI startup so it can attach to an already-created runtime host
- start WebHost and TUI against the same app runtime
- avoid running two independent `LiteyukiBot` instances
- show WebHost bind URL and external URL hint inside the TUI
- ensure shutdown closes TUI, WebHost, plugins, adapters, and background tasks in a deterministic order

Main code areas:

- `src/main.rs`
- `src/app_host.rs`
- `src/web/runtime.rs`
- `src/web/host/mod.rs`
- `src/tui/app/runtime.rs`
- `src/tui/app/render.rs`

Key design constraint:

`tui-web` must not be implemented by spawning `cargo run` and `cargo run --bin web` together. That would create duplicate runtime state, duplicate plugin load, duplicate adapter ownership, and confusing port/config behavior.

### Phase 4: Stabilize Tauri 2 Packaging

Work items:

- enable Tauri bundling
- define package identifiers and names for release
- decide installer formats per platform
- ensure `frontend/dist` is always built before Tauri packaging
- keep the runtime API base injection path tested
- verify tray behavior and shutdown on Windows, macOS, and GNU/Linux

Main code areas:

- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`
- `src-tauri/src/lib.rs`
- `package.json`
- `scripts/tauri-runner.mjs`

Likely platform outputs:

- Windows: `.msi` or NSIS installer, plus optional portable archive
- macOS: `.app` and `.dmg`
- GNU/Linux: AppImage, plus optional `.deb` / `.rpm` later

### Phase 5: Add Portable CLI Artifacts

Work items:

- build CLI binaries for Windows, macOS, and GNU/Linux
- package each binary with frontend assets if Web mode is expected to work offline
- include license files and minimal default config examples
- decide whether Python bridge assets are bundled or documented as external requirements

Portable archive examples:

- `liteyuki-windows-x86_64.zip`
- `liteyuki-macos-aarch64.tar.gz`
- `liteyuki-macos-x86_64.tar.gz`
- `liteyuki-linux-x86_64.tar.gz`

### Phase 6: Add Docker Runtime

Work items:

- build frontend assets in the image or copy prebuilt assets from CI
- compile the Rust runtime in a builder stage
- run the final image with a minimal runtime layer
- set default runtime target to `docker-web`
- expose WebHost and adapter ports
- define persistent volume paths for configs, plugins, logs, and runtime data
- add a `docker-compose.yml` example after the base image is stable

Main files:

- `Dockerfile`
- `.dockerignore`
- `.github/workflows/build-image.yml`
- optional `docker-compose.yml`
- README deployment section

### Phase 7: Release CI Matrix

Work items:

- add a release workflow separate from the Docker workflow
- build on native OS runners because Tauri desktop packaging is platform-sensitive
- cache Rust, pnpm, and Tauri dependencies
- upload artifacts for every platform
- optionally publish GitHub Releases on tags

Suggested jobs:

- `check`: formatting, lint, root Rust tests, frontend build
- `cli`: portable CLI artifacts for Windows, macOS, GNU/Linux
- `desktop`: Tauri installer/AppImage artifacts on native runners
- `docker`: multi-arch image build and push

## Expected Obstacles

### Runtime Ownership

The hardest engineering problem is shared runtime ownership.

The Web runtime already uses `EmbeddedAppHost`; the TUI path currently constructs and owns `LiteyukiBot` directly. To make `tui-web` reliable, the TUI must either:

- attach to `EmbeddedAppHost`
- or share a lower-level runtime object factored out of both launchers

The first option is likely faster. The second option may be cleaner long-term but is a larger refactor.

### TUI And Web Event Surfaces

TUI and Web both need logs, runtime status, plugin state, adapter state, and LLM capability state.

The Web side already has HTTP/SSE/WebSocket-facing APIs. The TUI side currently receives UI events directly. A shared runtime should avoid duplicating observers and should define which event stream is authoritative.

### Frontend Asset Packaging

WebHost currently detects `frontend/dist`. That is convenient in development, but release artifacts need a stable answer:

- embed frontend assets into the binary
- or ship `frontend/dist` next to the executable
- or use Tauri's packaged frontend for desktop only and require external assets for CLI Web

Embedding is better for portable CLI UX. Shipping a directory is simpler and may be acceptable for early releases.

### Python Bridge Distribution

The project uses `pyo3` and Python plugin compatibility code.

Release packaging must decide whether Python is:

- required to be installed by the user
- bundled with the app
- only supported in Docker and developer environments initially

Bundling Python across Windows, macOS, and Linux can become one of the most expensive packaging tasks.

### Tauri Platform Requirements

Tauri packaging depends on native platform tooling:

- Windows needs WebView2 assumptions, installer tooling, and eventually code signing.
- macOS needs signing and notarization for a smooth user experience.
- GNU/Linux AppImage needs system library compatibility, especially WebKitGTK-related dependencies.

Unsigned builds can still be produced, but they will have warning prompts and lower trust.

### Docker Runtime Shape

Docker should not default to a TUI mode. It should be Web-first and volume-driven.

The image must also avoid writing runtime state into the immutable application directory. Config, password state, plugins, logs, and generated runtime files should live under a mounted data root.

### CI Time And Cache Stability

The release pipeline will build Rust, frontend assets, Tauri desktop packages, and Docker images. Without careful cache keys and split jobs, CI time and failure rate will increase quickly.

## Acceptance Criteria

The release plan should be considered implemented only when all of these are true:

- `liteyuki run` starts `tui-web` by default from an installed CLI artifact.
- `liteyuki run --mode web` starts WebHost without TUI.
- `liteyuki run --mode tui` preserves terminal-only behavior.
- Desktop launch starts Tauri 2 + Web without requiring terminal commands.
- Docker starts `docker-web` by default and can persist state through mounted volumes.
- `config.yaml` can select launch mode when CLI/env does not override it.
- WebHost uses the same runtime state as TUI or Tauri in combined modes.
- GitHub Actions uploads platform-specific artifacts for Windows, macOS, and GNU/Linux.
- GitHub Actions produces an AppImage for GNU/Linux.
- GitHub Actions builds and pushes a Docker image.
- Release artifacts include frontend assets or otherwise fail fast with a clear diagnostic.

## Recommended Near-Term Order

1. Implement config-level launch mode parsing.
2. Add `liteyuki run --mode ...` as a real CLI contract.
3. Implement `tui-web` on top of one shared runtime.
4. Enable Tauri bundle output and test native packaging locally.
5. Add root Dockerfile and make Docker default to `docker-web`.
6. Add CI release artifacts after the local commands are stable.
