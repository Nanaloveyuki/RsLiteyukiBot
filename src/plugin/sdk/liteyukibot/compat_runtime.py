# LiteyukiBot legacy compatibility module installer.
# Executed in the shared Python bridge globals.
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


def _is_message_payload(payload):
    if not isinstance(payload, dict):
        return False
    if str(payload.get("post_type", "") or "").lower() == "message":
        return True
    if str(payload.get("message_type", "") or "").strip():
        return True
    return any(key in payload for key in ("raw_message", "message", "text"))


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


def _install_liteyukibot_compat_modules(sdk):
    liteyuki_root = _ensure_module("liteyuki")
    liteyuki_root.PluginType = PluginType
    liteyuki_root.PluginMetadata = PluginMetadata
    liteyuki_root.MessageEvent = MessageEvent
    liteyuki_root.on_message = on_message
    liteyuki_root.on_startswith = on_startswith
    liteyuki_root.is_su_rule = is_su_rule
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

