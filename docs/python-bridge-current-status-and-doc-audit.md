# Python Bridge Current Status And Doc Audit

## Purpose

This document records the current implementation state of the Python bridge / AstrBot compatibility lane and explains which `docs/` files should be treated as current truth versus historical planning material.

Date of audit: 2026-04-26

## Current Runtime Truth

The current Python bridge already supports:

- Python plugin load, start, shutdown, unload, and cleanup
- retained compatibility metadata for tools, web APIs, cron jobs, legacy tasks, and compat agents
- runtime tool execution through the host plugin API and the shared LLM tool bundle
- runtime web API execution through `/api/Plugin/Runtime/WebApi/{plugin_id}/{registered_path...}`
- a minimal Quart-compatible request/response shim for plugin web API handlers
- host-owned cron execution and cron state persistence
- runtime state and diagnostics query APIs

The important current limit is unchanged:

- legacy tasks are still visible metadata, not an executable backend surface

## Current Truth Table

| Surface | Metadata retained | Host query API | Runtime execution | Persistence / recovery |
| --- | --- | --- | --- | --- |
| Tools | Yes | Yes | Yes | No |
| Web APIs | Yes | Yes | Yes | No |
| Cron jobs | Yes | Yes | Yes | Yes |
| Legacy tasks | Yes | Yes | No | No |

## Doc Audit Result

### Current-state documents

Treat these as the current implementation-facing documents:

- `docs/plugin-runtime-current-state.md`
- `docs/tools-mcp-skills-web-api.md`
- `docs/frontend-backend-adaptation-requirements.md`
- `docs/python-bridge-compatibility-notes.md`
- `docs/python-bridge-risk-register.md`

### Historical plan / design documents

These are still useful, but they are not the authoritative current-state contract:

- `docs/python-bridge-refactor-plan.md`
- `docs/astrbot-tools-mcp-skills-action-plan.md`
- `docs/progressive-tool-disclosure-design.md`

## Remaining Gaps

The main Python bridge gaps still worth tracking are:

- no legacy task execution engine
- no full Quart server compatibility beyond the minimal runtime web API shim
- no full AstrBot provider / agent parity
- no durable capability snapshot for unloaded or disabled plugins

## Practical Reading Order

For current plugin bridge reality, prefer this order:

1. `docs/plugin-runtime-current-state.md`
2. `docs/python-bridge-current-status-and-doc-audit.md`
3. `docs/python-bridge-compatibility-notes.md`
4. `docs/python-bridge-risk-register.md`
5. `docs/tools-mcp-skills-web-api.md`

Use `docs/python-bridge-refactor-plan.md` only when you need the original bridge plan and phase history.
