# Python Bridge Refactor Plan

## Current Status

Status as of 2026-04-25:

- `src/plugin/sdk.rs` has already been split into `src/plugin/sdk/` modules
- the shared-interpreter runtime hardening work in this plan is largely complete
- AstrBot compatibility has moved beyond metadata retention for tools and web APIs:
  - tools can now be bridged into the Rust host runtime and executed
  - web APIs can now be dispatched through the host runtime route bridge
- cron jobs and legacy tasks are still registration-only and do not have a real scheduler backend

This file should be read as the refactor plan and phase history for the bridge lane, not as the authoritative current-state document.

## Goal

This round focuses on the Rust-side PyO3 bridge in `src/plugin/sdk/` and adjacent plugin runtime code.

Primary targets:

- reduce concrete runtime risks in the current shared-interpreter bridge
- improve reload/unload behavior so Python plugin state is less stale
- lower maintenance complexity around `sdk.rs`
- keep room for AstrBot plugin API compatibility

Out of scope for this round:

- LLM-specific AstrBot compatibility or provider-facing Python APIs
- attempting to sandbox arbitrary Python stdlib or third-party imports
- process-isolated Python runtimes

## Current Problems

1. `plan_load()` imports Python modules during the probe stage, so plugin top-level code can execute before runtime activation.
2. unload currently clears Rust-side state only; Python module cache and injected search paths are not cleaned.
3. `shutdown` / `unload` can call back into Python while still holding the Rust runtime mutex.
4. `src/plugin/sdk.rs` has accumulated multiple responsibilities and is too large to evolve safely.
5. compatibility work is currently centered on old Liteyuki-style Python plugins; AstrBot compatibility is not yet structured.

## Implementation Order

### Phase 1: Runtime hardening

- stop importing plugin modules during `plan_load()`
- move Python import/activation into the actual load stage
- track plugin-related module names and search paths during load
- clean module cache and removable search paths during unload
- avoid calling Python lifecycle hooks while holding `python_runtime` locks

### Phase 2: Structural decoupling

- split `sdk.rs` by responsibility instead of adding more helper blocks into the same file
- preferred extraction directions:
  - Python runtime state and command catalog helpers
  - Python bridge/probe/import helpers
  - Python config read/write helpers
- keep public `PluginSdk` API stable while moving internals

### Phase 3: AstrBot compatibility baseline

- document the minimum AstrBot compatibility surface needed for simple plugins
- prioritize import-level and event/decorator-level compatibility before broader platform/provider features
- preserve tool / web-api / scheduled-task registration metadata early so later backend work has a stable compatibility seam
- explicitly avoid overlapping with the repo's separate LLM work

### Phase 4: Host-side capability follow-up

- expose registered AstrBot compatibility metadata from Rust so backend/frontend can inspect it
- decide which surfaces stay metadata-only and which become executable in the host
- add explicit capability reporting for:
  - registered LLM tools
  - registered web APIs
  - registered cron / scheduled tasks

## Acceptance Criteria

- plugin planning no longer imports Python plugin modules
- reload/unload removes plugin-owned Python module cache entries
- shutdown/unload no longer invoke Python hooks while holding the runtime mutex
- `sdk.rs` is materially smaller and no longer the only home for Python bridge internals
- targeted regression tests cover at least load/unload/reload-sensitive behavior
- AstrBot compatibility can retain tool / web-api / scheduled-task metadata without silently dropping registrations

## Constraints

- shared interpreter is acceptable for now
- untrusted plugin code is still effectively trusted process-local code; this round improves engineering hygiene, not full containment
- avoid broad refactors outside the plugin bridge lane
- do not touch ongoing LLM-related compatibility work unless strictly required by compilation
