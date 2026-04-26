import base64
import contextvars
import enum
import inspect
import logging
import re
import shlex
import sys
import types
import typing
from dataclasses import dataclass, field


def _is_awaitable(value):
    return hasattr(value, "__await__")


def _normalize_prefixes(prefixes):
    if isinstance(prefixes, str):
        return [prefixes]
    if isinstance(prefixes, (list, tuple, set)):
        out = []
        for item in prefixes:
            text = str(item).strip()
            if text:
                out.append(text)
        return out
    return []


def _extract_message_text(payload):
    if not isinstance(payload, dict):
        return ""
    for key in ("raw_message", "text"):
        value = payload.get(key)
        if isinstance(value, str):
            return value
    message = payload.get("message")
    if isinstance(message, str):
        return message
    if isinstance(message, list):
        parts = []
        for segment in message:
            if isinstance(segment, str):
                parts.append(segment)
                continue
            if not isinstance(segment, dict):
                continue
            data = segment.get("data")
            if isinstance(data, dict):
                text = data.get("text")
                if isinstance(text, str):
                    parts.append(text)
                    continue
            text = segment.get("text")
            if isinstance(text, str):
                parts.append(text)
        return "".join(parts)
    return ""


def _is_message_payload(payload):
    if not isinstance(payload, dict):
        return False
    if str(payload.get("post_type", "") or "").lower() == "message":
        return True
    if str(payload.get("message_type", "") or "").strip():
        return True
    return any(key in payload for key in ("raw_message", "message", "text"))


def _coerce_text_output(value):
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, MessageEventResult):
        if value.chain is None:
            return ""
        return "".join(str(item) for item in value.chain)
    if isinstance(value, (list, tuple, set)):
        return "".join(str(item) for item in value)
    return str(value)


def _normalize_command_prefix(prefix):
    prefix = str(prefix or "").strip()
    if not prefix:
        return ""
    return prefix


def _matches_command(message, prefixes):
    return _match_command_prefix(message, prefixes) is not None


def _match_command_prefix(message, prefixes):
    message = str(message or "").strip()
    for prefix in prefixes:
        normalized = _normalize_command_prefix(prefix)
        if not normalized:
            continue
        choices = [normalized]
        if not normalized.startswith("/"):
            choices.append("/" + normalized)
        for choice in choices:
            if message == choice or message.startswith(choice + " "):
                return choice
    return None


def _append_handler_meta(target, meta):
    metas = list(getattr(target, "__astrbot_handler_meta__", []))
    if metas and _can_merge_handler_meta(metas[-1], meta):
        merged = dict(metas[-1])
        for key, value in meta.items():
            if key == "alias":
                merged["alias"] = _combine_aliases(merged.get("alias"), value)
            else:
                merged[key] = value
        metas[-1] = merged
    else:
        metas.append(dict(meta))
    target.__astrbot_handler_meta__ = metas
    return target


_astrbot_star_classes = {}
_astrbot_module_runtimes = {}
_astrbot_current_web_request = contextvars.ContextVar(
    "liteyuki_astrbot_current_web_request", default=None
)


def _coerce_web_value(value, default=None, value_type=None):
    if value is None:
        return default
    if value_type is None:
        return value
    try:
        return value_type(value)
    except Exception:
        return default


class _CompatAwaitableValue:
    def __init__(self, value):
        self._value = value

    async def _resolve(self):
        return self._value

    def __await__(self):
        return self._resolve().__await__()


class _CompatQueryDict(dict):
    def get(self, key, default=None, type=None):
        return _coerce_web_value(super().get(key, default), default, type)


class _CompatHeaders(dict):
    def _find_key(self, key):
        target = str(key or "").lower()
        for existing in self.keys():
            if str(existing).lower() == target:
                return existing
        return None

    def get(self, key, default=None, type=None):
        existing = self._find_key(key)
        if existing is None:
            return default
        return _coerce_web_value(super().get(existing, default), default, type)

    def __contains__(self, key):
        return self._find_key(key) is not None

    def __getitem__(self, key):
        existing = self._find_key(key)
        if existing is None:
            raise KeyError(key)
        return super().__getitem__(existing)


class CompatWebApiResponse(dict):
    def __init__(self, body=None, status=200, content_type=None):
        super().__init__(status=int(status or 200), body=body)
        if content_type:
            self["contentType"] = str(content_type)

    @property
    def status_code(self):
        return int(self.get("status", 200) or 200)

    @status_code.setter
    def status_code(self, value):
        self["status"] = int(value or 200)

    @property
    def content_type(self):
        return self.get("contentType")

    @content_type.setter
    def content_type(self, value):
        if value is None:
            self.pop("contentType", None)
            return
        self["contentType"] = str(value)


class CompatWebRequest(dict):
    def __init__(self, payload=None):
        super().__init__(dict(payload or {}))
        self["query"] = _CompatQueryDict(dict(self.get("query") or {}))
        self["headers"] = _CompatHeaders(dict(self.get("headers") or {}))

    @property
    def args(self):
        return self["query"]

    @property
    def headers(self):
        return self["headers"]

    @property
    def method(self):
        return str(self.get("method") or "").upper()

    @property
    def path(self):
        return str(self.get("path") or "")

    @property
    def remote_addr(self):
        return self.get("peerIp")

    @property
    def json(self):
        return _CompatAwaitableValue(self.get("bodyJson"))

    def _body_bytes(self):
        encoded = self.get("bodyBytesBase64")
        if not encoded:
            return b""
        try:
            return base64.b64decode(encoded)
        except Exception:
            return b""

    @property
    def data(self):
        return _CompatAwaitableValue(self._body_bytes())

    async def get_json(self, force=False, silent=False, cache=True):
        payload = self.get("bodyJson")
        if payload is not None:
            return payload
        if silent:
            return None
        raise ValueError("request body is not valid json")

    async def get_data(self, cache=True, as_text=False, parse_form_data=False):
        payload = self._body_bytes()
        if as_text:
            return payload.decode("utf-8", errors="replace")
        return payload


class _CompatRequestProxy:
    def _current(self):
        request = _astrbot_current_web_request.get()
        return request

    def __getattr__(self, name):
        if name.startswith("__") and name.endswith("__"):
            raise AttributeError(name)
        request = self._current()
        if request is None:
            raise AttributeError(name)
        return getattr(request, name)

    def __getitem__(self, key):
        request = self._current()
        if request is None:
            raise RuntimeError("no active AstrBot web request context")
        return request[key]

    def get(self, key, default=None):
        request = self._current()
        if request is None:
            return default
        return request.get(key, default)

    def items(self):
        request = self._current()
        if request is None:
            return {}.items()
        return request.items()

    def keys(self):
        request = self._current()
        if request is None:
            return {}.keys()
        return request.keys()

    def values(self):
        request = self._current()
        if request is None:
            return {}.values()
        return request.values()

    def __contains__(self, key):
        request = self._current()
        if request is None:
            return False
        return key in request

    def __iter__(self):
        request = self._current()
        if request is None:
            return iter(())
        return iter(request)


def _build_astrbot_web_request_context(payload):
    return CompatWebRequest(payload)


def _call_astrbot_web_handler(handler, request):
    accepts_request = True
    try:
        signature = inspect.signature(handler)
    except (TypeError, ValueError):
        signature = None
    if signature is not None:
        accepts_request = any(
            parameter.kind
            in (
                inspect.Parameter.POSITIONAL_ONLY,
                inspect.Parameter.POSITIONAL_OR_KEYWORD,
                inspect.Parameter.VAR_POSITIONAL,
            )
            for parameter in signature.parameters.values()
        )

    if accepts_request:
        return handler(request)
    return handler()


async def _invoke_astrbot_web_handler(handler, payload):
    request = _build_astrbot_web_request_context(payload)
    token = _astrbot_current_web_request.set(request)
    try:
        result = _call_astrbot_web_handler(handler, request)
        if _is_awaitable(result):
            return await result
        return result
    finally:
        _astrbot_current_web_request.reset(token)


def _set_astrbot_web_request_context(request):
    return _astrbot_current_web_request.set(
        request if isinstance(request, CompatWebRequest) else CompatWebRequest(request)
    )


def _reset_astrbot_web_request_context(token):
    _astrbot_current_web_request.reset(token)
    return True


def jsonify(*args, **kwargs):
    if args and kwargs:
        body = {"args": list(args), "kwargs": kwargs}
    elif kwargs:
        body = kwargs
    elif len(args) == 1:
        body = args[0]
    elif args:
        body = list(args)
    else:
        body = None
    return CompatWebApiResponse(
        body=body,
        status=200,
        content_type="application/json; charset=utf-8",
    )


def make_response(response=None, status=None, headers=None):
    if isinstance(response, CompatWebApiResponse):
        payload = CompatWebApiResponse(
            body=response.get("body"),
            status=response.get("status", 200),
            content_type=response.get("contentType"),
        )
    else:
        payload = CompatWebApiResponse(body=response, status=200)
    if status is not None:
        payload.status_code = status
    if headers:
        content_type = headers.get("Content-Type") or headers.get("content-type")
        if content_type:
            payload.content_type = content_type
    return payload


def _combine_aliases(existing, incoming):
    merged = []
    for source in (existing, incoming):
        if source is None:
            continue
        if isinstance(source, str):
            values = [source]
        else:
            values = list(source)
        for item in values:
            text = str(item or "").strip()
            if text and text not in merged:
                merged.append(text)
    return merged


def _can_merge_handler_meta(existing, incoming):
    return (
        existing.get("event") == "adapter_message"
        and incoming.get("event") == "adapter_message"
    )


class PluginType:
    APPLICATION = "application"
    SERVICE = "service"
    MODULE = "module"
    UNCLASSIFIED = "unclassified"
    TEST = "test"


@dataclass
class PluginMetadata:
    name: str
    description: str = ""
    usage: str = ""
    type: str = PluginType.UNCLASSIFIED
    author: str = ""
    homepage: str = ""
    extra: dict = field(default_factory=dict)


class MessageEvent:
    def __init__(self, event, sdk=None):
        self._event = event if isinstance(event, dict) else {}
        self._payload = self._event.get("payload", {})
        if not isinstance(self._payload, dict):
            self._payload = {}
        self._sdk = sdk
        self.raw_message = _extract_message_text(self._payload)

    @property
    def payload(self):
        return self._payload

    def reply(self, message):
        if self._sdk is None:
            return False
        text = str(message).strip()
        if not text:
            return False
        if hasattr(self._sdk, "reply_text"):
            try:
                return bool(self._sdk.reply_text(self._event, text))
            except Exception:
                return False
        if hasattr(self._sdk, "log"):
            self._sdk.log(text)
        return False


async def _dispatch_legacy_handlers(event, sdk, module_globals):
    handlers = module_globals.get("__liteyuki_legacy_handlers__", [])
    message_event = MessageEvent(event, sdk)
    for item in list(handlers):
        handler = item.get("handler")
        kind = item.get("kind")
        prefixes = item.get("prefixes", [])
        rule = item.get("rule")
        if not callable(handler):
            continue
        if kind == "message" and not _is_message_payload(message_event.payload):
            continue
        if prefixes and not any(_legacy_startswith(message_event.raw_message, prefix) for prefix in prefixes):
            continue
        if callable(rule):
            try:
                allowed = rule(message_event)
                if _is_awaitable(allowed):
                    allowed = await allowed
                if not bool(allowed):
                    continue
            except Exception:
                continue
        result = handler(message_event)
        if _is_awaitable(result):
            await result


def _ensure_legacy_dispatcher(module_globals):
    if "liteyuki_handle_event" in module_globals:
        return

    async def _compat_dispatch(event, sdk=None):
        await _dispatch_legacy_handlers(event, sdk, module_globals)

    module_globals["liteyuki_handle_event"] = _compat_dispatch


class _OnStartswith:
    def __init__(self, prefixes, rule=None):
        self._prefixes = _normalize_prefixes(prefixes)
        self._rule = rule

    def handle(self):
        def decorator(func):
            module_globals = getattr(func, "__globals__", {})
            handlers = module_globals.setdefault("__liteyuki_legacy_handlers__", [])
            handlers.append(
                {
                    "kind": "startswith",
                    "prefixes": self._prefixes,
                    "rule": self._rule,
                    "handler": func,
                }
            )
            _ensure_legacy_dispatcher(module_globals)
            return func

        return decorator


def on_startswith(prefixes, rule=None):
    return _OnStartswith(prefixes, rule=rule)


class _OnMessage:
    def __init__(self, rule=None):
        self._rule = rule

    def handle(self):
        def decorator(func):
            module_globals = getattr(func, "__globals__", {})
            handlers = module_globals.setdefault("__liteyuki_legacy_handlers__", [])
            handlers.append(
                {
                    "kind": "message",
                    "prefixes": [],
                    "rule": self._rule,
                    "handler": func,
                }
            )
            _ensure_legacy_dispatcher(module_globals)
            return func

        return decorator


def on_message(rule=None):
    return _OnMessage(rule=rule)


def _legacy_startswith(raw_message, prefix):
    raw_message = str(raw_message or "")
    prefix = str(prefix or "").strip()
    if not prefix:
        return False
    if raw_message.startswith(prefix):
        return True
    if prefix.startswith("/"):
        return False
    return raw_message.startswith("/" + prefix)


def is_su_rule(event):
    return True


class EventResultType(enum.Enum):
    CONTINUE = "continue"
    STOP = "stop"


class MessageChain(list):
    pass


class CommandResult:
    pass


class MessageEventResult:
    def __init__(self):
        self.chain = []
        self.result_type = EventResultType.CONTINUE

    def message(self, text):
        self.chain.append(str(text))
        return self

    def stop_event(self):
        self.result_type = EventResultType.STOP
        return self

    def continue_event(self):
        self.result_type = EventResultType.CONTINUE
        return self

    def is_stopped(self):
        return self.result_type == EventResultType.STOP


class MessageType(enum.Enum):
    GROUP_MESSAGE = "group"
    FRIEND_MESSAGE = "private"
    OTHER_MESSAGE = "other"


class EventMessageType(enum.IntFlag):
    GROUP_MESSAGE = 1
    PRIVATE_MESSAGE = 2
    OTHER_MESSAGE = 4
    ALL = GROUP_MESSAGE | PRIVATE_MESSAGE | OTHER_MESSAGE


class EventMessageTypeFilter:
    def __init__(self, event_message_type):
        self.event_message_type = event_message_type


class PermissionType(enum.Enum):
    ANY = "any"
    ADMIN = "admin"


class PermissionTypeFilter:
    def __init__(self, permission_type, raise_error=True):
        self.permission_type = permission_type
        self.raise_error = raise_error


class PlatformAdapterType(enum.Enum):
    ANY = "any"
    ALL = "all"
    ONEBOT = "onebot"


class PlatformAdapterTypeFilter:
    def __init__(self, platform_adapter_type):
        self.platform_adapter_type = platform_adapter_type


class RegexFilter:
    def __init__(self, regex):
        self.regex = regex


class CustomFilter:
    def __init__(self, raise_error=True):
        self.raise_error = raise_error


class FunctionTool:
    def __init__(
        self,
        name,
        description="",
        parameters=None,
        handler=None,
        handler_module_path=None,
        active=True,
        is_background_task=False,
    ):
        self.name = str(name or "").strip()
        self.description = str(description or "").strip()
        self.parameters = parameters or {"type": "object", "properties": {}}
        self.handler = handler
        self.handler_module_path = handler_module_path
        self.active = bool(active)
        self.is_background_task = bool(is_background_task)
        self.source = ""
        self._handler_name = getattr(handler, "__name__", None)

    async def call(self, context=None, **kwargs):
        if self.handler is None:
            raise NotImplementedError(
                "FunctionTool.call() requires a handler in compatibility mode"
            )
        result = self.handler(**kwargs)
        if _is_awaitable(result):
            result = await result
        return result

    def __repr__(self):
        return (
            f"FunctionTool(name={self.name!r}, "
            f"handler_module_path={self.handler_module_path!r})"
        )


class ToolSet:
    def __init__(self, tools=None):
        self.tools = list(tools or [])

    def empty(self):
        return len(self.tools) == 0

    def add_tool(self, tool):
        for index, existing in enumerate(self.tools):
            if existing.name != tool.name:
                continue
            existing_active = bool(getattr(existing, "active", True))
            new_active = bool(getattr(tool, "active", True))
            if new_active or not existing_active:
                self.tools[index] = tool
            return
        self.tools.append(tool)

    def remove_tool(self, name):
        self.tools = [tool for tool in self.tools if tool.name != name]

    def get_tool(self, name):
        for tool in self.tools:
            if tool.name == name:
                return tool
        return None

    def get_func(self, name):
        return self.get_tool(name)

    @property
    def func_list(self):
        return self.tools

    def names(self):
        return [tool.name for tool in self.tools]

    def merge(self, other):
        for tool in getattr(other, "tools", []):
            self.add_tool(tool)

    def __len__(self):
        return len(self.tools)

    def __bool__(self):
        return bool(self.tools)

    def __iter__(self):
        return iter(self.tools)

    def __repr__(self):
        return f"ToolSet(tools={self.tools!r})"


class BaseFunctionToolExecutor:
    pass


class CompatToolManager:
    def __init__(self):
        self._tool_set = ToolSet()

    @property
    def func_list(self):
        return self._tool_set.tools

    def add_tool(self, tool):
        self._tool_set.add_tool(tool)
        return tool

    def remove_tool(self, name):
        self._tool_set.remove_tool(name)

    def get_tool(self, name):
        return self._tool_set.get_tool(name)

    def get_func(self, name):
        return self.get_tool(name)

    def add_func(self, name, func_args, desc, handler):
        return self.add_tool(self.spec_to_func(name, func_args, desc, handler))

    def remove_func(self, name):
        self.remove_tool(name)

    def spec_to_func(self, name, func_args, desc, handler):
        properties = {}
        required = []
        for item in list(func_args or []):
            if not isinstance(item, dict):
                continue
            arg_name = str(item.get("name", "") or "").strip()
            if not arg_name:
                continue
            schema = {
                "type": str(item.get("type", "string") or "string"),
                "description": str(item.get("description", "") or ""),
            }
            if isinstance(item.get("items"), dict):
                schema["items"] = dict(item.get("items"))
            properties[arg_name] = schema
            required.append(arg_name)
        parameters = {"type": "object", "properties": properties}
        if required:
            parameters["required"] = required
        return FunctionTool(
            name=name,
            description=desc,
            parameters=parameters,
            handler=handler,
            handler_module_path=getattr(handler, "__module__", None),
        )

    def activate_llm_tool(self, name, *_args, **_kwargs):
        tool = self.get_tool(name)
        if tool is None:
            return False
        tool.active = True
        return True

    def deactivate_llm_tool(self, name):
        tool = self.get_tool(name)
        if tool is None:
            return False
        tool.active = False
        return True

    def get_builtin_tool(self, tool_type):
        if isinstance(tool_type, FunctionTool):
            return tool_type
        return tool_type()


_astrbot_global_tool_manager = CompatToolManager()


def _normalize_astrbot_module_name(module_or_name):
    if isinstance(module_or_name, types.ModuleType):
        return getattr(module_or_name, "__name__", "").strip()
    return str(module_or_name or "").strip()


def _attach_astrbot_runtime(module, runtime):
    module.__astrbot_runtime__ = runtime
    module.__astrbot_llm_tools__ = runtime["llm_tools"]
    module.__astrbot_scheduled_tasks__ = runtime["scheduled_tasks"]
    module.__astrbot_cron_jobs__ = runtime["cron_jobs"]
    module.__astrbot_registered_web_apis__ = runtime["registered_web_apis"]
    module.__astrbot_agents__ = runtime["agents"]
    return runtime


class CompatCronJob:
    def __init__(
        self,
        job_id,
        job_type,
        name,
        description="",
        cron_expression=None,
        payload=None,
        enabled=True,
        run_once=False,
        timezone=None,
        persistent=True,
        handler=None,
    ):
        self.job_id = str(job_id or "").strip()
        self.job_type = str(job_type or "").strip()
        self.name = str(name or "").strip()
        self.description = str(description or "").strip()
        self.cron_expression = cron_expression
        self.payload = dict(payload or {})
        self.enabled = bool(enabled)
        self.run_once = bool(run_once)
        self.timezone = timezone
        self.persistent = bool(persistent)
        self.handler = handler
        self.next_run_time = None
        self.last_run_at = None
        self.last_error = None
        self.status = "idle"

    def model_dump(self):
        return dict(self.__dict__)


class CompatCronManager:
    def __init__(self, module_name):
        self._module_name = _normalize_astrbot_module_name(module_name)

    def _runtime(self):
        return _ensure_astrbot_module_runtime(self._module_name)

    def _next_job_id(self, job_type):
        runtime = self._runtime()
        prefix = f"{self._module_name}:{job_type}:"
        current = sum(
            1
            for job in runtime["cron_jobs"]
            if str(getattr(job, "job_id", "")).startswith(prefix)
        )
        return f"{prefix}{current + 1}"

    async def add_basic_job(
        self,
        name,
        handler,
        cron_expression,
        description="",
        payload=None,
        enabled=True,
        timezone=None,
        persistent=True,
    ):
        job = CompatCronJob(
            job_id=self._next_job_id("basic"),
            job_type="basic",
            name=name,
            description=description,
            cron_expression=cron_expression,
            payload=payload,
            enabled=enabled,
            timezone=timezone,
            persistent=persistent,
            handler=handler,
        )
        self._runtime()["cron_jobs"].append(job)
        return job

    async def add_active_job(
        self,
        name,
        description="",
        cron_expression=None,
        payload=None,
        enabled=True,
        run_once=False,
        timezone=None,
        persistent=True,
    ):
        job = CompatCronJob(
            job_id=self._next_job_id("active_agent"),
            job_type="active_agent",
            name=name,
            description=description,
            cron_expression=cron_expression,
            payload=payload,
            enabled=enabled,
            run_once=run_once,
            timezone=timezone,
            persistent=persistent,
        )
        self._runtime()["cron_jobs"].append(job)
        return job

    async def update_job(self, job_id, **kwargs):
        for job in self._runtime()["cron_jobs"]:
            if job.job_id != job_id:
                continue
            for key, value in kwargs.items():
                if hasattr(job, key):
                    setattr(job, key, value)
            return job
        return None

    async def delete_job(self, job_id):
        runtime = self._runtime()
        runtime["cron_jobs"] = [
            job for job in runtime["cron_jobs"] if job.job_id != job_id
        ]
        module = sys.modules.get(self._module_name)
        if module is not None:
            _attach_astrbot_runtime(module, runtime)

    async def list_jobs(self, job_type=None):
        jobs = list(self._runtime()["cron_jobs"])
        if job_type is None:
            return jobs
        return [job for job in jobs if job.job_type == job_type]


def _ensure_astrbot_module_runtime(module_or_name):
    module_name = _normalize_astrbot_module_name(module_or_name)
    runtime = _astrbot_module_runtimes.get(module_name)
    if runtime is None:
        runtime = {
            "module_name": module_name,
            "llm_tools": [],
            "scheduled_tasks": [],
            "cron_jobs": [],
            "registered_web_apis": [],
            "agents": [],
            "tool_manager": _astrbot_global_tool_manager,
            "cron_manager": CompatCronManager(module_name),
        }
        _astrbot_module_runtimes[module_name] = runtime
    module = sys.modules.get(module_name)
    if module is not None:
        _attach_astrbot_runtime(module, runtime)
    return runtime


def _get_astrbot_plugin_runtime(module_or_name):
    return _ensure_astrbot_module_runtime(module_or_name)


def _register_module_tool(module_name, tool):
    runtime = _ensure_astrbot_module_runtime(module_name)
    tool.handler_module_path = tool.handler_module_path or runtime["module_name"]
    _astrbot_global_tool_manager.add_tool(tool)
    for index, existing in enumerate(runtime["llm_tools"]):
        if existing.name == tool.name:
            runtime["llm_tools"][index] = tool
            break
    else:
        runtime["llm_tools"].append(tool)
    return tool


def _cleanup_astrbot_plugin_runtime(module_or_name):
    module_name = _normalize_astrbot_module_name(module_or_name)
    runtime = _astrbot_module_runtimes.pop(module_name, None)
    if runtime is not None:
        for tool in list(runtime.get("llm_tools", [])):
            _astrbot_global_tool_manager.remove_tool(tool.name)
    _astrbot_star_classes.pop(module_name, None)
    module = sys.modules.get(module_name)
    if module is not None:
        for attr in (
            "__astrbot_runtime__",
            "__astrbot_llm_tools__",
            "__astrbot_scheduled_tasks__",
            "__astrbot_cron_jobs__",
            "__astrbot_registered_web_apis__",
            "__astrbot_agents__",
            "__astrbot_context__",
        ):
            if hasattr(module, attr):
                delattr(module, attr)
    return True


def _stringify_task_id(task):
    if isinstance(task, str):
        return task
    name = getattr(task, "__name__", None)
    if isinstance(name, str) and name.strip():
        return name.strip()
    display_name = getattr(task, "name", None)
    if isinstance(display_name, str) and display_name.strip():
        return display_name.strip()
    return repr(task)


def _task_kind(task):
    if task is None:
        return "unknown"
    if isinstance(task, str):
        return "string"
    return type(task).__name__


def _normalize_web_api_route(route):
    value = str(route or "").strip()
    if not value:
        return "/"
    return "/" + value.strip("/")


def _normalize_web_api_methods(methods):
    normalized = []
    for method in list(methods or ["GET"]):
        value = str(method or "").strip().upper()
        if value and value not in normalized:
            normalized.append(value)
    return normalized or ["GET"]


def _snapshot_function_tool(tool):
    return {
        "name": str(getattr(tool, "name", "") or "").strip(),
        "description": str(getattr(tool, "description", "") or "").strip(),
        "parameters": getattr(tool, "parameters", {}) or {},
        "active": bool(getattr(tool, "active", True)),
        "source": str(getattr(tool, "source", "") or "unknown").strip() or "unknown",
        "handlerModulePath": (
            str(getattr(tool, "handler_module_path", "") or "").strip() or None
        ),
    }


def _snapshot_web_api(item):
    route, view_handler, methods, desc = item
    handler_module = getattr(view_handler, "__module__", None)
    return {
        "route": _normalize_web_api_route(route),
        "methods": _normalize_web_api_methods(methods),
        "description": str(desc or "").strip(),
        "source": "astrbot_context",
        "runtimeKind": "python",
        "handlerModulePath": str(handler_module or "").strip() or None,
    }


def _snapshot_cron_job(job):
    return {
        "jobId": str(getattr(job, "job_id", "") or "").strip(),
        "jobType": str(getattr(job, "job_type", "") or "").strip(),
        "name": str(getattr(job, "name", "") or "").strip(),
        "description": str(getattr(job, "description", "") or "").strip(),
        "cronExpression": getattr(job, "cron_expression", None),
        "runOnce": bool(getattr(job, "run_once", False)),
        "enabled": bool(getattr(job, "enabled", True)),
        "timezone": getattr(job, "timezone", None),
        "persistent": bool(getattr(job, "persistent", True)),
        "payload": getattr(job, "payload", {}) or {},
        "nextRunTime": getattr(job, "next_run_time", None),
        "lastRunTime": getattr(job, "last_run_at", None),
        "lastError": getattr(job, "last_error", None),
    }


def _snapshot_task(entry):
    task = entry.get("task")
    return {
        "taskId": _stringify_task_id(task),
        "description": str(entry.get("desc", "") or "").strip(),
        "taskKind": _task_kind(task),
        "source": "astrbot_context",
    }


def _snapshot_astrbot_plugin_runtime(module_or_name):
    module_name = _normalize_astrbot_module_name(module_or_name)
    runtime = _astrbot_module_runtimes.get(module_name)
    if runtime is None:
        return None

    return {
        "moduleName": runtime["module_name"],
        "tools": [_snapshot_function_tool(tool) for tool in list(runtime.get("llm_tools", []))],
        "webApis": [
            _snapshot_web_api(item)
            for item in list(runtime.get("registered_web_apis", []))
        ],
        "cronJobs": [
            _snapshot_cron_job(job) for job in list(runtime.get("cron_jobs", []))
        ],
        "tasks": [_snapshot_task(task) for task in list(runtime.get("scheduled_tasks", []))],
        "agentCount": len(list(runtime.get("agents", []))),
    }


def _tool_schema_type_from_annotation(annotation):
    if annotation in (inspect.Parameter.empty, None):
        return "string"

    origin = typing.get_origin(annotation)
    if origin in (typing.Union, types.UnionType):
        args = [arg for arg in typing.get_args(annotation) if arg is not type(None)]
        if not args:
            return "string"
        return _tool_schema_type_from_annotation(args[0])

    if annotation is str:
        return "string"
    if annotation is bool:
        return "boolean"
    if annotation is int:
        return "integer"
    if annotation is float:
        return "number"
    if annotation in (dict,):
        return "object"
    if annotation in (list, tuple, set):
        return "array"
    if origin in (list, tuple, set):
        return "array"
    if origin is dict:
        return "object"
    return "string"


def _extract_tool_description(awaitable):
    doc = inspect.getdoc(awaitable) or ""
    if not doc:
        return ""
    return doc.split("\n\n", 1)[0].strip()


def _build_tool_parameters(awaitable):
    properties = {}
    required = []
    try:
        parameters = list(inspect.signature(awaitable).parameters.values())
    except (TypeError, ValueError):
        return {"type": "object", "properties": {}}

    for parameter in parameters:
        if parameter.name in {
            "self",
            "event",
            "context",
            "request",
            "response",
            "run_context",
            "tool",
            "tool_args",
            "tool_result",
        }:
            continue
        schema = {
            "type": _tool_schema_type_from_annotation(parameter.annotation),
        }
        properties[parameter.name] = schema
        if parameter.default is inspect.Parameter.empty:
            required.append(parameter.name)

    payload = {"type": "object", "properties": properties}
    if required:
        payload["required"] = required
    return payload


def _build_function_tool(awaitable, name=None):
    tool_name = str(name or getattr(awaitable, "__name__", "") or "").strip()
    tool = FunctionTool(
        name=tool_name,
        description=_extract_tool_description(awaitable),
        parameters=_build_tool_parameters(awaitable),
        handler=awaitable,
        handler_module_path=getattr(awaitable, "__module__", None),
    )
    tool.source = "astrbot_decorator"
    return tool


def _bind_module_tools(module, instance):
    runtime = _ensure_astrbot_module_runtime(module)
    for tool in runtime["llm_tools"]:
        handler_name = getattr(tool, "_handler_name", None)
        if not handler_name or not hasattr(instance, handler_name):
            continue
        tool.handler = getattr(instance, handler_name)
        tool.handler_module_path = getattr(module, "__name__", tool.handler_module_path)


class AstrBotConfig(dict):
    pass


class Provider:
    pass


class ProviderMetaData:
    pass


class Personality:
    pass


class Platform:
    pass


class AstrBotMessage(dict):
    pass


class MessageMember(dict):
    pass


class PlatformMetadata:
    def __init__(self, name="", platform_id=""):
        self.name = name
        self.id = platform_id


class Context:
    def __init__(self, sdk, config=None, module_name=""):
        self._sdk = sdk
        self._config = config or {}
        self._module_name = _normalize_astrbot_module_name(module_name)
        self._runtime = _ensure_astrbot_module_runtime(self._module_name)
        self.registered_web_apis = self._runtime["registered_web_apis"]
        self._register_tasks = self._runtime["scheduled_tasks"]
        self.cron_manager = self._runtime["cron_manager"]

    def get_config(self, umo=None):
        return self._config

    def config_get(self, key="", default=None):
        try:
            value = self._sdk.config_get(key)
        except Exception:
            return default
        if value is None:
            return default
        return value

    def config_set(self, key, value):
        return self._sdk.config_set(key, value)

    def config_delete(self, key):
        return self._sdk.config_delete(key)

    def get_plugin_id(self):
        return getattr(self._sdk, "plugin_id", "")

    def get_llm_tool_manager(self):
        return self._runtime["tool_manager"]

    def activate_llm_tool(self, name):
        return self.get_llm_tool_manager().activate_llm_tool(name)

    def deactivate_llm_tool(self, name):
        return self.get_llm_tool_manager().deactivate_llm_tool(name)

    def add_llm_tools(self, *tools):
        for tool in tools:
            if not isinstance(tool, FunctionTool):
                continue
            if not getattr(tool, "source", ""):
                tool.source = "astrbot_context"
            _register_module_tool(self._module_name, tool)

    def register_web_api(self, route, view_handler, methods=None, desc=""):
        route = _normalize_web_api_route(route)
        methods = _normalize_web_api_methods(methods)
        for index, item in enumerate(self.registered_web_apis):
            if item[0] == route and item[2] == methods:
                self.registered_web_apis[index] = (route, view_handler, methods, desc)
                return
        self.registered_web_apis.append((route, view_handler, methods, desc))

    def register_task(self, task, desc=""):
        self._register_tasks.append({"task": task, "desc": str(desc or "")})


class Star:
    author = ""
    name = ""

    def __init__(self, context, config=None):
        self.context = context

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        _astrbot_star_classes[cls.__module__] = cls

    async def initialize(self):
        return None

    async def terminate(self):
        return None


class StarTools:
    _context = None

    @classmethod
    def initialize(cls, context):
        cls._context = context


def register_star(*args, **kwargs):
    def decorator(obj):
        return obj

    return decorator


def register_command(command_name=None, sub_command=None, alias=None, **kwargs):
    def decorator(awaitable):
        explicit_variants = kwargs.pop("_full_command_variants", None)
        command = command_name
        parent = kwargs.get("_parent_command")
        if explicit_variants is not None:
            variants = list(explicit_variants)
            command = variants[0] if variants else ""
            local_aliases = variants[1:]
        else:
            local_aliases = list(alias or [])
        if sub_command is not None and explicit_variants is None:
            command = sub_command
        if isinstance(command, RegisteringCommandable):
            command = kwargs.get("_parent_command", "")
        command = str(command or "").strip()
        if parent:
            command = f"{parent} {command}".strip()
        _append_handler_meta(
            awaitable,
            {
                "event": "adapter_message",
                "command": command,
                "alias": local_aliases,
            },
        )
        return awaitable

    return decorator


class RegisteringCommandable:
    def __init__(self, command_prefixes):
        if isinstance(command_prefixes, str):
            command_prefixes = [command_prefixes]
        normalized = []
        for item in list(command_prefixes or []):
            text = str(item or "").strip()
            if text and text not in normalized:
                normalized.append(text)
        self._command_prefixes = normalized

    def _join_variants(self, primary=None, alias=None):
        candidates = []
        if primary:
            candidates.append(primary)
        candidates.extend(list(alias or []))
        if not candidates:
            return list(self._command_prefixes)

        out = []
        for prefix in self._command_prefixes:
            for candidate in candidates:
                text = f"{prefix} {candidate}".strip()
                if text and text not in out:
                    out.append(text)
        return out

    def command(self, sub_command=None, alias=None, **kwargs):
        variants = self._join_variants(sub_command, alias)
        return register_command(
            command_name=variants[0] if variants else "",
            alias=variants[1:],
            _full_command_variants=variants,
            **kwargs,
        )

    def group(self, sub_command=None, alias=None, **kwargs):
        next_prefixes = self._join_variants(sub_command, alias)

        def decorator(_obj):
            return RegisteringCommandable(next_prefixes)

        return decorator

    def custom_filter(self, custom_type_filter, *args, **kwargs):
        return register_custom_filter(custom_type_filter, *args, **kwargs)


def register_command_group(command_group_name=None, sub_command=None, alias=None, **kwargs):
    if isinstance(command_group_name, RegisteringCommandable):
        prefixes = command_group_name._join_variants(sub_command, alias)
    else:
        prefixes = []
        for item in [command_group_name, *(list(alias or []))]:
            text = str(item or "").strip()
            if text and text not in prefixes:
                prefixes.append(text)

    def decorator(_obj):
        return RegisteringCommandable(prefixes)

    return decorator


def register_custom_filter(custom_type_filter, *args, **kwargs):
    raise_error = True
    if args:
        raise_error = bool(args[0])

    def decorator(awaitable):
        _append_handler_meta(
            awaitable,
            {
                "event": "adapter_message",
                "custom_filter": custom_type_filter,
                "raise_error": raise_error,
            },
        )
        return awaitable

    return decorator


def register_event_message_type(event_message_type, **kwargs):
    def decorator(awaitable):
        _append_handler_meta(
            awaitable,
            {
                "event": "adapter_message",
                "event_message_type": event_message_type,
            },
        )
        return awaitable

    return decorator


def register_platform_adapter_type(platform_adapter_type, **kwargs):
    def decorator(awaitable):
        _append_handler_meta(
            awaitable,
            {
                "event": "adapter_message",
                "platform_adapter_type": platform_adapter_type,
            },
        )
        return awaitable

    return decorator


def register_regex(regex, **kwargs):
    def decorator(awaitable):
        _append_handler_meta(
            awaitable, {"event": "adapter_message", "regex": regex}
        )
        return awaitable

    return decorator


def register_permission_type(permission_type, raise_error=True):
    def decorator(awaitable):
        _append_handler_meta(
            awaitable,
            {
                "event": "adapter_message",
                "permission_type": permission_type,
                "raise_error": raise_error,
            },
        )
        return awaitable

    return decorator


def register_on_astrbot_loaded(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_astrbot_loaded"})
        return awaitable

    return decorator


def register_on_platform_loaded(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_platform_loaded"})
        return awaitable

    return decorator


def register_on_plugin_error(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_plugin_error"})
        return awaitable

    return decorator


def register_on_plugin_loaded(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_plugin_loaded"})
        return awaitable

    return decorator


def register_on_plugin_unloaded(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_plugin_unloaded"})
        return awaitable

    return decorator


def register_after_message_sent(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "after_message_sent"})
        return awaitable

    return decorator


def register_on_waiting_llm_request(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_waiting_llm_request"})
        return awaitable

    return decorator


def register_on_llm_request(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_llm_request"})
        return awaitable

    return decorator


def register_on_llm_response(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_llm_response"})
        return awaitable

    return decorator


def register_on_agent_begin(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_agent_begin"})
        return awaitable

    return decorator


def register_on_agent_done(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_agent_done"})
        return awaitable

    return decorator


def register_llm_tool(name=None, **kwargs):
    registering_agent = kwargs.get("registering_agent")

    def decorator(awaitable):
        tool = _build_function_tool(awaitable, name=name)
        if registering_agent is not None:
            registering_agent._agent.tools.append(tool)
        else:
            _register_module_tool(awaitable.__module__, tool)
        _append_handler_meta(
            awaitable,
            {"event": "llm_tool", "name": tool.name, "tool": tool},
        )
        return awaitable

    return decorator


class CompatAgent:
    def __init__(self, name, instruction, tools=None, run_hooks=None):
        self.name = str(name or "").strip()
        self.instructions = str(instruction or "").strip()
        self.tools = list(tools or [])
        self.run_hooks = run_hooks


class RegisteringAgent:
    def __init__(self, agent):
        self._agent = agent

    def llm_tool(self, *args, **kwargs):
        kwargs["registering_agent"] = self
        return register_llm_tool(*args, **kwargs)


def register_agent(name, instruction, tools=None, run_hooks=None):
    def decorator(awaitable):
        agent = CompatAgent(name, instruction, tools=tools, run_hooks=run_hooks)
        runtime = _ensure_astrbot_module_runtime(awaitable.__module__)
        runtime["agents"].append(agent)
        return RegisteringAgent(agent)

    return decorator


def register_on_using_llm_tool(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_using_llm_tool"})
        return awaitable

    return decorator


def register_on_llm_tool_respond(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_llm_tool_respond"})
        return awaitable

    return decorator


def register_on_decorating_result(**kwargs):
    def decorator(awaitable):
        _append_handler_meta(awaitable, {"event": "on_decorating_result"})
        return awaitable

    return decorator


def register_platform_adapter(*args, **kwargs):
    def decorator(awaitable):
        return awaitable

    return decorator


class AstrMessageEvent:
    def __init__(self, event, sdk=None):
        self._event = event if isinstance(event, dict) else {}
        self._payload = self._event.get("payload", {})
        if not isinstance(self._payload, dict):
            self._payload = {}
        self._sdk = sdk
        self.message_str = _extract_message_text(self._payload)
        self.raw_message = self.message_str
        self._extras = {}
        self._result = None
        self.call_llm = False
        self.platform_meta = PlatformMetadata(
            self._payload.get("_adapter_protocol", ""),
            self._payload.get("_adapter_id", ""),
        )

    @property
    def payload(self):
        return self._payload

    def get_message_str(self):
        return self.message_str

    def get_message_outline(self):
        return self.message_str

    def get_group_id(self):
        return str(self._payload.get("group_id", "") or "")

    def get_self_id(self):
        return str(self._payload.get("self_id", "") or "")

    def get_sender_id(self):
        return str(self._payload.get("user_id", "") or "")

    def get_sender_name(self):
        sender = self._payload.get("sender")
        if isinstance(sender, dict):
            nickname = sender.get("nickname")
            if nickname is not None:
                return str(nickname)
        return ""

    def get_message_type(self):
        message_type = str(self._payload.get("message_type", "") or "").lower()
        if message_type == "group":
            return MessageType.GROUP_MESSAGE
        if message_type == "private":
            return MessageType.FRIEND_MESSAGE
        return MessageType.OTHER_MESSAGE

    def is_private_chat(self):
        return self.get_message_type() == MessageType.FRIEND_MESSAGE

    def is_admin(self):
        sender = self._payload.get("sender")
        if isinstance(sender, dict):
            role = str(sender.get("role", "") or "").lower()
            return role in ("admin", "owner")
        return False

    def set_extra(self, key, value):
        self._extras[key] = value

    def get_extra(self, key=None, default=None):
        if key is None:
            return self._extras
        return self._extras.get(key, default)

    def clear_extra(self):
        self._extras.clear()

    def set_result(self, result):
        if isinstance(result, str):
            result = MessageEventResult().message(result)
        if isinstance(result, MessageEventResult) and result.chain is None:
            result.chain = []
        self._result = result

    def stop_event(self):
        if self._result is None:
            self.set_result(MessageEventResult().stop_event())
        else:
            self._result.stop_event()

    def continue_event(self):
        if self._result is None:
            self.set_result(MessageEventResult().continue_event())
        else:
            self._result.continue_event()

    def is_stopped(self):
        if self._result is None:
            return False
        return bool(self._result.is_stopped())

    def get_result(self):
        return self._result

    def clear_result(self):
        self._result = None

    def should_call_llm(self, call_llm):
        self.call_llm = bool(call_llm)

    def make_result(self):
        return MessageEventResult()

    def plain_result(self, text):
        return MessageEventResult().message(text)

    def image_result(self, url_or_path):
        return MessageEventResult().message(str(url_or_path))

    def chain_result(self, chain):
        result = MessageEventResult()
        result.chain = list(chain or [])
        return result

    def request_llm(
        self,
        prompt,
        func_tool_manager=None,
        tool_set=None,
        session_id="",
        image_urls=None,
        audio_urls=None,
        contexts=None,
        system_prompt="",
        conversation=None,
    ):
        return {
            "prompt": prompt,
            "tool_set": tool_set,
            "contexts": contexts or [],
            "system_prompt": system_prompt,
            "conversation": conversation,
        }

    def reply(self, message):
        if self._sdk is None:
            return False
        text = _coerce_text_output(message).strip()
        if not text:
            return False
        return bool(self._sdk.reply_text(self._event, text))

    async def send(self, message):
        return self.reply(message)


def _event_message_type_matches(event, expected):
    current = event.get_message_type()
    if current == MessageType.GROUP_MESSAGE:
        actual = EventMessageType.GROUP_MESSAGE
    elif current == MessageType.FRIEND_MESSAGE:
        actual = EventMessageType.PRIVATE_MESSAGE
    else:
        actual = EventMessageType.OTHER_MESSAGE
    return bool(actual & expected)


def _handler_matches(meta, event):
    command = meta.get("command")
    if command is not None:
        prefixes = [meta.get("command", "")]
        prefixes.extend(meta.get("alias", []))
        if not _matches_command(event.get_message_str(), prefixes):
            return False

    regex = meta.get("regex")
    if regex is not None and not re.search(regex, event.get_message_str()):
        return False

    expected = meta.get("event_message_type")
    if expected is not None and not _event_message_type_matches(event, expected):
        return False

    permission = meta.get("permission_type")
    if permission in (PermissionType.ADMIN, "admin") and not event.is_admin():
        return False

    platform_adapter_type = meta.get("platform_adapter_type")
    if platform_adapter_type not in (
        PlatformAdapterType.ANY,
        PlatformAdapterType.ALL,
        "any",
        "all",
        None,
    ):
        current = str(event.payload.get("_adapter_protocol", "") or "").lower()
        if platform_adapter_type in (PlatformAdapterType.ONEBOT, "onebot") and "onebot" not in current:
            return False

    custom_filter = meta.get("custom_filter")
    if callable(custom_filter):
        result = custom_filter(event)
        if _is_awaitable(result):
            return False
        if not bool(result):
            return False

    return True


async def _invoke_registered_handler(handler, event, meta):
    result = handler(*_build_handler_args(handler, event, meta))
    if _is_awaitable(result):
        result = await result
    if isinstance(result, MessageEventResult):
        event.set_result(result)
    elif isinstance(result, str):
        await event.send(result)
    return result


def _build_handler_args(handler, event, meta):
    args = [event]
    command_argv = _extract_command_arguments(event.get_message_str(), meta)
    if not command_argv:
        return args

    try:
        parameters = list(inspect.signature(handler).parameters.values())
    except (TypeError, ValueError):
        return args + command_argv

    positional = [
        parameter
        for parameter in parameters
        if parameter.kind
        in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
    ]
    accepts_varargs = any(
        parameter.kind == inspect.Parameter.VAR_POSITIONAL
        for parameter in parameters
    )
    if accepts_varargs:
        return args + command_argv

    extra_capacity = max(len(positional) - 1, 0)
    return args + command_argv[:extra_capacity]


def _extract_command_arguments(message, meta):
    message = str(message or "").strip()
    command = meta.get("command")
    if command is None:
        return []
    prefixes = [command]
    prefixes.extend(meta.get("alias", []))
    matched = _match_command_prefix(message, prefixes)
    if matched is None:
        return []
    remainder = message[len(matched) :].strip()
    if not remainder:
        return []
    try:
        return shlex.split(remainder)
    except ValueError:
        return remainder.split()


async def _deliver_event_result(event):
    result = event.get_result()
    if result is None:
        return False
    text = _coerce_text_output(result).strip()
    if not text:
        return False
    return bool(await event.send(text))


def _iter_module_handlers(module, instance):
    handlers = []
    module_globals = getattr(module, "__dict__", {})
    for func in module_globals.values():
        metas = getattr(func, "__astrbot_handler_meta__", None)
        if metas:
            for meta in metas:
                handlers.append((meta, func))
    if instance is not None:
        for name, value in type(instance).__dict__.items():
            metas = getattr(value, "__astrbot_handler_meta__", None)
            if not metas:
                continue
            bound_handler = getattr(instance, name)
            for meta in metas:
                handlers.append((meta, bound_handler))
    return handlers


def _load_context_config(sdk):
    try:
        config = sdk.config_get("")
    except Exception:
        return {}
    if isinstance(config, dict):
        return config
    return {}


def _resolve_star_class(module):
    star_cls = _astrbot_star_classes.get(getattr(module, "__name__", ""))
    if star_cls is not None:
        return star_cls
    for value in getattr(module, "__dict__", {}).values():
        if isinstance(value, type) and value is not Star and issubclass(value, Star):
            return value
    return None


def _bind_astrbot_plugin_runtime(module, sdk):
    runtime = _ensure_astrbot_module_runtime(module)
    star_cls = _resolve_star_class(module)
    instance = None
    if star_cls is not None:
        context = Context(sdk, _load_context_config(sdk), getattr(module, "__name__", ""))
        StarTools.initialize(context)
        instance = star_cls(context, context.get_config())
        _bind_module_tools(module, instance)
        module.__astrbot_context__ = context
    else:
        module.__astrbot_context__ = None

    _attach_astrbot_runtime(module, runtime)

    handlers = _iter_module_handlers(module, instance)
    if instance is None and not handlers and not runtime["llm_tools"]:
        return False

    async def _astrbot_dispatch(event, runtime_sdk=None):
        current_sdk = runtime_sdk or sdk
        event_obj = AstrMessageEvent(event, current_sdk)
        for meta, handler in handlers:
            if meta.get("event") != "adapter_message":
                continue
            if not _handler_matches(meta, event_obj):
                continue
            await _invoke_registered_handler(handler, event_obj, meta)
            if event_obj.is_stopped():
                break
        await _deliver_event_result(event_obj)
        return event_obj.get_result()

    async def _astrbot_start(runtime_sdk=None):
        if instance is not None:
            initializer = getattr(instance, "initialize", None)
            if callable(initializer):
                result = initializer()
                if _is_awaitable(result):
                    await result
        for meta, handler in handlers:
            if meta.get("event") != "on_astrbot_loaded":
                continue
            result = handler()
            if _is_awaitable(result):
                await result

    async def _astrbot_shutdown(runtime_sdk=None):
        if instance is not None:
            terminator = getattr(instance, "terminate", None)
            if callable(terminator):
                result = terminator()
                if _is_awaitable(result):
                    await result

    async def _astrbot_health(runtime_sdk=None):
        return None

    module.liteyuki_handle_event = _astrbot_dispatch
    module.handle_event = _astrbot_dispatch
    module.on_event = _astrbot_dispatch
    module.liteyuki_start = _astrbot_start
    module.liteyuki_health_check = _astrbot_health
    module.liteyuki_shutdown = _astrbot_shutdown
    module.liteyuki_unload = _astrbot_shutdown
    return True


def _ensure_module(name):
    module = sys.modules.get(name)
    if module is None:
        module = types.ModuleType(name)
        sys.modules[name] = module
    return module


def _install_compat_modules(sdk):
    liteyuki_root = _ensure_module("liteyuki")
    liteyuki_root.PluginType = PluginType
    liteyuki_root.PluginMetadata = PluginMetadata
    liteyuki_root.MessageEvent = MessageEvent
    liteyuki_root.on_message = on_message
    liteyuki_root.on_startswith = on_startswith
    liteyuki_root.is_su_rule = is_su_rule
    liteyuki_root._cleanup_astrbot_plugin_runtime = _cleanup_astrbot_plugin_runtime
    liteyuki_root._get_astrbot_plugin_runtime = _get_astrbot_plugin_runtime
    liteyuki_root._snapshot_astrbot_plugin_runtime = _snapshot_astrbot_plugin_runtime
    liteyuki_root._bind_astrbot_plugin_runtime = _bind_astrbot_plugin_runtime
    liteyuki_root._build_astrbot_web_request_context = _build_astrbot_web_request_context
    liteyuki_root._invoke_astrbot_web_handler = _invoke_astrbot_web_handler
    liteyuki_root._set_astrbot_web_request_context = _set_astrbot_web_request_context
    liteyuki_root._reset_astrbot_web_request_context = _reset_astrbot_web_request_context
    liteyuki_root.sdk = sdk if sdk is not None else None

    liteyuki_sdk_module = _ensure_module("liteyuki_sdk")
    liteyuki_sdk_module.sdk = sdk if sdk is not None else None

    liteyuki_plugin = _ensure_module("liteyuki.plugin")
    liteyuki_plugin.PluginType = PluginType
    liteyuki_plugin.PluginMetadata = PluginMetadata

    liteyuki_session = _ensure_module("liteyuki.session")
    liteyuki_session_on = _ensure_module("liteyuki.session.on")
    liteyuki_session_event = _ensure_module("liteyuki.session.event")
    liteyuki_session_rule = _ensure_module("liteyuki.session.rule")
    liteyuki_session_on.on_message = on_message
    liteyuki_session_on.on_startswith = on_startswith
    liteyuki_session_event.MessageEvent = MessageEvent
    liteyuki_session_rule.is_su_rule = is_su_rule
    liteyuki_root.plugin = liteyuki_plugin
    liteyuki_root.session = liteyuki_session
    liteyuki_session.on = liteyuki_session_on
    liteyuki_session.event = liteyuki_session_event
    liteyuki_session.rule = liteyuki_session_rule

    quart_module = _ensure_module("quart")
    quart_module.request = _CompatRequestProxy()
    quart_module.jsonify = jsonify
    quart_module.make_response = make_response
    quart_module.Response = CompatWebApiResponse

    astrbot_root = _ensure_module("astrbot")
    astrbot_root.logger = logging.getLogger("astrbot")

    astrbot_core = _ensure_module("astrbot.core")
    astrbot_core.html_renderer = types.SimpleNamespace()
    astrbot_core.sp = types.SimpleNamespace()

    astrbot_core_agent = _ensure_module("astrbot.core.agent")
    astrbot_core_agent_tool = _ensure_module("astrbot.core.agent.tool")
    astrbot_core_agent_tool.FunctionTool = FunctionTool
    astrbot_core_agent_tool.ToolSet = ToolSet

    astrbot_core_agent_tool_executor = _ensure_module(
        "astrbot.core.agent.tool_executor"
    )
    astrbot_core_agent_tool_executor.BaseFunctionToolExecutor = BaseFunctionToolExecutor

    astrbot_core_provider = _ensure_module("astrbot.core.provider")
    astrbot_core_provider_register = _ensure_module("astrbot.core.provider.register")
    astrbot_core_provider_register.llm_tools = _astrbot_global_tool_manager
    astrbot_core_provider_func_tool_manager = _ensure_module(
        "astrbot.core.provider.func_tool_manager"
    )
    astrbot_core_provider_func_tool_manager.FunctionTool = FunctionTool
    astrbot_core_provider_func_tool_manager.FunctionToolManager = CompatToolManager

    astrbot_core_star = _ensure_module("astrbot.core.star")
    astrbot_core_star.Star = Star
    astrbot_core_star.Context = Context
    astrbot_core_star.StarTools = StarTools

    astrbot_core_star_register = _ensure_module("astrbot.core.star.register")
    astrbot_core_star_register.register_star = register_star
    astrbot_core_star_register.register_command = register_command
    astrbot_core_star_register.register_command_group = register_command_group
    astrbot_core_star_register.register_custom_filter = register_custom_filter
    astrbot_core_star_register.register_event_message_type = register_event_message_type
    astrbot_core_star_register.register_platform_adapter_type = register_platform_adapter_type
    astrbot_core_star_register.register_regex = register_regex
    astrbot_core_star_register.register_permission_type = register_permission_type
    astrbot_core_star_register.register_on_astrbot_loaded = register_on_astrbot_loaded
    astrbot_core_star_register.register_on_platform_loaded = register_on_platform_loaded
    astrbot_core_star_register.register_on_plugin_error = register_on_plugin_error
    astrbot_core_star_register.register_on_plugin_loaded = register_on_plugin_loaded
    astrbot_core_star_register.register_on_plugin_unloaded = register_on_plugin_unloaded
    astrbot_core_star_register.register_after_message_sent = register_after_message_sent
    astrbot_core_star_register.register_on_waiting_llm_request = register_on_waiting_llm_request
    astrbot_core_star_register.register_on_llm_request = register_on_llm_request
    astrbot_core_star_register.register_on_llm_response = register_on_llm_response
    astrbot_core_star_register.register_on_agent_begin = register_on_agent_begin
    astrbot_core_star_register.register_on_agent_done = register_on_agent_done
    astrbot_core_star_register.register_llm_tool = register_llm_tool
    astrbot_core_star_register.register_agent = register_agent
    astrbot_core_star_register.register_on_using_llm_tool = register_on_using_llm_tool
    astrbot_core_star_register.register_on_llm_tool_respond = register_on_llm_tool_respond
    astrbot_core_star_register.register_on_decorating_result = register_on_decorating_result

    astrbot_core_platform = _ensure_module("astrbot.core.platform")
    astrbot_core_platform.AstrMessageEvent = AstrMessageEvent
    astrbot_core_platform.Platform = Platform
    astrbot_core_platform.AstrBotMessage = AstrBotMessage
    astrbot_core_platform.MessageMember = MessageMember
    astrbot_core_platform.MessageType = MessageType
    astrbot_core_platform.PlatformMetadata = PlatformMetadata

    astrbot_core_platform_register = _ensure_module("astrbot.core.platform.register")
    astrbot_core_platform_register.register_platform_adapter = register_platform_adapter

    astrbot_core_message_event_result = _ensure_module(
        "astrbot.core.message.message_event_result"
    )
    astrbot_core_message_event_result.MessageEventResult = MessageEventResult
    astrbot_core_message_event_result.MessageChain = MessageChain
    astrbot_core_message_event_result.CommandResult = CommandResult
    astrbot_core_message_event_result.EventResultType = EventResultType
    astrbot_core_message_event_result.ResultContentType = str

    astrbot_core_star_filter_event_message_type = _ensure_module(
        "astrbot.core.star.filter.event_message_type"
    )
    astrbot_core_star_filter_event_message_type.EventMessageType = EventMessageType
    astrbot_core_star_filter_event_message_type.EventMessageTypeFilter = EventMessageTypeFilter

    astrbot_core_star_filter_permission = _ensure_module(
        "astrbot.core.star.filter.permission"
    )
    astrbot_core_star_filter_permission.PermissionType = PermissionType
    astrbot_core_star_filter_permission.PermissionTypeFilter = PermissionTypeFilter

    astrbot_core_star_filter_platform = _ensure_module(
        "astrbot.core.star.filter.platform_adapter_type"
    )
    astrbot_core_star_filter_platform.PlatformAdapterType = PlatformAdapterType
    astrbot_core_star_filter_platform.PlatformAdapterTypeFilter = PlatformAdapterTypeFilter

    astrbot_core_star_filter_regex = _ensure_module("astrbot.core.star.filter.regex")
    astrbot_core_star_filter_regex.RegexFilter = RegexFilter

    astrbot_core_star_filter_custom = _ensure_module(
        "astrbot.core.star.filter.custom_filter"
    )
    astrbot_core_star_filter_custom.CustomFilter = CustomFilter

    astrbot_api = _ensure_module("astrbot.api")
    astrbot_api.logger = astrbot_root.logger
    astrbot_api.html_renderer = astrbot_core.html_renderer
    astrbot_api.sp = astrbot_core.sp
    astrbot_api.FunctionTool = FunctionTool
    astrbot_api.ToolSet = ToolSet
    astrbot_api.BaseFunctionToolExecutor = BaseFunctionToolExecutor
    astrbot_api.AstrBotConfig = AstrBotConfig
    astrbot_api.agent = register_agent
    astrbot_api.llm_tool = register_llm_tool

    astrbot_api_star = _ensure_module("astrbot.api.star")
    astrbot_api_star.Star = Star
    astrbot_api_star.Context = Context
    astrbot_api_star.StarTools = StarTools
    astrbot_api_star.register = register_star

    astrbot_api_event = _ensure_module("astrbot.api.event")
    astrbot_api_event.AstrMessageEvent = AstrMessageEvent
    astrbot_api_event.CommandResult = CommandResult
    astrbot_api_event.EventResultType = EventResultType
    astrbot_api_event.MessageChain = MessageChain
    astrbot_api_event.MessageEventResult = MessageEventResult
    astrbot_api_event.ResultContentType = str

    astrbot_api_event_filter = _ensure_module("astrbot.api.event.filter")
    astrbot_api_event_filter.CustomFilter = CustomFilter
    astrbot_api_event_filter.EventMessageType = EventMessageType
    astrbot_api_event_filter.EventMessageTypeFilter = EventMessageTypeFilter
    astrbot_api_event_filter.PermissionType = PermissionType
    astrbot_api_event_filter.PermissionTypeFilter = PermissionTypeFilter
    astrbot_api_event_filter.PlatformAdapterType = PlatformAdapterType
    astrbot_api_event_filter.PlatformAdapterTypeFilter = PlatformAdapterTypeFilter
    astrbot_api_event_filter.after_message_sent = register_after_message_sent
    astrbot_api_event_filter.command = register_command
    astrbot_api_event_filter.command_group = register_command_group
    astrbot_api_event_filter.custom_filter = register_custom_filter
    astrbot_api_event_filter.event_message_type = register_event_message_type
    astrbot_api_event_filter.llm_tool = register_llm_tool
    astrbot_api_event_filter.on_agent_begin = register_on_agent_begin
    astrbot_api_event_filter.on_agent_done = register_on_agent_done
    astrbot_api_event_filter.on_astrbot_loaded = register_on_astrbot_loaded
    astrbot_api_event_filter.on_decorating_result = register_on_decorating_result
    astrbot_api_event_filter.on_llm_request = register_on_llm_request
    astrbot_api_event_filter.on_llm_response = register_on_llm_response
    astrbot_api_event_filter.on_llm_tool_respond = register_on_llm_tool_respond
    astrbot_api_event_filter.on_platform_loaded = register_on_platform_loaded
    astrbot_api_event_filter.on_plugin_error = register_on_plugin_error
    astrbot_api_event_filter.on_plugin_loaded = register_on_plugin_loaded
    astrbot_api_event_filter.on_plugin_unloaded = register_on_plugin_unloaded
    astrbot_api_event_filter.on_using_llm_tool = register_on_using_llm_tool
    astrbot_api_event_filter.on_waiting_llm_request = register_on_waiting_llm_request
    astrbot_api_event_filter.permission_type = register_permission_type
    astrbot_api_event_filter.platform_adapter_type = register_platform_adapter_type
    astrbot_api_event_filter.regex = register_regex
    astrbot_api_event.filter = astrbot_api_event_filter

    astrbot_api_platform = _ensure_module("astrbot.api.platform")
    astrbot_api_platform.AstrMessageEvent = AstrMessageEvent
    astrbot_api_platform.Platform = Platform
    astrbot_api_platform.AstrBotMessage = AstrBotMessage
    astrbot_api_platform.MessageMember = MessageMember
    astrbot_api_platform.MessageType = MessageType
    astrbot_api_platform.PlatformMetadata = PlatformMetadata
    astrbot_api_platform.register_platform_adapter = register_platform_adapter

    astrbot_api_provider = _ensure_module("astrbot.api.provider")
    astrbot_api_provider.Provider = Provider
    astrbot_api_provider.ProviderMetaData = ProviderMetaData
    astrbot_api_provider.Personality = Personality

    astrbot_api_message_components = _ensure_module("astrbot.api.message_components")

    astrbot_api_all = _ensure_module("astrbot.api.all")
    astrbot_api_all.AstrBotConfig = AstrBotConfig
    astrbot_api_all.logger = astrbot_root.logger
    astrbot_api_all.html_renderer = astrbot_core.html_renderer
    astrbot_api_all.llm_tool = register_llm_tool
    astrbot_api_all.MessageEventResult = MessageEventResult
    astrbot_api_all.MessageChain = MessageChain
    astrbot_api_all.CommandResult = CommandResult
    astrbot_api_all.EventResultType = EventResultType
    astrbot_api_all.AstrMessageEvent = AstrMessageEvent
    astrbot_api_all.command = register_command
    astrbot_api_all.command_group = register_command_group
    astrbot_api_all.event_message_type = register_event_message_type
    astrbot_api_all.regex = register_regex
    astrbot_api_all.platform_adapter_type = register_platform_adapter_type
    astrbot_api_all.EventMessageTypeFilter = EventMessageTypeFilter
    astrbot_api_all.EventMessageType = EventMessageType
    astrbot_api_all.PlatformAdapterTypeFilter = PlatformAdapterTypeFilter
    astrbot_api_all.PlatformAdapterType = PlatformAdapterType
    astrbot_api_all.register = register_star
    astrbot_api_all.Context = Context
    astrbot_api_all.Star = Star
    astrbot_api_all.Provider = Provider
    astrbot_api_all.ProviderMetaData = ProviderMetaData
    astrbot_api_all.Personality = Personality
    astrbot_api_all.Platform = Platform
    astrbot_api_all.AstrBotMessage = AstrBotMessage
    astrbot_api_all.MessageMember = MessageMember
    astrbot_api_all.MessageType = MessageType
    astrbot_api_all.PlatformMetadata = PlatformMetadata
    astrbot_api_all.register_platform_adapter = register_platform_adapter

    astrbot_root.api = astrbot_api
    astrbot_root.core = astrbot_core
    astrbot_core.agent = astrbot_core_agent
    astrbot_core_agent.tool = astrbot_core_agent_tool
    astrbot_core_agent.tool_executor = astrbot_core_agent_tool_executor
    astrbot_core.star = astrbot_core_star
    astrbot_core.platform = astrbot_core_platform
    astrbot_core.provider = astrbot_core_provider
    astrbot_core_provider.register = astrbot_core_provider_register
    astrbot_core_provider.func_tool_manager = astrbot_core_provider_func_tool_manager
    astrbot_core.message = types.SimpleNamespace(
        message_event_result=astrbot_core_message_event_result
    )
    astrbot_core.star.register = astrbot_core_star_register
    astrbot_api.star = astrbot_api_star
    astrbot_api.event = astrbot_api_event
    astrbot_api.platform = astrbot_api_platform
    astrbot_api.provider = astrbot_api_provider
    astrbot_api.message_components = astrbot_api_message_components
    astrbot_api.all = astrbot_api_all
    astrbot_api.filter = astrbot_api_event_filter
    astrbot_api_all.filter = astrbot_api_event_filter


_install_compat_modules(globals().get("__bridge_sdk__"))
