# Python Bridge Current Status And Doc Audit

## Purpose

This document records the current implementation state of the Python bridge / AstrBot compatibility lane after the latest runtime web API adaptation pass, and audits whether the existing `docs/` content still matches the real project state.

Date of audit: 2026-04-25

## What Was Finished In This Pass

### Runtime web API compatibility

The Python bridge now supports a minimal AstrBot-style Quart-compatible web API execution path on top of the host runtime route bridge.

Implemented pieces:

- registered AstrBot web APIs can be executed through:
  - `/api/Plugin/Runtime/WebApi/{plugin_id}/{registered_path...}`
- Python compat runtime now exposes a minimal `quart` shim for plugin web API handlers:
  - `quart.request`
  - `quart.jsonify`
  - `quart.make_response`
- request context is bound inside the actual awaited Python execution path, so it works correctly with the dedicated shared async runtime thread
- request inspection now supports the common compatibility subset:
  - `request.method`
  - `request.path`
  - `request.args.get(..., type=...)`
  - `request.headers.get(...)`
  - `await request.get_json()`
  - `await request.get_data(...)`
  - `request.remote_addr`

### Runtime diagnostics / execution consistency

This pass continued to align the Python bridge with the backend APIs that already exist in the host:

- capability snapshots
- runtime web API dispatch
- tool execution bridge
- runtime state query
- diagnostics query

Cron jobs and legacy tasks remain registration-only surfaces.

## Current Truth Table

| Surface | Metadata retained | Host query API | Runtime execution | Persistence / recovery |
| --- | --- | --- | --- | --- |
| Tools | Yes | Yes | Yes | No |
| Web APIs | Yes | Yes | Yes | No |
| Cron jobs | Yes | Yes | No | No |
| Legacy tasks | Yes | Yes | No | No |

## Regression Coverage Added / Revalidated

Verified in this pass:

- AstrBot context metadata tests still pass
- command-group alias propagation tests still pass
- runtime web API dispatch route tests pass
- Quart-compatible request/response usage passes through the host runtime bridge

Commands run:

- `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline`
- `cargo test --test plugin_manager`
- `cargo test plugin_runtime_web_api_routes_dispatch_registered_handlers --lib`

## Docs Audit

### Documents confirmed consistent with current implementation

- `docs/python-bridge-compatibility-notes.md`
  - matches the current bridge reality after the latest updates
  - correctly says tools and web APIs now have host bridges
  - correctly says cron/task execution is still intentionally missing
- `docs/python-bridge-risk-register.md`
  - still matches the real shared-interpreter risk profile
  - correctly distinguishes executable tool/web-api support from registration-only cron/task support
- `docs/frontend-backend-adaptation-requirements.md`
  - matches the current backend capability surface used by frontend integration work
  - correctly treats tools and web APIs as executable while cron/task remain scheduler-pending
- `docs/tools-mcp-skills-frontend-api-draft.md`
  - still matches the current tools / MCP / skills backend surfaces

### Documents updated during this audit

- `docs/plugin-backend-api-requirements.md`
  - fixed an outdated sentence in the Phase 1 rationale that still said Rust could not inspect or serve capability registrations
  - current truth is that visibility/query/execution bridges exist for tools and web APIs, while scheduler support is the remaining gap
- `docs/python-bridge-refactor-plan.md`
  - added a status note so readers do not mistake it for the current implementation state
  - clarified that `sdk.rs` has already been split into `src/plugin/sdk/`

### Documents that are intentionally plan / design artifacts

These are still useful and not "wrong", but they should be read as planning material rather than authoritative current-state specs:

- `docs/python-bridge-refactor-plan.md`
- `docs/astrbot-tools-mcp-skills-action-plan.md`
- `docs/progressive-tool-disclosure-design.md`

## Current Remaining Gaps

The main plugin/backend gaps still remaining after this pass are:

- no real cron scheduler backend
- no legacy task execution engine
- no full Quart server compatibility; only the minimal shim needed for plugin runtime web API handlers
- no full AstrBot provider / agent parity

## Practical Reading Order

For current plugin bridge reality, prefer reading in this order:

1. `docs/python-bridge-current-status-and-doc-audit.md`
2. `docs/python-bridge-compatibility-notes.md`
3. `docs/python-bridge-risk-register.md`
4. `docs/plugin-backend-api-requirements.md`

Use `docs/python-bridge-refactor-plan.md` only when you want the original refactor phases and historical implementation direction.
