# Plugin Backend API Requirements

## Goal

Define the missing backend APIs and runtime surfaces needed to make the current plugin system, especially the Python AstrBot compatibility layer, actually usable beyond metadata registration.

This document focuses on backend requirements only.
Frontend shape can be added on top later, but backend capability boundaries should be fixed first.

## Implementation Status Snapshot

As of the current repository state on 2026-04-26, the backend is no longer in a metadata-only state.

Implemented:

- Phase 1 runtime capability snapshot
- Phase 2 capability query APIs
- Phase 3 registered web API execution
- Phase 4 registered tool execution
- Phase 5 cron scheduler backend for host-executable plugin cron jobs
- Phase 6 runtime state and diagnostics APIs

Still missing:

- legacy task execution backend
- durable capability snapshots when a plugin is unloaded or disabled
- richer diagnostics such as runtime hook failure history and scheduler execution history

## Repository-Verified Baseline

The current repository already contains several adjacent pieces that this work must align with:

- `src/plugin/sdk/python/compat_runtime.py`
  - retains AstrBot-compatible runtime registrations in module-scoped runtime state
  - currently stores `llm_tools`, `registered_web_apis`, `cron_jobs`, `scheduled_tasks`, and `agents`
- `src/plugin/sdk/python/lifecycle.rs`
  - owns Python plugin load, start, health check, shutdown, unload, and cleanup
  - clears Python-side runtime state on unload via `_cleanup_astrbot_plugin_runtime(...)`
- `tests/plugin_manager.rs`
  - already proves that AstrBot-compatible tool metadata, web API metadata, cron metadata, and legacy task metadata are retained inside the compatibility runtime
- `src/web/host/plugin_api.rs`
  - already exposes plugin list, enable/disable, import, store placeholder endpoints, explicit config read/write, runtime capability query endpoints, runtime tool execution, runtime state, and diagnostics
- `src/web/host/plugin_pages.rs`
  - already serves declared plugin pages under `/plugin/{plugin_id}/page/{page_path}`
  - injects an auth bridge for iframe-served plugin pages
  - is not a runtime web API dispatcher
- `src/llm/service.rs` and `src/llm/tools.rs`
  - already assemble the host LLM runtime tool bundle from local runtime tools, MCP tools, explicitly supplied extra tools, and plugin-registered AstrBot tools
- `src/app_host.rs`
  - already exposes plugin catalog snapshots, runtime capability snapshots, runtime web API dispatch, runtime tool execution, and runtime diagnostics for WebHost consumers

## Current State

Already available:

- plugin discovery, load, unload, enable, disable
- plugin config read and write for explicitly declared config files
- Python plugin runtime load, start, health check, shutdown, unload
- TUI command registration and scoped command enable/disable
- adapter text reply bridge for Python plugins
- plugin extension page discovery and serving for manifest-declared pages
- Rust-side typed capability snapshots and query APIs for:
  - tools
  - web APIs
  - cron jobs
  - legacy tasks
- host HTTP capability query APIs for:
  - `/api/Plugin/Capabilities`
  - `/api/Plugin/Capabilities/All`
  - `/api/Plugin/Tools`
  - `/api/Plugin/WebApis`
  - `/api/Plugin/CronJobs`
  - `/api/Plugin/Tasks`
- real backend execution for registered web APIs under:
  - `/api/Plugin/Runtime/WebApi/{plugin_id}/{registered_path...}`
- real backend execution for registered plugin tools through:
  - `PluginSdk::execute_plugin_tool(...)`
  - `PluginSdk::build_plugin_tool_bundle(...)`
  - `PluginSdk::build_all_plugin_tool_bundle(...)`
  - `POST /api/Plugin/Tools/Execute`
  - host LLM runtime bundle assembly used by `/api/LLM/Chat`
- real backend scheduling for host-executable registered cron jobs through:
  - `PluginCronTaskScheduler`
  - `PluginSdk::run_due_plugin_jobs(...)`
  - `EmbeddedAppHost::run_plugin_cron_tick(...)`
  - host-owned cron state persistence at `plugin-cron-state.json`
- runtime state and diagnostics query APIs for:
  - `/api/Plugin/RuntimeState`
  - `/api/Plugin/Diagnostics`
- AstrBot compatibility metadata retention for:
  - LLM tools
  - web APIs
  - cron jobs
  - legacy registered tasks
  - compat agents

Not yet available:

- task execution backend beyond truthful capability reporting
- persistence and startup recovery for legacy tasks or non-cron scheduler surfaces
- host-owned durable capability snapshots for unloaded or disabled plugins
- richer plugin diagnostics for:
  - runtime hook failures
  - scheduler/job execution history
  - durable failure retention across restart

Out of scope for the first backend pass even though metadata is already retained in the compat runtime:

- compat agents as a first-class exported host runtime surface

## Current State Matrix

| Capability | Metadata retained in Python compat runtime | Rust read/query API | Host HTTP query API | Runtime execution | Persistence / recovery |
| --- | --- | --- | --- | --- | --- |
| Plugin extension pages | Yes, via manifest metadata | Yes, through plugin catalog snapshot | Yes, through `/api/Plugin/List` extension page payload | Yes, static page serving only | Manifest-driven only |
| Registered LLM tools | Yes | Yes | Yes, through `/api/Plugin/Tools` | Yes, through host LLM tool runtime and `/api/Plugin/Tools/Execute` | No |
| Registered web APIs | Yes | Yes | Yes, through `/api/Plugin/WebApis` and `/api/Plugin/Capabilities` | Yes, through `/api/Plugin/Runtime/WebApi/...` | No |
| Registered cron jobs | Yes | Yes | Yes, through `/api/Plugin/CronJobs` and `/api/Plugin/Capabilities` | Yes, through the host cron scheduler and Python execution bridge | Yes, through `plugin-cron-state.json` |
| Registered legacy tasks | Yes | Yes | Yes, through `/api/Plugin/Tasks` and `/api/Plugin/Capabilities` | No | No |

## Architectural Boundaries

These boundaries should stay explicit during implementation:

- Plugin extension pages and plugin runtime web APIs are different surfaces.
  - Pages already exist under `/plugin/{plugin_id}/page/...`
  - Registered runtime web APIs should not be conflated with page serving
- The Python compatibility layer remains the registration source of truth only until Rust snapshot extraction is added.
- WebHost and LLM code should consume typed Rust snapshot models, not raw Python tuples or opaque Python objects.
- Capability query APIs must describe support truthfully.
  - Metadata retained in Python does not automatically mean executable backend support exists.

## Backend Requirement Summary

The backend should provide five layers:

1. plugin runtime capability snapshot
2. capability query APIs
3. execution APIs
4. persistence and lifecycle recovery APIs
5. diagnostics and support-state APIs

The recommended implementation order is the same.

## Canonical Status Model

Every query response for runtime plugin capabilities should be able to express at least these dimensions:

- `registered`
  - metadata exists in the runtime registration snapshot
- `executable`
  - the host can actually dispatch the capability right now
- `persistent`
  - capability state survives process restart through host-owned persistence
- `active`
  - capability is currently enabled and usable
- `status`
  - recommended enum values:
    - `registered_only`
    - `deferred`
    - `active`
    - `disabled`
    - `error`
    - `unsupported`

This status model should be used consistently across Web APIs, tools, cron jobs, tasks, and plugin-wide summaries.

## Phase 1: Runtime Capability Snapshot

### Requirement

Expose plugin runtime registrations from the Python bridge back into Rust in a stable, queryable form.

### Why

The current compat layer retains tool, web API, cron, and task metadata in Python runtime state.
The Rust host can now inspect and serve those registrations through capability, runtime web API, tool execution, runtime state, and diagnostics APIs.
The remaining gap is no longer "visibility", but truthful execution-state reporting and the still-missing legacy task execution backend.

### Required Rust-side snapshot objects

Add backend models for:

- `PluginRegisteredTool`
- `PluginRegisteredWebApi`
- `PluginRegisteredCronJob`
- `PluginRegisteredTask`
- `PluginCapabilitySnapshot`

### Minimum fields

`PluginRegisteredTool`

- `plugin_id`
- `name`
- `description`
- `parameters`
  - keep as JSON Schema-like `serde_json::Value`
- `active`
- `source`
  - `astrbot_decorator`
  - `astrbot_context`
  - future extensible
- `handler_module_path`
  - optional but strongly recommended for diagnostics

`PluginRegisteredWebApi`

- `plugin_id`
- `route`
- `methods`
- `description`
- `source`
- `runtime_kind`
- `handler_module_path`
  - optional if extraction is not available in the first pass

`PluginRegisteredCronJob`

- `plugin_id`
- `job_id`
- `job_type`
- `name`
- `description`
- `cron_expression`
- `run_once`
- `enabled`
- `timezone`
- `persistent`
- `payload`
- `next_run_time`
  - optional in snapshot-only phase, required once scheduler exists
- `last_run_time`
  - optional in snapshot-only phase, required once scheduler exists
- `last_error`
  - optional in snapshot-only phase, required once scheduler exists

`PluginRegisteredTask`

- `plugin_id`
- `task_id`
  - required because the current compat layer retains the task token/object, not only a description
- `description`
- `task_kind`
  - keep generic for now
- `source`

`PluginCapabilitySnapshot`

- `plugin_id`
- `runtime_kind`
- `tools`
- `web_apis`
- `cron_jobs`
- `tasks`
- `updated_at`

### Extraction requirements

- Snapshot extraction must be read-only in the first pass.
- Extraction should be resilient when a plugin is loaded but not yet fully executable.
- Snapshot data should be associated with the plugin id, not only Python module names.
- Returned data should survive normal plugin load state inspection, but be cleared on unload.
- Snapshot extraction should not force HTTP routes or LLM execution support to exist first.
- Decorator-only Python metadata such as aliases, regex filters, nested command groups, and event filters should be treated as a separate projection problem from the first capability snapshot pass.

### Required SDK methods

Add read-only query methods on the Rust side, ideally under `PluginSdk`:

- `get_plugin_capabilities(plugin_id)`
- `list_plugin_tools(plugin_id)`
- `list_plugin_web_apis(plugin_id)`
- `list_plugin_cron_jobs(plugin_id)`
- `list_plugin_tasks(plugin_id)`
- `list_all_plugin_capabilities()`

### Recommended implementation notes

- Extraction logic should stay close to the Python bridge and lifecycle code, not inside WebHost.
- The resulting snapshot types should be plain Rust structs suitable for:
  - HTTP serialization
  - diagnostics
  - future execution binding
- Snapshot extraction should be written so that plugin unload cleanup remains deterministic.

## Phase 2: Capability Query APIs

### Requirement

Expose plugin runtime capabilities through host backend APIs for WebUI and diagnostics.

### Why

Current plugin APIs expose plugin list and config, but not runtime capabilities.

### Required HTTP endpoints

Add backend endpoints equivalent to:

- `GET /api/Plugin/Capabilities?id=<plugin_id>`
- `GET /api/Plugin/Tools?id=<plugin_id>`
- `GET /api/Plugin/WebApis?id=<plugin_id>`
- `GET /api/Plugin/CronJobs?id=<plugin_id>`
- `GET /api/Plugin/Tasks?id=<plugin_id>`

Optionally also:

- `GET /api/Plugin/Capabilities/All`

### Response contract

These endpoints should follow the existing host API envelope style:

- `{ code, message, data }`

Recommended `Capabilities` payload shape:

```json
{
  "pluginId": "astrbot-context-tools",
  "runtimeKind": "python",
  "support": {
    "tools": {
      "registered": true,
      "executable": true,
      "persistent": false,
      "active": true,
      "status": "active"
    },
    "webApis": {
      "registered": true,
      "executable": true,
      "persistent": false,
      "active": true,
      "status": "active"
    },
    "cronJobs": {
      "registered": true,
      "executable": false,
      "persistent": false,
      "active": true,
      "status": "registered_only"
    },
    "tasks": {
      "registered": true,
      "executable": false,
      "persistent": false,
      "active": true,
      "status": "registered_only"
    }
  },
  "snapshot": {
    "tools": [],
    "webApis": [],
    "cronJobs": [],
    "tasks": [],
    "updatedAt": "2026-04-25T00:00:00Z"
  }
}
```

### Important constraint

Do not report registered metadata as fully supported execution capability if the backend cannot run it yet.

## Phase 3: Registered Web API Execution

### Requirement

Allow plugins to register runtime HTTP APIs that are actually routed by the host.

### Why

`Context.register_web_api()` is already compatible at metadata level, but there is no real router binding.

### Required backend behavior

- mount plugin-registered routes into host routing
- dispatch to the Python handler through the existing bridge
- support at least:
  - `GET`
  - `POST`
  - `PUT`
  - `PATCH`
  - `DELETE`
- provide normalized request context:
  - method
  - path
  - query
  - headers
  - body
  - peer IP if available

### Recommended route shape

Keep query and management APIs under `/api/Plugin/...`, but mount executable plugin runtime web APIs under a host-owned namespace such as:

- `/api/Plugin/Runtime/WebApi/{plugin_id}/{registered_path...}`

Reasons:

- avoids collisions with core `/api/Plugin/...` management endpoints
- reuses the existing `/api` auth boundary
- keeps runtime APIs clearly separate from `/plugin/{plugin_id}/page/...`

### Required response contract

Plugin web API handlers should be able to return:

- plain text
- JSON object
- explicit status code plus body
- later optional binary response support

### Safety boundary

- route namespace must be plugin-scoped or otherwise collision-safe
- duplicate normalized route plus method combinations should fail deterministically
- unload must remove mounted plugin routes
- disabled plugins must not keep serving mounted runtime routes
- handler failure must return an API error response, not crash the plugin host

## Phase 4: Registered Tool Execution

### Requirement

Bridge AstrBot-compatible registered tools into the host LLM tool runtime.

### Why

The compat layer can already collect tool metadata, but the actual host tool executor is separate.

### Required backend behavior

- convert plugin registered tools into host-executable `LlmFunctionTool` descriptors
- execute Python handlers on demand
- support tool activation and deactivation state
- surface execution errors as tool call errors, not plugin crashes

### Required backend methods

- `build_plugin_tool_bundle(plugin_id)`
- `build_all_plugin_tool_bundle()`
- `execute_plugin_tool(plugin_id, tool_name, arguments)`

Current repository status:

- implemented on `PluginSdk`
- implemented in the Python runtime bridge
- exposed through `POST /api/Plugin/Tools/Execute`
- integrated into the host LLM runtime bundle used by `/api/LLM/Chat`

### Naming and collision rules

Tool names must be globally collision-safe.

Recommended approach:

- keep the original plugin-declared tool name as metadata
- expose a canonical runtime-safe tool id such as:
  - `plugin::{plugin_id}::{tool_name}`

This avoids collisions with:

- built-in host tools
- MCP tools
- tools from other plugins

### Required integration points

- WebUI capability inspection
- host LLM runtime tool bundle assembly
- later MCP or external tool export if needed

### Constraints

- plugin unload must remove executable tool bindings
- disabled plugins must not remain callable through the LLM tool runtime
- long-running tools need timeout and cancellation semantics
- tool execution should be attachable to future diagnostics for:
  - last execution error
  - last execution timestamp
  - timeout / cancellation result

## Phase 5: Cron and Task Scheduler Backend

### Requirement

Keep the shipped cron scheduler backend stable and add the still-missing task execution backend.

### Why

The compat layer now has a real host-owned cron scheduler with persistence and recovery.
The remaining execution gap in this phase is legacy tasks rather than cron visibility or cron persistence.

### Required minimum scheduler capabilities

- recurring cron jobs
- one-shot jobs
- enable and disable
- update and delete
- list jobs by plugin
- dispatch into plugin runtime on trigger

### Required backend methods

- `register_plugin_cron_job(...)`
- `update_plugin_cron_job(...)`
- `delete_plugin_cron_job(...)`
- `list_plugin_cron_jobs(plugin_id)`
- `run_due_plugin_jobs()`

### Required persistence

Persist at least:

- plugin id
- job id
- schedule
- payload
- enabled
- run_once
- timezone
- next run time
- last run time
- last error
- scheduler status

### Required lifecycle behavior

- restore persisted jobs on startup
- remove or disable orphaned jobs when the plugin is missing
- unload should detach runtime bindings
- disabled plugins should not keep executing scheduled jobs
- persistence ownership should live on the Rust host side, not only inside Python runtime objects

### Task support note

The current compatibility layer also retains legacy task registrations.
That does not automatically imply a full task-execution engine should ship in the same step.

The minimum first pass is:

- include tasks in capability snapshots
- report task support truthfully through status fields
- defer complex task orchestration until after cron job execution exists

## Phase 6: Plugin Diagnostics and State APIs

### Requirement

Expose runtime inspection and failure state for plugin features that are more dynamic than static manifest metadata.

### Required query areas

- registered tools
- registered web APIs
- registered cron jobs
- registered tasks
- runtime hook failures
- last execution error for tool or cron job
- capability support matrix per plugin

### Suggested endpoints

- `GET /api/Plugin/RuntimeState?id=<plugin_id>`
- `GET /api/Plugin/Diagnostics?id=<plugin_id>`

Current repository status:

- both endpoints are implemented
- current payloads already cover runtime kind, load state, snapshot extracted or not, executable binding presence, scheduler status, last web API dispatch error, last tool execution error, and last cron execution state
- runtime hook failure history and scheduler execution history are still not implemented

### Suggested diagnostics payload areas

- plugin runtime kind
- plugin load state
- snapshot extracted successfully or not
- executable bindings installed or not
- scheduler status for plugin jobs
- last web API dispatch error
- last tool execution error
- last cron execution error

## Non-Goals For This Phase

These do not need to be solved in the first backend pass:

- full AstrBot provider manager parity
- full agent orchestration parity
- dashboard-perfect AstrBot API cloning
- Python sandboxing
- distributed scheduling

## Recommended Delivery Order

### Step 1

Implement Rust-side capability snapshot extraction from Python runtime.

This is the lowest-risk step and unlocks query APIs without pretending execution support already exists.

### Step 2

Expose read-only capability APIs for WebUI and debugging.

At this step, the response contract must already distinguish:

- registered metadata
- executable backend support
- persistent support

### Step 3

Implement runtime web API dispatch.

This is the first place where plugin runtime capability metadata becomes a real host-serving surface.

### Step 4

Implement plugin tool execution bridge into host LLM tools.

This step should integrate with the existing LLM tool assembly path instead of inventing a separate executor stack.

### Step 5

Implement scheduler backend and persistence.

At this point the host should become responsible for restart recovery and orphan handling.

### Step 6

Add diagnostics, status reporting, and failure introspection.

This should be additive and should not change the core execution contracts.

## Suggested Test Coverage

At minimum, add or extend tests for:

- snapshot extraction from an AstrBot-style plugin that registers:
  - at least one tool
  - one web API
  - one cron job
  - one legacy task
- query endpoints returning runtime capability data for a loaded plugin
- capability responses correctly reporting executable support for tools and web APIs
- runtime web API route dispatch, including method filtering and unload cleanup
- plugin tool execution success, failure, timeout, and unload cleanup
- host LLM runtime integration proving plugin tools are callable from `/api/LLM/Chat`
- scheduler persistence restore and disabled-plugin behavior
- unload and reload not leaving stale capabilities behind

The current suite already provides a useful starting point for fixture reuse:

- `tests/plugin_manager.rs`
  - AstrBot-compatible runtime metadata retention for tools, web APIs, cron jobs, and tasks
  - unload cleanup and Python runtime lifecycle coverage
  - command dispatch and scoped command policy behavior

## Acceptance Criteria

- backend can enumerate plugin runtime capabilities, not only manifest metadata
- WebUI can query per-plugin tools, web APIs, cron jobs, and tasks
- capability query responses distinguish metadata-only support from executable support
- registered web APIs are actually callable through the host router
- registered plugin tools can be executed by the host tool runtime
- cron jobs and tasks have truthful support-state reporting
- cron jobs have real backend scheduling support once Phase 5 lands
- unload and reload do not leave stale capability registrations behind

## Remaining Gaps After Current Implementation

The repository now satisfies capability query, runtime web API execution, plugin tool execution, and basic diagnostics/state APIs.

The main remaining backend gaps are:

- host-executable cron jobs now run through a Rust-owned scheduler backend with persisted state recovery
- legacy tasks are still status-only and do not have a task execution engine
- plugin capability snapshots are still runtime-state projections and are not persisted when a plugin is not loaded
- diagnostics are currently last-error and last-success snapshots, not a full failure timeline

## Suggested Landing Points In Current Codebase

The current repository layout suggests these implementation boundaries:

- `src/plugin/sdk/mod.rs`
  - stable Rust-side query surface on `PluginSdk`
- `src/plugin/sdk/python/*`
  - extraction of Python-side compat registrations into typed Rust models
- `src/web/host/plugin_api.rs`
  - read/query endpoints under `/api/Plugin/...`
- `src/web/host/router.rs`
  - routing entrypoint for executable runtime web APIs
- `src/llm/service.rs`
  - host LLM runtime assembly integration
- `src/llm/tools.rs`
  - keep host local tools and plugin-exposed tools clearly separated, then merge with collision checks
- new focused runtime modules as needed for:
  - plugin capability models
  - plugin capability extraction
  - plugin web API runtime
  - plugin tool execution runtime
  - plugin scheduler runtime

## Implementation Notes

- Keep the Python compat runtime as the registration source of truth only until Rust snapshot extraction is added.
- Do not couple this work to frontend rendering details.
- Do not let WebHost parse raw Python tuple formats directly.
- Prefer small focused Rust modules over pushing all capability handling into one large file.
- Preserve the already-working plugin page serving path and auth bridge; this document is about the missing runtime capability backend surfaces around it.
