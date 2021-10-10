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
import json
from typing import Callable

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
        ws_url: str,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
            handler=handler,
            ws_url=ws_url,
        )

        self._clock = clock

    def stop(self):
        pass
        # try:
        #     self.close()
        # finally:
        #     reactor.stop()

    def _single_stream(self, stream):
        if isinstance(stream, str):
            return True
        elif isinstance(stream, list):
            return False
        else:
            raise ValueError("Invalid stream name, expect string or array")

    def live_subscribe(self, stream, id, **kwargs):
        # """
        # Live subscribe WebSocket.
        #
        # Connect to the server:
        #  - SPOT: wss://stream.binance.com:9443/ws
        #  - SPOT testnet : wss://testnet.binance.vision/ws
        # and sending the subscribe message, e.g.
        # {"method": "SUBSCRIBE","params":["btcusdt@miniTicker"],"id": 100}
        # """
        # combined = False
        # if self._single_stream(stream):
        #     stream = [stream]
        # else:
        #     combined = True

        data = {"method": "SUBSCRIBE", "params": stream, "id": id}

        data.update(**kwargs)
        payload = json.dumps(data, ensure_ascii=False).encode("utf8")
        stream_name = "-".join(stream)
        print(payload)
        print(stream_name)
        # return self._start_socket(
        #     stream_name, payload, callback, is_combined=combined, is_live=True
        # )

    def instant_subscribe(self, stream, **kwargs):
        # """Instant subscribe, e.g.
        # wss://stream.binance.com:9443/ws/btcusdt@bookTicker
        # wss://stream.binance.com:9443/stream?streams=btcusdt@bookTicker/bnbusdt@bookTicker
        # """
        # combined = False
        # if not self._single_stream(stream):
        #     combined = True
        #     stream = "/".join(stream)

        data = {"method": "SUBSCRIBE", "params": stream}

        data.update(**kwargs)
        payload = json.dumps(data, ensure_ascii=False).encode("utf8")
        stream_name = "-".join(stream)
        print(payload)
        print(stream_name)
        # return self._start_socket(
        #     stream_name, payload, callback, is_combined=combined, is_live=False
        # )
