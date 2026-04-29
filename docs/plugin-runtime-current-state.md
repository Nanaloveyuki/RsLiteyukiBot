# Plugin Runtime Current State

## Purpose

This document is the current-state replacement for the older plugin backend requirements draft.

Use it when you need the real plugin runtime and Web Host surfaces that already exist in this repository, not the historical implementation plan.

Repository state verified against the current worktree on 2026-04-26.

## Already Implemented

### Plugin management and catalog

The backend already supports:

- plugin discovery, load, unload, enable, disable
- plugin config read/write for declared config files
- plugin zip import
- plugin extension page serving under `/plugin/{plugin_id}/page/{page_path}`
- iframe page auth bridge for plugin pages

The Web Host already exposes:

- `GET /api/Plugin/List`
- `POST /api/Plugin/SetStatus`
- `POST /api/Plugin/Import`
- `GET /api/Plugin/Config?id=<plugin_id>`
- `POST /api/Plugin/Config`
- `GET /api/Plugin/Store/List`
- `GET /api/Plugin/Store/Detail/{id}`

### Runtime capability query APIs

The backend already exports runtime capability snapshots through:

- `GET /api/Plugin/Capabilities`
- `GET /api/Plugin/Capabilities/All`
- `GET /api/Plugin/Tools`
- `GET /api/Plugin/WebApis`
- `GET /api/Plugin/CronJobs`
- `GET /api/Plugin/Tasks`
- `GET /api/Plugin/RuntimeState`
- `GET /api/Plugin/Diagnostics`

### Runtime execution

The backend already executes:

- registered plugin tools through `POST /api/Plugin/Tools/Execute`
- registered runtime web APIs through `/api/Plugin/Runtime/WebApi/{plugin_id}/{registered_path...}`
- host-executable cron jobs through the host-owned plugin cron scheduler

The host LLM runtime also includes plugin tools in the shared tool bundle used by `/api/LLM/Chat`.

### Python bridge / AstrBot compatibility

The current Python bridge already retains and exports metadata for:

- tools
- web APIs
- cron jobs
- legacy tasks
- compat agents

The important boundary is that retained metadata and executable support are not the same thing for every surface.

## `/api/Plugin/List` Current Shape

`/api/Plugin/List` is no longer a display-only route.

The current payload already includes plugin classification and runtime-facing fields such as:

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
- `sourcePath`
- `homepage`

This is enough for the frontend to start building real plugin classification, filtering, badges, and capability entry points.

## Current Capability Matrix

| Surface | Metadata retained | Host query API | Runtime execution | Persistence / recovery |
| --- | --- | --- | --- | --- |
| Extension pages | Yes | Yes | Yes, static page serving | Manifest-driven only |
| Tools | Yes | Yes | Yes | No |
| Web APIs | Yes | Yes | Yes | No |
| Cron jobs | Yes | Yes | Yes | Yes, `plugin-cron-state.json` |
| Legacy tasks | Yes | Yes | No | No |

## Practical Boundaries

- Plugin extension pages and runtime web APIs are different surfaces.
- Runtime capability APIs are snapshot/query routes, not page-serving routes.
- Cron jobs now have a real backend scheduler path.
- Legacy tasks are still registration-visible but not executable.
- Current capability visibility is runtime-based; unloaded or disabled plugins do not yet have durable host-owned capability snapshots.

## Remaining Gaps

The main backend gaps still worth tracking are:

- no legacy task execution backend
- no durable capability snapshot for unloaded or disabled plugins
- no richer diagnostics history for hook failures, scheduler runs, or failure retention across restart
- no full AstrBot provider/agent parity

## Recommended Reading Order

For current plugin/runtime reality, read in this order:

1. `docs/plugin-runtime-current-state.md`
2. `docs/python-bridge-current-status-and-doc-audit.md`
3. `docs/python-bridge-compatibility-notes.md`
4. `docs/python-bridge-risk-register.md`

Use `docs/python-bridge-refactor-plan.md` only when you need the historical bridge plan.
