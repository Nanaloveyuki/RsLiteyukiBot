# Memory Refresh Notes (2026-04-27)

This file is a workspace-local draft for manually refreshing `C:\Users\miaom\.codex\memories\MEMORY.md`.
It only covers items that were directly re-verified in the current checkout.

## Confirmed current-state corrections

- Do not treat `src/config_paths.rs` as the active config-path anchor anymore.
  - Current workspace state shows `src/config_paths.rs` is deleted.
  - Config-path constants now live in `src/hardcode_data/config_path.rs`.
  - Config-path resolution and migration helpers now live in `src/utils/config_path.rs`.

- Do not describe `src/hardcode_data/config_path.rs` as empty.
  - It currently contains real filename/path constants such as:
    - `APP_CONFIG_FILENAMES`
    - `LLM_CONFIG_FILENAMES`
    - `PASSWORD_CONFIG_FILENAME`
    - `WEBUI_PASSWORD_FILENAME`
    - `MCP_CONFIG_FILENAME`
    - `TOOL_STATE_FILENAME`
    - `PLUGIN_CRON_STATE_FILENAME`

- Update module-tree references for shared path/config utilities.
  - `src/lib.rs` now declares:
    - `pub(crate) mod hardcode_data;`
    - `pub(crate) mod utils;`
  - Memory entries that still point readers only to `src/config_paths.rs` should be rewritten to the new split:
    - constants/hardcoded names: `src/hardcode_data/config_path.rs`
    - reusable path logic: `src/utils/config_path.rs`

## Caution: in-flight areas

- `src/config_edit.rs` is currently in a dirty/in-flight state in this checkout.
  - `git status` shows it as modified.
  - Avoid writing memory that overstates its final structure until the current refactor settles.
  - Older memory entries that assume `src/config_edit.rs` is a stable single-file seam should be treated as provisional.

- `tests/app_config_migrated.rs` is also currently modified.
  - Memory should avoid claiming the current test wiring is final until this config/bootstrap refactor stabilizes.

## Suggested replacement wording

- Config path and migration anchor:
  - "The old `src/config_paths.rs` anchor has been split. Path/name constants now live in `src/hardcode_data/config_path.rs`, while reusable path resolution and migration helpers live in `src/utils/config_path.rs`."

- Hardcoded config filename guidance:
  - "Repo-wide config filename constants are intentionally centralized in `src/hardcode_data/config_path.rs`; reusable path derivation should call helpers from `src/utils/config_path.rs` instead of duplicating directory or migration logic."

- Refactor-state caution:
  - "When tasks mention `config_edit`, verify the current workspace structure first before relying on older single-file notes, because this area is under active refactor."
