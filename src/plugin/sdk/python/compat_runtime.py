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


def _ensure_module(name):
    module = sys.modules.get(name)
    if module is None:
        module = types.ModuleType(name)
        sys.modules[name] = module
    return module


def _install_python_compat_modules(sdk):
    _install_liteyukibot_compat_modules(sdk)
    _install_astrbot_compat_modules(sdk)
    _install_neomofox_compat_modules(sdk)
