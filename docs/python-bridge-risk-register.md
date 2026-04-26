# Python Bridge Risk Register

## Risk Posture

The current PyO3 bridge is a shared-interpreter compatibility layer for local or ecosystem plugins.
It is not a secure sandbox for untrusted Python code.

That means:

- Python plugins can still import Python stdlib and third-party modules freely
- permission checks on `sdk` methods only protect host-exposed helpers
- this round should improve runtime correctness and reduce accidental cross-plugin breakage
- this round should not pretend to fully secure arbitrary Python execution

## High Priority Risks

### Probe-side effects

Previous issue:

- planning and probing could import plugin modules too early

Impact:

- top-level plugin code can run before bridge activation
- probe can mutate global Python state
- repeated planning can leave stale imports behind

Target:

- make planning parse/prepare only
- defer actual import to load

### Stale module cache on reload

Previous issue:

- unload cleaned Rust-side registration but left `sys.modules` state behind

Impact:

- reload may keep old module objects and globals
- code changes may not take effect cleanly
- plugin package submodules can leak across reloads

Target:

- capture plugin-owned module names during load
- remove them during unload

### Search path pollution

Previous issue:

- plugin search paths were appended to `sys.path` without cleanup

Impact:

- module resolution can drift over time
- unrelated plugins can accidentally import each other's files
- reload behavior becomes harder to reason about

Target:

- track search paths per plugin
- remove paths when no loaded plugin still depends on them

### Lock + callback deadlock surface

Previous issue:

- some lifecycle hooks were invoked while holding the Rust runtime mutex

Impact:

- plugin hook code can deadlock if it calls back into SDK methods that also need the same lock

Target:

- copy required Python refs out under lock
- release lock before calling Python

### Async event-loop churn

Naive await handling can recreate a fresh Python `asyncio` loop for every plugin callback or split one plugin across multiple host threads.

Impact:

- high-frequency message events pay repeated loop startup and teardown cost
- async plugin hooks cannot reuse loop-local resources predictably
- AstrBot-style async handlers become more expensive than they need to be

Current status:

- async Python awaitables are now executed on a dedicated bridge thread with a single persistent loop

Remaining hazard:

- this is still a host-managed compatibility executor, not full Python task supervision
- plugins should not assume long-lived background tasks survive reload or unload

Additional mitigation:

- pending tasks created during one callback are cancelled before the next callback finishes

## Medium Priority Risks

### Global compat module overwrite

The bridge injects compat modules into `sys.modules` globally.

Impact:

- shared interpreter plugins can observe mutable global compat state
- plugin-local assumptions around imported compat modules can drift

Target for this round:

- keep behavior compatible
- avoid making the problem worse
- structure bridge code so later per-plugin compat binding is possible

Current mitigation:

- obvious class-level mutable compat state such as `Context.registered_web_apis` now stays instance-local
- module-owned AstrBot compat runtime state is now explicitly cleaned during unload

### Metadata-only compatibility can be mistaken for real execution support

Current issue:

- AstrBot-style tool registration, web api registration, and cron/job registration now preserve metadata in the compat layer
- legacy task registration is still metadata-only in the Rust host
- tool execution and web api execution now have real host bridges
- host-executable cron jobs now have a real scheduler backend

Impact:

- plugin authors can still believe every retained compat surface is fully supported because import-time registration succeeds
- later backend/frontend work may assume execution already exists when only registration exists

Current mitigation:

- expose capability state explicitly so tools, web apis, and host-executable cron jobs report executable support while legacy tasks still report registration-only support
- document clearly which surfaces are executable and which are still placeholders or task-pending

Follow-up target:

- keep capability and diagnostics payloads truthful as scheduler/runtime support expands

### Default config fallback ambiguity

If a plugin has config permissions but no explicit plugin config path, config helpers can fall back to app-level paths.

Impact:

- plugin config read/write behavior can be surprising

Target for this round:

- keep behavior stable unless a safe tightening is obvious
- document the hazard

## Non-Goals

- blocking Python builtins, stdlib, subprocesses, sockets, or filesystem access
- building a capability-secure Python sandbox inside the same process
- solving every AstrBot API surface in one change
