# Neo-MoFox compatibility module installer.
# Executed in the shared Python bridge globals.

_neomofox_plugin_registry = {}


def _normalize_neomofox_component_name(value, fallback):
    text = str(value or "").strip()
    if text:
        return text
    return str(fallback or "").strip()


class NeoMoFoxBaseComponent:
    def __init__(self, *args, **kwargs):
        self.args = args
        self.kwargs = kwargs


class BasePlugin(NeoMoFoxBaseComponent):
    plugin_name = ""
    plugin_description = ""
    plugin_version = ""
    configs = []
    dependent_components = []

    def __init__(self, config=None):
        super().__init__(config=config)
        self.config = config

    def get_components(self):
        return []

    async def on_plugin_loaded(self):
        return None

    async def on_plugin_unloaded(self):
        return None


class BaseTool(NeoMoFoxBaseComponent):
    tool_name = ""
    tool_description = ""


class BaseAction(NeoMoFoxBaseComponent):
    action_name = ""
    action_description = ""


class BaseCommand(NeoMoFoxBaseComponent):
    command_name = ""
    command_description = ""
    command_prefix = "/"


class BaseService(NeoMoFoxBaseComponent):
    service_name = ""


class BaseEventHandler(NeoMoFoxBaseComponent):
    handler_name = ""
    handler_description = ""
    init_subscribe = []


class BaseRouter(NeoMoFoxBaseComponent):
    router_name = ""

    def register_endpoints(self):
        return None


def register_plugin(cls):
    plugin_name = _normalize_neomofox_component_name(
        getattr(cls, "plugin_name", ""), getattr(cls, "__name__", "")
    )
    if plugin_name:
        cls.plugin_name = plugin_name
        _neomofox_plugin_registry[plugin_name] = cls
    return cls


def cmd_route(*path):
    route = tuple(str(item).strip() for item in path if str(item).strip())

    def decorator(func):
        routes = list(getattr(func, "__neomofox_cmd_routes__", []))
        routes.append(route)
        func.__neomofox_cmd_routes__ = routes
        return func

    return decorator


def _load_neomofox_config(sdk):
    try:
        config = sdk.config_get("")
    except Exception:
        return {}
    if isinstance(config, dict):
        return config
    return {}


def _resolve_neomofox_plugin_class(module):
    module_name = getattr(module, "__name__", "")
    for value in getattr(module, "__dict__", {}).values():
        if (
            isinstance(value, type)
            and value is not BasePlugin
            and issubclass(value, BasePlugin)
        ):
            return value
    for value in _neomofox_plugin_registry.values():
        if getattr(value, "__module__", "") == module_name:
            return value
    prefix = module_name + "."
    for value in _neomofox_plugin_registry.values():
        if str(getattr(value, "__module__", "")).startswith(prefix):
            return value
    return None


def _normalize_neomofox_component_list(plugin_instance):
    try:
        components = plugin_instance.get_components()
    except Exception:
        return []
    if not isinstance(components, (list, tuple, set)):
        return []
    return [item for item in components if isinstance(item, type)]


def _build_neomofox_component_instance(component_cls, plugin_instance):
    if issubclass(component_cls, BaseAction):
        return component_cls(None, plugin_instance)
    if issubclass(component_cls, BaseCommand):
        return component_cls(plugin_instance, "")
    return component_cls(plugin_instance)


def _neomofox_component_name(component_cls, attr, fallback):
    return _normalize_neomofox_component_name(getattr(component_cls, attr, ""), fallback)


def _build_neomofox_tool(component, component_cls, module_name):
    execute = getattr(component, "execute", None)
    if not callable(execute):
        return None
    tool_name = _neomofox_component_name(
        component_cls, "tool_name", getattr(component_cls, "__name__", "")
    )
    if not tool_name:
        return None
    tool = FunctionTool(
        name=tool_name,
        description=str(getattr(component_cls, "tool_description", "") or "").strip(),
        parameters=_build_tool_parameters(execute),
        handler=execute,
        handler_module_path=module_name,
    )
    tool.source = "neomofox_component"
    return tool


def _iter_neomofox_command_handlers(component, component_cls):
    command_name = _neomofox_component_name(
        component_cls, "command_name", getattr(component_cls, "__name__", "")
    )
    if not command_name:
        return []
    prefix = str(getattr(component_cls, "command_prefix", "/") or "/").strip() or "/"
    metas = []
    for name, value in type(component).__dict__.items():
        routes = getattr(value, "__neomofox_cmd_routes__", None)
        if not routes:
            continue
        handler = getattr(component, name)
        for route in routes:
            parts = [str(item).strip() for item in route if str(item).strip()]
            command = " ".join([command_name] + parts).strip()
            aliases = [prefix + command] if prefix and not command.startswith(prefix) else []
            metas.append(
                (
                    {
                        "event": "adapter_message",
                        "command": command,
                        "alias": aliases,
                    },
                    handler,
                )
            )
    return metas


def _iter_neomofox_event_handlers(component, component_cls):
    execute = getattr(component, "execute", None)
    if not callable(execute):
        return []
    subscriptions = list(getattr(component_cls, "init_subscribe", []) or [])
    return [
        (
            {
                "event": "neomofox_event",
                "subscriptions": [str(item) for item in subscriptions],
            },
            execute,
        )
    ]


def _neomofox_event_name(event):
    if isinstance(event, dict):
        topic = str(event.get("topic", "") or "")
        payload = event.get("payload", {})
        if topic:
            return topic
        if isinstance(payload, dict):
            return str(payload.get("post_type", "") or "message")
    return "event"


def _neomofox_event_payload(event):
    if isinstance(event, dict):
        payload = event.get("payload", {})
        if isinstance(payload, dict):
            return payload
    return {}


async def _invoke_neomofox_component_handler(handler, *args):
    result = handler(*args)
    if _is_awaitable(result):
        result = await result
    return result


def _bind_neomofox_plugin_runtime(module, sdk):
    plugin_cls = _resolve_neomofox_plugin_class(module)
    if plugin_cls is None:
        return False
    config = _load_neomofox_config(sdk)
    plugin_instance = plugin_cls(config)
    module.__neomofox_plugin_instance__ = plugin_instance
    module.__neomofox_components__ = []

    runtime = _ensure_astrbot_module_runtime(module)
    handlers = []
    module_name = getattr(module, "__name__", "")
    for component_cls in _normalize_neomofox_component_list(plugin_instance):
        try:
            component = _build_neomofox_component_instance(component_cls, plugin_instance)
        except Exception:
            continue
        module.__neomofox_components__.append(component)
        if issubclass(component_cls, (BaseTool, BaseAction)):
            tool = _build_neomofox_tool(component, component_cls, module_name)
            if tool is not None:
                _register_module_tool(module_name, tool)
        elif issubclass(component_cls, BaseCommand):
            handlers.extend(_iter_neomofox_command_handlers(component, component_cls))
        elif issubclass(component_cls, BaseEventHandler):
            handlers.extend(_iter_neomofox_event_handlers(component, component_cls))

    async def _neomofox_dispatch(event, runtime_sdk=None):
        event_obj = AstrMessageEvent(event, runtime_sdk or sdk)
        event_name = _neomofox_event_name(event)
        event_payload = _neomofox_event_payload(event)
        for meta, handler in handlers:
            if meta.get("event") == "adapter_message":
                if not _handler_matches(meta, event_obj):
                    continue
                await _invoke_registered_handler(handler, event_obj, meta)
                if event_obj.is_stopped():
                    break
            elif meta.get("event") == "neomofox_event":
                subscriptions = meta.get("subscriptions", [])
                if subscriptions and event_name not in subscriptions:
                    continue
                await _invoke_neomofox_component_handler(
                    handler, event_name, dict(event_payload)
                )
        await _deliver_event_result(event_obj)
        return event_obj.get_result()

    async def _neomofox_start(runtime_sdk=None):
        result = plugin_instance.on_plugin_loaded()
        if _is_awaitable(result):
            await result

    async def _neomofox_shutdown(runtime_sdk=None):
        result = plugin_instance.on_plugin_unloaded()
        if _is_awaitable(result):
            await result

    async def _neomofox_health(runtime_sdk=None):
        return None

    module.__neomofox_runtime__ = runtime
    module.liteyuki_handle_event = _neomofox_dispatch
    module.handle_event = _neomofox_dispatch
    module.on_event = _neomofox_dispatch
    module.liteyuki_start = _neomofox_start
    module.liteyuki_health_check = _neomofox_health
    module.liteyuki_shutdown = _neomofox_shutdown
    module.liteyuki_unload = _neomofox_shutdown
    return True


def _install_neomofox_compat_modules(sdk):
    liteyuki_root = _ensure_module("liteyuki")
    previous_bind = getattr(liteyuki_root, "_bind_astrbot_plugin_runtime", None)

    def _bind_astrbot_or_neomofox_plugin_runtime(module, runtime_sdk):
        if callable(previous_bind) and previous_bind(module, runtime_sdk):
            return True
        return _bind_neomofox_plugin_runtime(module, runtime_sdk)

    liteyuki_root._bind_astrbot_plugin_runtime = (
        _bind_astrbot_or_neomofox_plugin_runtime
    )
    liteyuki_root._bind_neomofox_plugin_runtime = _bind_neomofox_plugin_runtime

    src_module = _ensure_module("src")
    app_module = _ensure_module("src.app")
    plugin_system_module = _ensure_module("src.app.plugin_system")
    base_module = _ensure_module("src.app.plugin_system.base")
    core_module = _ensure_module("src.core")
    components_module = _ensure_module("src.core.components")
    component_base_module = _ensure_module("src.core.components.base")

    for module in (base_module, components_module):
        module.BasePlugin = BasePlugin
        module.BaseTool = BaseTool
        module.BaseAction = BaseAction
        module.BaseCommand = BaseCommand
        module.BaseService = BaseService
        module.BaseEventHandler = BaseEventHandler
        module.BaseRouter = BaseRouter
        module.register_plugin = register_plugin
        module.cmd_route = cmd_route

    component_base_module.plugin = types.SimpleNamespace(BasePlugin=BasePlugin)
    component_base_module.tool = types.SimpleNamespace(BaseTool=BaseTool)
    component_base_module.action = types.SimpleNamespace(BaseAction=BaseAction)
    component_base_module.command = types.SimpleNamespace(
        BaseCommand=BaseCommand, cmd_route=cmd_route
    )
    component_base_module.service = types.SimpleNamespace(BaseService=BaseService)
    component_base_module.event_handler = types.SimpleNamespace(
        BaseEventHandler=BaseEventHandler
    )
    component_base_module.router = types.SimpleNamespace(BaseRouter=BaseRouter)

    src_module.app = app_module
    app_module.plugin_system = plugin_system_module
    plugin_system_module.base = base_module
    src_module.core = core_module
    core_module.components = components_module
    components_module.base = component_base_module
