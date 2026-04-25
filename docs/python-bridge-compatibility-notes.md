# Python Bridge Compatibility Notes

## Compatibility Direction

There are two Python-side compatibility targets in this repository:

1. legacy Liteyuki-style plugins already partially supported by the existing bridge
2. `dev-docs/AstrBot-master` plugin APIs that need gradual compatibility

This round should keep Liteyuki compatibility stable while preparing a cleaner base for AstrBot compatibility.

## AstrBot Scope For This Round

Do first:

- keep the minimum import surface stable for simple class-based plugins
- support the simplest event and decorator registration patterns first
- avoid coupling the bridge refactor to AstrBot's LLM/provider stack

Do not mix into this round unless required:

- full provider manager parity
- real cron execution and dashboard persistence
- full dashboard-specific APIs beyond the current runtime bridge surfaces

## Minimum AstrBot API Surface To Track

Based on the checked-in `dev-docs/AstrBot-master` codebase, the most relevant non-LLM entry surfaces are:

- `astrbot.api.star`
- `astrbot.api.event`
- `astrbot.api.event.filter`
- `astrbot.api.platform`
- `astrbot.core.star.register`
- `astrbot.core.platform.astr_message_event`

The most likely early compatibility candidates are:

- `Star`
- `Context`
- `register`
- command-like decorators
- message event wrapper methods that simple plugins call directly

Currently covered by the compat layer:

- `from astrbot.api import star, logger`
- `from astrbot.api import FunctionTool, ToolSet`
- `from astrbot.api.event import AstrMessageEvent, MessageEventResult, filter`
- class-based `Star` plugin discovery and runtime binding
- `@filter.command(...)`
- `@filter.command_group(...)` nested prefix composition with alias propagation
- `@filter.event_message_type(...)`
- `@filter.on_astrbot_loaded()`
- `@filter.llm_tool(...)` metadata registration into a compatibility tool manager
- `MessageEventResult` reply delivery through the existing adapter reply bridge
- minimal positional command argument injection for handlers such as `async def foo(self, event, arg1, arg2)`
- `Context.get_llm_tool_manager()` / `add_llm_tools()` / `activate_llm_tool()` / `deactivate_llm_tool()`
- `Context.register_web_api()` metadata retention
- `Context.cron_manager.add_active_job()` / `add_basic_job()` / `list_jobs()` metadata retention
- deprecated `Context.register_task()` metadata retention
- compat exports for `astrbot.core.agent.tool`, `astrbot.core.agent.tool_executor`, and `astrbot.core.provider.register`

Known intentional gaps:

- no full AstrBot command parser parity
- no typed argument conversion or richer parsed-parameter model
- no provider manager parity beyond tool metadata retention
- no real cron execution backend
- no dashboard persistence for registered cron jobs / web apis
- web api execution support is limited to the host runtime route bridge, not a full Quart server
- no real agent handoff execution path

Now implemented in the host:

- registered AstrBot tools can be bridged into the Rust host LLM runtime
- registered AstrBot tools can be executed directly through the host plugin API surface
- registered AstrBot web APIs can be dispatched through the host runtime route bridge
- runtime state and last-error diagnostics can be queried through the host plugin API surface

Still intentionally missing:

- no provider manager parity beyond tool metadata retention
- no cron scheduler backend or persistence recovery
- no durable unloaded-plugin capability snapshot

## Compatibility Strategy

1. stabilize the runtime first
2. move bridge internals into smaller modules
3. add AstrBot compat shims only on top of a cleaner runtime

Reason:

- AstrBot compatibility built on top of stale reload semantics or lock-sensitive lifecycle code would create hard-to-debug failures
- the bridge should first become predictable, then broader

Current runtime note:

- async Python handlers now run through a single dedicated bridge loop instead of ad-hoc per-callback loops
- this keeps AstrBot-style async callbacks on one host-managed execution lane
- pending background tasks are cancelled after each callback to avoid leaking old plugin state across reloads
- registered AstrBot tools can now be executed through the host tool runtime bridge
- registered AstrBot web APIs can now be dispatched through the host runtime route bridge
- cron and legacy task compatibility in this round is still metadata-preserving only
- unload now clears module-owned AstrBot compat runtime state before removing Python modules

## Coordination Note

Another active workstream is handling LLM-related content.

This bridge pass should avoid overlapping with:

- provider protocol compatibility
- LLM request/response bridge semantics
- model invocation helpers

If future AstrBot compatibility work needs those surfaces, it should build on a separate, explicit follow-up.
