# Frontend Backend Adaptation Requirements

## Purpose

This document is the frontend-facing follow-up plan based on the backend surfaces that already exist today.

It should answer two questions:

- which capabilities are already ready for frontend consumption now
- which remaining gaps still require backend follow-up before a fuller frontend pass

Repository state verified against the current worktree on 2026-04-26.

## Confirmed Backend Baseline

### 1. LLM / Prompt / Provider

The Web Host already exposes:

- `GET /api/LLM/GetSettings`
- `POST /api/LLM/Chat`
- `GET /api/LLM/GetManagerState`
- `POST /api/LLM/SaveManagerState`
- `POST /api/LLM/FetchModels`
- `POST /api/LLM/TestModels`
- `POST /api/LLM/PreviewRequest`
- `GET /api/LLM/PromptProfiles`
- `POST /api/LLM/PromptProfiles/Save`
- `POST /api/LLM/PromptProfiles/Delete`
- `POST /api/LLM/PromptProfiles/Use`
- `POST /api/LLM/PromptProfiles/Preview`

Current conclusion:

- prompt profile management is no longer blocked by missing backend APIs
- the next step is a real frontend page, not another backend placeholder

### 2. Plugin management and runtime capability

The backend already supports:

- `GET /api/Plugin/List`
- `POST /api/Plugin/SetStatus`
- `POST /api/Plugin/Import`
- `GET /api/Plugin/Store/List`
- `GET /api/Plugin/Store/Detail/{id}`
- `GET /api/Plugin/Config?id=<plugin_id>`
- `POST /api/Plugin/Config`
- `/plugin/{plugin_id}/page/{page_path}`

The runtime capability surfaces already exposed to frontend consumers are:

- `GET /api/Plugin/Capabilities`
- `GET /api/Plugin/Capabilities/All`
- `GET /api/Plugin/Tools`
- `GET /api/Plugin/WebApis`
- `GET /api/Plugin/CronJobs`
- `GET /api/Plugin/Tasks`
- `GET /api/Plugin/RuntimeState`
- `GET /api/Plugin/Diagnostics`
- `POST /api/Plugin/Tools/Execute`
- `/api/Plugin/Runtime/WebApi/{plugin_id}/{registered_path...}`

`/api/Plugin/List` already includes runtime-facing classification fields such as:

- `runtimeKind`
- `pluginType`
- `sourceKind`
- `compatKind`
- `sourceFamily`
- `adapterFamily`
- `compatLevel`
- `status`
- `hasConfig`
- `hasPages`
- `hasCapabilities`

Current conclusion:

- plugin classification is no longer blocked on basic list fields
- plugin detail, capability, and diagnostics views can be built now
- the remaining backend gap here is not basic visibility, but the deeper runtime edges listed later in this file

### 3. Tools / MCP / Skills

The Web Host already exposes:

- `GET /api/tools`
- `POST /api/tools/toggle`
- `GET /api/mcp/servers`
- `POST /api/mcp/save`
- `POST /api/mcp/test`
- `GET /api/skills`
- `GET /api/skills/read?name=<skill_name>`
- `POST /api/skills/upload`

Current conclusion:

- this lane is no longer read-only
- a first frontend pass can already support inventory plus basic management actions

## Main Frontend Work Remaining

### 1. Move AI pages out of `debug`

The current frontend still hides real product capability under debug-oriented routes.

The next information architecture should treat these as first-class pages:

- `AI / Chat`
- `AI / Providers & Models`
- `AI / Prompts`
- `AI / Capabilities`

### 2. Build a real prompt management page

The backend APIs now exist.
The frontend should build a proper prompt workflow instead of showing only the current profile name.

The minimum useful page should support:

- prompt profile list
- active profile switch
- create
- edit
- delete
- preview
- save and reload after mutation

### 3. Upgrade the plugin page into a real plugin center

The plugin area should no longer stop at:

- list
- enable/disable
- config modal
- extension iframe

The next frontend pass should add:

- installed plugin list with badges and filters
- plugin detail view
- config tab
- pages tab
- capabilities tab
- runtime state / diagnostics tab

Because `/api/Plugin/List` already carries `runtimeKind`, `sourceKind`, `compatKind`, and `hasCapabilities`, the frontend can now distinguish:

- native Liteyuki plugins
- Python bridge plugins
- AstrBot-compatible plugins

### 4. Add a real capabilities area for Tools / MCP / Skills

The next frontend pass can already ship a practical page with three tabs:

- `Tools`
- `MCP`
- `Skills`

The first iteration should focus on:

- inventory
- schema/detail inspection
- warnings and diagnostics
- basic write actions already backed by the API

That means:

- tool enable/disable
- MCP save/test
- skill read/upload

The discovery helpers below should remain visible but non-toggleable:

- `list_tool_categories`
- `list_tools_in_category`
- `get_tool_schema`

## Backend Gaps Still Relevant To Frontend Planning

These are the remaining backend realities the frontend still needs to respect:

- legacy tasks are queryable but not executable
- capability snapshots are still runtime-based, not durable unloaded-plugin snapshots
- diagnostics are still shallow compared with a full runtime history view
- MCP deeper lifecycle control is still future work
- skill update/delete management is still future work

This means the frontend should not over-promise support state.

In capability views, the UI should still be prepared to represent support states such as:

- `registered_only`
- `deferred`
- `active`
- `disabled`
- `error`
- `unsupported`

## Cross-Page Constraints

Frontend changes in this lane should continue to use the existing runtime-aware request layer:

- `frontend/src/utils/runtime.ts`
- `frontend/src/utils/auth.ts`
- `frontend/src/utils/request.ts`

Do not reintroduce:

- hardcoded ports
- raw `fetch('/api/...')` patterns in the main app shell
- direct token parsing from storage in each page

Keep plugin pages and plugin runtime APIs conceptually separate:

- plugin pages are served resources under `/plugin/{plugin_id}/page/...`
- plugin runtime APIs are backend capability routes under `/api/Plugin/...`

## Recommended Build Order

### P0

- move chat and provider/model management out of `debug`
- add a first `AI / Capabilities` page
- upgrade plugin list badges and filters using existing backend fields

### P1

- build `AI / Prompts` on top of the shipped prompt profile APIs
- add plugin detail tabs for capabilities, runtime state, and diagnostics
- wire plugin pages back into plugin detail instead of treating them as a fully separate lane

### P2

- deeper plugin runtime diagnostics
- richer Tools / MCP / Skills lifecycle management
- post-scheduler views once legacy task or broader scheduler work exists

## Related Documents

- `docs/plugin-runtime-current-state.md`
- `docs/tools-mcp-skills-web-api.md`
- `docs/python-bridge-current-status-and-doc-audit.md`
