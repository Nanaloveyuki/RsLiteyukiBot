---
name: rust-llm-provider-url-consistency
description: Diagnose and repair Rust-side `/llm provider add/remove/use/list` consistency bugs where `llm.base_url` must stay aligned with `llm.provider_urls`. Use when Codex updates LLM provider URL command parsing, runtime handlers, config persistence, normalization, or regression tests in this repository.
---

# Rust Llm Provider Url Consistency

## Overview

Use this skill when a Rust LLM provider command can persist an invalid `base_url`, drift from `provider_urls`, or bypass normalization and persistence checks.

## Workflow

1. Map the full command path.

- Inspect parser and command enum definitions in `src/tui/app/commands.rs` and `src/tui/app.rs`.
- Inspect runtime handling and config writes in `src/main.rs`.
- Inspect config normalization and persistence helpers in `src/app_config.rs` and `src/config_edit.rs`.

2. Preserve the invariants.

- Treat `base_url` as the active selection and `provider_urls` as the allowed set.
- Normalize URLs with existing helpers before comparing or persisting them.
- Reject `use` or `remove` targets that are not present in `provider_urls`.
- Never write a new `base_url` on an error path.

3. Patch minimally.

- Prefer a shared helper such as `ensure_registered_provider_url(...)` when `remove` and `use` need the same membership check.
- Reuse existing error wording when possible, especially `not found` and `no provider base-url configured`.
- Keep parser-side validation lightweight and keep write-protection in the Rust runtime handler, where the actual persistence happens.

4. Add regression tests.

- Cover success: switching to a registered URL updates `base_url` and preserves `provider_urls`.
- Cover failure: an unknown URL returns an error and leaves the config file unchanged.
- If tests set `LY_LLM_CONFIG_PATH`, guard the env var and serialize those tests with a process-wide mutex.
- If command parsing changes, keep parser tests in `src/tui/app/tests.rs`, but do not rely on them alone for persistence bugs.

5. Validate before finishing.

- Run `cargo fmt`.
- Run focused tests first, for example `cargo test llm_provider_use_ -- --nocapture`.
- Expand to broader suites only if the patch touched shared config parsing or persistence code.

## Repo Hints

- Useful search terms: `UseProviderUrl`, `AddProviderUrl`, `RemoveProviderUrl`, `provider_urls`, `base_url`, `persist_llm_patch`.
- In this repo, the critical write path lives in `src/main.rs`; parser coverage alone will miss config corruption bugs.
- `src/app_config.rs` decides runtime fallback behavior, so check it before changing write semantics.

## Done Criteria

- Invalid `/llm provider use <base-url>` cannot corrupt config.
- `base_url` and `provider_urls` stay consistent after `add`, `remove`, and `use`.
- Tests assert both the returned result and the resulting file contents.
