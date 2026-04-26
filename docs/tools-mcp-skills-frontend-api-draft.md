# Tools / MCP / Skills Frontend API Draft

## Purpose

This document records the frontend-facing API surface that matches the current backend implementation state in `RsLiteyukiBot`.

Current status:

- Backend runtime support for `tools / mcp / skills` is implemented in the shared LLM path.
- Web management APIs are now implemented for tool, MCP, and skill discovery.
- Basic dashboard write APIs now exist for:
  - tool enable/disable
  - MCP save/test
  - skill read/upload
- Deeper lifecycle management such as MCP process hosting, skill delete/update, and richer policy controls is still future work.

## Implemented Backend Runtime

These capabilities are already wired into the backend runtime:

- Shared LLM runtime bundle construction in `src/llm/tools.rs`
- Repo-local skill scanning and inventory prompt injection in `src/llm/skills.rs`
- Streamable HTTP MCP loading and tool adaptation in `src/llm/mcp.rs`
- Shared `/ask` runtime integration in `src/llm/service.rs`
- Web OpenAI-compatible chat integration in `src/web/host/llm_api.rs`

Runtime-exposed local tools currently include:

- `workspace_list_files`
- `workspace_read_file`
- `read_skill_document`
- `list_tool_categories`
- `list_tools_in_category`
- `get_tool_schema`

## Implemented Web Routes

These routes now exist under the Web host API layer and return NapCat-style envelopes:

- `GET /api/tools`
- `POST /api/tools/toggle`
- `GET /api/mcp/servers`
- `POST /api/mcp/save`
- `POST /api/mcp/test`
- `GET /api/skills`
- `GET /api/skills/read`
- `POST /api/skills/upload`

### 1. Tools

`GET /api/tools`

Purpose:

- return the currently registered runtime tools
- include local and MCP-backed tools in one list

Current response shape:

```json
{
  "code": 0,
  "msg": "success",
  "data": {
    "tools": [
      {
        "name": "workspace_read_file",
        "description": "Read a UTF-8 workspace file with optional line slicing.",
        "parameters": {
          "type": "object"
        },
        "category": "workspace",
        "origin": "local",
        "whenToUse": "Use when you already know which repository file or SKILL.md you need to inspect.",
        "strict": true,
        "active": true
      }
    ],
    "warnings": []
  }
}
```

Notes:

- Includes local runtime tools, MCP-backed tools, and local discovery helpers in one list.
- `origin` is `local` or `mcp:<server_name>`.
- `active` indicates whether the tool is currently enabled in the shared runtime.

### 2. MCP

`GET /api/mcp/servers`

Purpose:

- return configured MCP servers and their current tool inventory / warning state

Current response shape:

```json
{
  "code": 0,
  "msg": "success",
  "data": {
    "servers": [
      {
        "name": "filesystem",
        "transport": "streamable_http",
        "url": "http://127.0.0.1:8787/mcp",
        "active": true,
        "toolCount": 3,
        "toolNames": ["read_file", "list_dir", "stat_path"],
        "warnings": []
      }
    ],
    "warnings": []
  }
}
```

Notes:

- Unsupported transports do not fail the route; they surface per-server warnings.
- Current backend support is limited to `streamable_http` / `http`.

### 3. Skills

`GET /api/skills`

Purpose:

- list repo-local skills discovered under the runtime skill root

Current response shape:

```json
{
  "code": 0,
  "msg": "success",
  "data": {
    "skills": [
      {
        "name": "rust-debugging",
        "description": "Workflow for narrowing Rust compile failures.",
        "path": "skills/rust-debugging/SKILL.md"
      }
    ],
    "warnings": []
  }
}
```

### 4. Tool Management

`POST /api/tools/toggle`

Purpose:

- enable or disable a runtime tool

Current request shape:

```json
{
  "name": "workspace_read_file",
  "active": false
}
```

Current response shape:

```json
{
  "code": 0,
  "msg": "success",
  "data": {
    "name": "workspace_read_file",
    "active": false,
    "configPath": "%USERPROFILE%/.liteyuki/configs/tool-state.json",
    "tools": [],
    "warnings": []
  }
}
```

Note:

- this route is now implemented
- tool activation state is persisted in `tool-state.json`
- the discovery helpers `list_tool_categories`, `list_tools_in_category`, and `get_tool_schema` should stay always enabled and should not expose a toggle in the UI

### 5. MCP Management

`POST /api/mcp/save`

Purpose:

- replace the current MCP server config file

Current request shape:

```json
{
  "servers": [
    {
      "name": "filesystem",
      "url": "http://127.0.0.1:8787/mcp",
      "transport": "streamable_http",
      "active": true,
      "headers": {}
    }
  ]
}
```

`POST /api/mcp/test`

Purpose:

- test one server config without requiring the dashboard to start a full chat

Current request shape:

```json
{
  "server": {
    "name": "filesystem",
    "url": "http://127.0.0.1:8787/mcp",
    "transport": "streamable_http",
    "active": true,
    "headers": {}
  }
}
```

Current response shape:

```json
{
  "code": 0,
  "msg": "success",
  "data": {
    "servers": [
      {
        "name": "filesystem",
        "transport": "streamable_http",
        "url": "http://127.0.0.1:8787/mcp",
        "active": true,
        "toolCount": 3,
        "toolNames": ["read_file", "list_dir", "stat_path"],
        "warnings": []
      }
    ],
    "warnings": []
  }
}
```

### 6. Skill Inspection

`GET /api/skills/read?name=<skill_name>`

Purpose:

- read a single `SKILL.md` in a dashboard inspector

Current response shape:

```json
{
  "code": 0,
  "msg": "success",
  "data": {
    "name": "rust-debugging",
    "description": "Workflow for narrowing Rust compile failures.",
    "path": "skills/rust-debugging/SKILL.md",
    "content": "# Skill...",
    "truncated": false
  }
}
```

`POST /api/skills/upload`

Purpose:

- create or overwrite a repo-local `SKILL.md` entry

Current request shape:

```json
{
  "name": "uploaded-skill",
  "content": "---\ndescription: Uploaded skill\n---\n# Uploaded",
  "overwrite": false
}
```

## Backend Data Rules

The future frontend should assume these backend rules:

- Workspace file tools are restricted to the resolved workspace root.
- MCP tools are namespaced as `mcp__<server>__<tool>`.
- Skill inventory failure should not block normal LLM chat.
- MCP config failure should surface as warnings, not silent disappearance.
- Only providers on the shared OpenAI-style runtime currently support tool execution.
- Anthropic and Gemini should be treated as text-generation providers unless tool support is added explicitly later.

## Remaining Gaps

The remaining backend gaps for this area are no longer the basic CRUD-style inspector APIs.
What is still missing is the next layer of runtime management:

- MCP stdio / child-process hosting and supervision
- skill update / delete / rename flows
- more granular per-tool policy or scope control beyond a simple enabled flag
- UI-oriented audit history for tool and MCP state changes

## Non-Goals For The Next Frontend Pass

These should stay out of the next UI iteration unless backend requirements change:

- skill upload marketplace flows
- sandbox skill sync
- MCP stdio process management UI
- per-tool schema editors
- provider-specific tool capability matrices in the dashboard
