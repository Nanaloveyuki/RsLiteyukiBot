# Docs Index

## Purpose

This directory now separates current-state documents from historical plan/design material.

If you are trying to understand what the repository already does today, start from the current-state section.

## Current-State Documents

- `plugin-runtime-current-state.md`
  - current plugin runtime, capability APIs, execution surfaces, and remaining backend gaps
- `tools-mcp-skills-web-api.md`
  - current Web Host routes for tools, MCP, and skills
- `frontend-backend-adaptation-requirements.md`
  - next frontend work based on the backend surfaces that already exist
- `python-bridge-current-status-and-doc-audit.md`
  - Python bridge current state plus doc reading order
- `python-bridge-compatibility-notes.md`
  - practical compatibility notes for the Python/AstrBot bridge
- `python-bridge-risk-register.md`
  - bridge risks and support-state caveats

## Plan / Design / History Documents

- `python-bridge-refactor-plan.md`
  - historical refactor phases and original bridge direction
- `astrbot-tools-mcp-skills-action-plan.md`
  - historical action plan for the broader tools/MCP/skills lane
- `astrbot-tools-mcp-skills-implementation.md`
  - external/reference-style implementation notes
- `progressive-tool-disclosure-design.md`
  - design notes for gradual tool/skill disclosure
- `common-plugin-abi-source-adapter-design.md`
  - source-family and common-ABI design for plugin ingestion

## Recommended Reading Order

For the current plugin/runtime lane:

1. `plugin-runtime-current-state.md`
2. `python-bridge-current-status-and-doc-audit.md`
3. `tools-mcp-skills-web-api.md`
4. `frontend-backend-adaptation-requirements.md`

Use the plan/design files only when you want the earlier implementation direction or future-oriented design discussion.
