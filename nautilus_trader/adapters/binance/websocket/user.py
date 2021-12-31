# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Callable

from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


class BinanceUserDataWebSocket(BinanceWebSocketClient):
    """
    Provides access to the `Binance User Data` streaming WebSocket API.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        handler: Callable[[bytes], None],
    ):
        super().__init__(
            loop=loop,
            clock=clock,
            logger=logger,
            handler=handler,
            base_url="wss://stream.binance.com:9443",
        )

    def subscribe(self, key: str):
        """
        Subscribe to the user data stream.

        Parameters
        ----------
        key : str
            The listen key for the subscription.

        """
        self._add_stream(key)
