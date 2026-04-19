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


@on_startswith(["liteecho"], rule=is_su_rule).handle()
async def liteecho(event: MessageEvent):
    message = event.raw_message.strip()
    for prefix in ("/liteecho", "liteecho"):
        if message.startswith(prefix):
            event.reply(message[len(prefix):].strip())
            return
