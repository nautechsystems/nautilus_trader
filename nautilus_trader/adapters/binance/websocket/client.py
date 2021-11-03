# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

import asyncio
from typing import Callable, List

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.websocket import WebSocketClient


class BinanceWebSocketClient(WebSocketClient):
    """
    Provides a `Binance` streaming WebSocket client.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        handler: Callable[[bytes], None],
        base_url: str,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
            handler=handler,
        )

        self._base_url = base_url
        self._clock = clock
        self._streams: List[str] = []

    @property
    def subscriptions(self):
        return self._streams.copy()

    @property
    def has_subscriptions(self):
        if self._streams:
            return True
        else:
            return False

    async def connect(
        self,
        start: bool = True,
        **ws_kwargs,
    ) -> None:
        if not self._streams:
            raise RuntimeError("No subscriptions for connection.")

        # Always connecting combined streams for consistency
        ws_url = self._base_url + "/stream?streams=" + "/".join(self._streams)
        await super().connect(ws_url=ws_url, start=start, **ws_kwargs)

    def _add_stream(self, stream: str):
        if stream not in self._streams:
            self._streams.append(stream)
