# Common Plugin ABI And Source Adapter Design

## Goal

Define a host-owned common plugin ABI for RsLiteyukiBot that:

- keeps the current Rust host manifest model as the single source of truth for discovery and runtime planning
- does not require editing third-party plugin source code just to make it discoverable
- supports multiple plugin source families under `%USERPROFILE%/.liteyuki/plugins/`
- uses override mapping files to translate source-specific metadata and config conventions into a unified host descriptor

This design is intentionally about a **common host manifest / adapter ABI**, not a promise that every foreign ecosystem plugin can execute natively without a runtime adapter.

## Repository-Verified Current State

The current repository already has several important pieces:

- host plugin discovery is manifest-first and scans `plugin.json`
  - [src/plugin/loader.rs](/E:/repo/RsLiteyukiBot/src/plugin/loader.rs:32)
- the host runtime descriptor model is already unified around:
  - `PluginMetadata`
  - `PluginRuntimeSpec`
  - `PluginSdkSpec`
  - `PluginDescriptor`
  - [src/plugin/model.rs](/E:/repo/RsLiteyukiBot/src/plugin/model.rs:24)
- the default user-local plugin root is already `%USERPROFILE%/.liteyuki/plugins`
  - [src/runtime_support.rs](/E:/repo/RsLiteyukiBot/src/runtime_support.rs:386)
- `/api/Plugin/List` already emits family/classification-facing fields:
  - `runtimeKind`
  - `pluginType`
  - `sourceKind`
  - `compatKind`
  - [src/web/host/mod.rs](/E:/repo/RsLiteyukiBot/src/web/host/mod.rs:1022)
- Python bridge planning already supports host-side entrypoint and path overrides through manifest `runtime.options`
  - [src/plugin/sdk/python/probe.rs](/E:/repo/RsLiteyukiBot/src/plugin/sdk/python/probe.rs:60)
  - [src/plugin/sdk/python/lifecycle.rs](/E:/repo/RsLiteyukiBot/src/plugin/sdk/python/lifecycle.rs:259)
- AstrBot compatibility already exists as a host-managed shim layer, not as raw source execution parity
  - [src/plugin/sdk/python/compat_runtime.py](/E:/repo/RsLiteyukiBot/src/plugin/sdk/python/compat_runtime.py:1)
  - [docs/python-bridge-compatibility-notes.md](/E:/repo/RsLiteyukiBot/docs/python-bridge-compatibility-notes.md:5)

This means the repository is already structurally closer to a host-owned adapter model than to direct source-native plugin loading.

## Why A Common ABI Is Needed

The checked-in compatibility references show that plugin ecosystems differ at more than one layer:

### AstrBot

- plugin metadata comes from `metadata.yaml`
  - [dev-docs/AstrBot-master/docs/zh/dev/star/plugin-new.md](/E:/repo/RsLiteyukiBot/dev-docs/AstrBot-master/docs/zh/dev/star/plugin-new.md:26)
- plugin config schema comes from `_conf_schema.json`
  - [dev-docs/AstrBot-master/docs/en/dev/star/guides/plugin-config.md](/E:/repo/RsLiteyukiBot/dev-docs/AstrBot-master/docs/en/dev/star/guides/plugin-config.md:10)
- runtime assumes `Star`, `Context`, decorators, and AstrBot-managed config injection

### Legacy Liteyuki Python Plugins

- plugin metadata commonly uses `__plugin_meta__`
  - [dev-docs/legacy-python/liteyuki/plugin/model.py](/E:/repo/RsLiteyukiBot/dev-docs/legacy-python/liteyuki/plugin/model.py:38)
  - [dev-docs/python-src/liteyuki_plugins/scheduled_tasks/__init__.py](/E:/repo/RsLiteyukiBot/dev-docs/python-src/liteyuki_plugins/scheduled_tasks/__init__.py:11)
- runtime behavior is closer to lightweight Python module bootstrap and custom host callbacks

### NoneBot

- runtime is not just metadata-driven; it expects a full NoneBot app lifecycle
  - `nonebot.init(...)`
  - `nonebot.load_plugin(...)`
  - matcher/driver/adapter globals
  - [dev-docs/python-src/liteyuki_plugins/nonebot/__init__.py](/E:/repo/RsLiteyukiBot/dev-docs/python-src/liteyuki_plugins/nonebot/__init__.py:15)
  - [dev-docs/python-src/liteyuki_plugins/nonebot/np_main/loader.py](/E:/repo/RsLiteyukiBot/dev-docs/python-src/liteyuki_plugins/nonebot/np_main/loader.py:18)
- many example plugins directly depend on `nonebot_plugin_alconna`, matchers, permissions, and NoneBot globals
  - [dev-docs/python-src/liteyuki_plugins/nonebot/nonebot_plugins/liteyuki_pacman/rpm.py](/E:/repo/RsLiteyukiBot/dev-docs/python-src/liteyuki_plugins/nonebot/nonebot_plugins/liteyuki_pacman/rpm.py:14)

Conclusion:

- a common **host manifest** is realistic
- a common **source runtime ABI** is only partially realistic
- AstrBot and legacy Liteyuki can be adapted into the host bridge
- NoneBot needs a dedicated runtime adapter or sidecar and cannot be truthfully treated as “just another Python plugin with different JSON”

## Main Architectural Decision

Use a two-layer model:

1. `Source Plugin Layout`
   - raw plugin files kept in source-family-specific directories
2. `Host Override Manifest`
   - a host-owned mapping file that produces a normalized `PluginDescriptor`

The host should never assume a foreign source tree is directly executable just because it is discoverable.

## Proposed Directory Layout

Keep `%USERPROFILE%/.liteyuki/plugins/` as the single user-local plugin root, but split source payloads from host override manifests:

```text
%USERPROFILE%/.liteyuki/plugins/
  manifests/
    astrbot-hello.override.json
    liteyuki-weather.override.json
    nonebot-packman.override.json
  astrbot_plugin/
    astrbot-hello/
      metadata.yaml
      _conf_schema.json
      main.py
      requirements.txt
  liteyukibot_plugin/
    liteyuki-weather/
      __init__.py
      ...
  nonebot_plugin/
    nonebot-packman/
      __init__.py
      ...
  native_plugin/
    builtin-like-native/
      plugin.json
      ...
```

Why split `manifests/` from raw source directories:

- raw source trees remain close to their original ecosystem layout
- override files stay small and host-owned
- plugin import, rollback, and later migration are easier
- the root does not become a mix of foreign layouts plus generated files

## Common Host Override Schema

The override file should map one raw source plugin into one host descriptor.

Suggested shape:

```json
{
  "version": 1,
  "pluginId": "astrbot-hello",
  "source": {
    "kind": "astrbot",
    "path": "astrbot_plugin/astrbot-hello",
    "metadataFiles": ["metadata.yaml", "_conf_schema.json"],
    "configStrategy": {
      "kind": "astrbot_schema"
    }
  },
  "host": {
    "name": "AstrBot Hello",
    "description": "Mapped from AstrBot metadata.yaml",
    "pluginType": "service",
    "runtime": {
      "kind": "python",
      "entrypoint": "main",
      "abi": "liteyuki-python-bridge",
      "options": {
        "python_path": [".", "astrbot_plugin/astrbot-hello"],
        "compat_family": "astrbot",
        "config_path": "%USERPROFILE%/.liteyuki/configs/plugins/astrbot-hello.json"
      }
    },
    "permissions": ["config.read", "config.write", "adapter.reply"],
    "commands": [],
    "extra": {
      "sourceKind": "astrbot-compatible",
      "sourceManifest": "metadata.yaml"
    }
  }
}
```

## Adapter Families

The host should explicitly map source families to adapter strategies:

| Source family | Raw layout source | Host runtime strategy | Feasibility |
| --- | --- | --- | --- |
| `liteyukibot_plugin` | legacy Liteyuki Python modules with `__plugin_meta__` | direct Python bridge | High |
| `astrbot_plugin` | AstrBot `metadata.yaml` + compat decorators/APIs | Python bridge + AstrBot compat shim | High |
| `nonebot_plugin` | NoneBot plugin/module tree | dedicated NoneBot adapter or external sidecar | Medium, but not via current Python bridge alone |
| `native_plugin` | host-native manifest plugin | direct `plugin.json` | High |

## What Override Files Can Solve

Override files are good at solving:

- source-family classification
- host plugin id normalization
- display metadata normalization
- plugin root to Python module path mapping
- config file location overrides
- search path injection
- explicit lifecycle handler names
- declaring which adapter family should be used

Override files are **not** sufficient for:

- emulating NoneBot matcher semantics
- replacing NoneBot global driver/app lifecycle
- auto-translating arbitrary foreign runtime callbacks into host callbacks
- satisfying third-party plugin runtime dependencies without a real adapter runtime

## Discovery Flow Changes Needed

Current discovery only scans `plugin.json` in root and child directories:

- [src/plugin/loader.rs](/E:/repo/RsLiteyukiBot/src/plugin/loader.rs:32)

To support the proposed model, add a discovery pipeline before `PluginManifestLoader`:

1. scan `%USERPROFILE%/.liteyuki/plugins/manifests/*.override.json`
2. for each override:
   - validate source path stays inside the plugin root
   - load source-family-specific metadata readers
   - synthesize a normalized `PluginDescriptor`
3. pass the synthesized descriptor into the existing host planning/load path
4. keep direct `plugin.json` loading for native plugins and internal builtins

This should be implemented as a new layer rather than by making `PluginManifestLoader` understand every foreign ecosystem format.

Suggested module split:

- `src/plugin/source_adapter/mod.rs`
- `src/plugin/source_adapter/model.rs`
- `src/plugin/source_adapter/override_loader.rs`
- `src/plugin/source_adapter/family/astrbot.rs`
- `src/plugin/source_adapter/family/liteyuki_py.rs`
- `src/plugin/source_adapter/family/nonebot.rs`

## Config Compatibility Strategy

This is where the override model is most valuable.

### AstrBot

Map:

- `_conf_schema.json`
- generated config file path
- optional schema visibility metadata

Host behavior:

- preserve the original schema file in the raw source dir
- generate a host-owned resolved config path under `%USERPROFILE%/.liteyuki/configs/plugins/`
- surface the schema in a normalized backend API later

### Legacy Liteyuki

Map:

- source plugin name
- legacy YAML/JSON config location
- optional per-plugin config override path

Host behavior:

- do not require the plugin to adopt `plugin.json`
- allow override to declare `config_path`

### NoneBot

Map:

- displayed plugin metadata
- plugin source path
- optional host-side config stub

But do not overstate support:

- host config override does not remove the need for a real NoneBot runtime or sidecar

## Runtime Compatibility Strategy By Family

### `liteyukibot_plugin`

Use the current Python bridge.

The host already understands:

- Python entrypoints
- search paths
- lifecycle override names
- config path override

This family is the easiest candidate for manifest synthesis.

### `astrbot_plugin`

Use:

- manifest synthesis
- Python bridge
- AstrBot compat shim

This is consistent with the current repository direction and should be treated as the main foreign-ecosystem compatibility lane.

### `nonebot_plugin`

Do not pretend current compatibility exists.

Recommended choices:

1. `runtime.kind = external`
   - run a managed NoneBot subprocess / sidecar
2. or a dedicated `runtime.kind = python` adapter family that boots a NoneBot app inside a fenced compatibility runtime

Even in option 2, this is not the same adapter as the current Liteyuki/AstrBot bridge.

## Proposed Host Classification Fields

Current `/api/Plugin/List` already has `sourceKind` and `compatKind`, but they are derived from runtime behavior heuristics:

- [src/web/host/mod.rs](/E:/repo/RsLiteyukiBot/src/web/host/mod.rs:1193)

Under the new model, make them explicit and manifest-driven:

- `sourceFamily`
  - `native`
  - `liteyuki_py`
  - `astrbot`
  - `nonebot`
- `adapterFamily`
  - `native`
  - `liteyuki_python_bridge`
  - `astrbot_python_bridge`
  - `nonebot_external`
- `compatLevel`
  - `native`
  - `bridged`
  - `sidecar`
  - `metadata_only`

This will be more stable than inferring AstrBot-ness only from capability snapshots.

## Feasibility Assessment

### Feasible now

- user-local plugin root split by source family
- override mapping files
- host-owned common descriptor synthesis
- explicit source-family classification in backend payloads
- AstrBot and legacy Liteyuki migration onto synthesized host manifests

### Feasible later, but requires dedicated implementation

- AstrBot `_conf_schema.json` to host config schema normalization
- richer override validation and migration tooling
- generated override templates for imported plugins

### Not feasible under the “no source code changes, override only” assumption

- direct execution of arbitrary NoneBot plugins inside the current Liteyuki/AstrBot Python bridge
- truthfully claiming a shared runtime ABI across all foreign plugin ecosystems

## Recommended Phases

### Phase 1: Host Override Infrastructure

- add override schema
- add override discovery
- synthesize `PluginDescriptor`
- keep direct `plugin.json` loading untouched

### Phase 2: Liteyuki Python Source Adapter

- support legacy `__plugin_meta__`
- support explicit `config_path` override generation
- validate source roots and entrypoint resolution

### Phase 3: AstrBot Source Adapter

- parse `metadata.yaml`
- map `_conf_schema.json`
- tag plugin as `sourceFamily=astrbot`
- continue to use the existing compat runtime

### Phase 4: Plugin Center Classification

- expose explicit source-family and adapter-family payloads
- stop relying on heuristic-only `compatKind`

### Phase 5: NoneBot Decision Point

Choose one:

1. metadata-only support
2. managed external NoneBot runtime
3. dedicated embedded NoneBot adapter

Do not merge this into AstrBot/Liteyuki bridge work unless the runtime boundary is explicit.

## Recommended Final Direction

Adopt the proposal, but narrow the wording:

- do build a **common host plugin ABI**
- do use **source-family-specific raw directories**
- do use **override mapping files**
- do not describe this as a universal no-code-change runtime ABI

The best wording for the architecture is:

> a common host manifest plus source adapters

not:

> one runtime ABI that all plugin ecosystems can share unchanged

That framing matches the current codebase, the checked-in compatibility references, and the actual boundary between config normalization and runtime semantics.

## Practical Next Step

The next concrete implementation artifact should be a minimal override loader design with:

- override JSON schema
- source-family enum
- synthesized descriptor builder
- discovery order and conflict rules
- import/install behavior for category directories under `%USERPROFILE%/.liteyuki/plugins/`

That is the smallest useful step that moves the repository toward this model without overcommitting to impossible NoneBot parity.
