# -*- coding: utf-8 -*-
"""
Copyright (C) 2020-2024 LiteyukiStudio. All Rights Reserved 

@Time    : 2024/8/22 下午12:31
@Author  : snowykami
@Email   : snowykami@outlook.com
@File    : liteecho.py
@Software: PyCharm
"""

from liteyuki.session.on import on_startswith
from liteyuki.session.event import MessageEvent
from liteyuki.session.rule import is_su_rule
from liteyuki_sdk import sdk


def _read_config():
    if sdk is None:
        return {}
    return {
        "enabled": bool(sdk.config_get("enabled")),
        "reply_prefix": str(sdk.config_get("reply_prefix") or ""),
        "reply_suffix": str(sdk.config_get("reply_suffix") or ""),
        "trim_input": bool(sdk.config_get("trim_input")),
    }


@on_startswith(["liteecho"], rule=is_su_rule).handle()
async def liteecho(event: MessageEvent):
    config = _read_config()
    if not config.get("enabled", True):
        return

    message = event.raw_message.strip()
    for prefix in ("/liteecho", "liteecho"):
        if message.startswith(prefix):
            content = message[len(prefix):]
            if config.get("trim_input", True):
                content = content.strip()
            event.reply(
                f"{config.get('reply_prefix', '')}{content}{config.get('reply_suffix', '')}"
            )
            return
