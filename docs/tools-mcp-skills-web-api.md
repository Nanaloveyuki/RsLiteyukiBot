# Tools / MCP / Skills Web API

## Purpose

This document is the current-state replacement for the older frontend API draft.

Use it for the real backend routes and runtime behavior that already exist in the repository today.

Repository state verified against the current worktree on 2026-04-26.

## Runtime Baseline

These capability lanes are already wired into the shared backend runtime:

- local runtime tools
- repo-local skill scanning and inventory prompt injection
- Streamable HTTP MCP loading and tool adaptation
- shared LLM tool bundle assembly for TUI and Web chat paths

The main runtime anchors are:

- `src/llm/tools.rs`
- `src/llm/skills.rs`
- `src/llm/mcp.rs`
- `src/llm/service.rs`
- `src/web/host/capability_api.rs`

## Implemented Web Routes

The Web Host already exposes these routes:

- `GET /api/tools`
- `POST /api/tools/toggle`
- `GET /api/mcp/servers`
- `POST /api/mcp/save`
- `POST /api/mcp/test`
- `GET /api/skills`
- `GET /api/skills/read?name=<skill_name>`
- `POST /api/skills/upload`

All of them return the same NapCat-style envelope used by the rest of the Web API layer.

## Route Summary

### Tools

`GET /api/tools`

- returns the current runtime tool inventory
- includes local tools and MCP-backed tools in one list
- returns runtime warnings together with the inventory

`POST /api/tools/toggle`

- persists tool enabled state to `tool-state.json`
- returns the updated tool inventory after the toggle

Current hard rule:

- `list_tool_categories`
- `list_tools_in_category`
- `get_tool_schema`

These three discovery helpers are required runtime helpers and cannot be disabled.

### MCP

`GET /api/mcp/servers`

- returns the current MCP server config path
- returns per-server tool inventory and warnings

`POST /api/mcp/save`

- replaces the current MCP config file
- re-inspects configured servers after the write

`POST /api/mcp/test`

- tests the supplied server config through a temporary config file
- does not require persisting the submitted config first

Current backend support is for HTTP-style MCP transport only.
Unsupported transport should surface as warnings rather than disappearing silently.

### Skills

`GET /api/skills`

- lists repo-local skills discovered under the runtime skill root

`GET /api/skills/read`

- reads one discovered skill document for frontend inspection

`POST /api/skills/upload`

- uploads a skill package into the managed skill area

## What This Means For The Frontend

The frontend no longer needs to treat this lane as read-only.

The first useful UI can already include:

- tool inventory and enable/disable
- MCP inventory, save, and test
- skill inventory, read, and upload

The remaining missing pieces are deeper lifecycle management, not the basic dashboard round-trip.

## Remaining Gaps

Still not present:

- tool permission and policy layering beyond simple active/inactive state
- MCP child-process hosting and supervision UI
- richer MCP lifecycle history
- skill update/delete management
- audit history for capability state changes

## Recommended Reading Order

For this lane, read in this order:

1. `docs/tools-mcp-skills-web-api.md`
2. `docs/frontend-backend-adaptation-requirements.md`
3. `docs/astrbot-tools-mcp-skills-action-plan.md`

Use `docs/astrbot-tools-mcp-skills-action-plan.md` as a historical/planning artifact, not as the current API contract.
