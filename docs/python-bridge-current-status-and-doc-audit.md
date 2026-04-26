# Python Bridge Current Status And Doc Audit

## Purpose

This document records the current implementation state of the Python bridge / AstrBot compatibility lane after the latest runtime web API adaptation pass, and audits whether the existing `docs/` content still matches the real project state.

Date of audit: 2026-04-26

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

Cron jobs now have a real host-owned scheduler backend.
Legacy tasks remain registration-only surfaces.

## Current Truth Table

| Surface | Metadata retained | Host query API | Runtime execution | Persistence / recovery |
| --- | --- | --- | --- | --- |
| Tools | Yes | Yes | Yes | No |
| Web APIs | Yes | Yes | Yes | No |
| Cron jobs | Yes | Yes | Yes | Yes |
| Legacy tasks | Yes | Yes | No | No |

## Regression Coverage Added / Revalidated

Verified in this pass:

- AstrBot context metadata tests still pass
- command-group alias propagation tests still pass
- runtime web API dispatch route tests pass
- Quart-compatible request/response usage passes through the host runtime bridge
- plugin cron scheduler route/diagnostics integration passes

Commands run:

- `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline`
- `cargo test --test plugin_manager`
- `cargo test plugin_runtime_web_api_routes_dispatch_registered_handlers --lib`
- `cargo test plugin_cron_scheduler_executes_basic_jobs_and_updates_diagnostics -- --nocapture`

## Docs Audit

### Documents confirmed consistent with current implementation

- `docs/python-bridge-compatibility-notes.md`
  - updated in this audit to distinguish shipped cron scheduling from still-missing legacy task execution
- `docs/python-bridge-risk-register.md`
  - updated in this audit so the metadata-only warning now applies to legacy tasks rather than cron
- `docs/tools-mcp-skills-frontend-api-draft.md`
  - still matches the current tools / MCP / skills backend surfaces

### Documents updated during this audit

- `docs/plugin-backend-api-requirements.md`
  - updated the implementation snapshot and state matrix to reflect the landed cron scheduler backend
- `docs/python-bridge-compatibility-notes.md`
  - removed outdated statements that still described cron execution as missing
- `docs/python-bridge-risk-register.md`
  - narrowed the metadata-only warning to legacy tasks and kept cron execution state truthful
- `docs/frontend-backend-adaptation-requirements.md`
  - still contains pre-cron-scheduler wording in the current worktree and should be updated in the frontend adaptation lane rather than this backend-only pass
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
