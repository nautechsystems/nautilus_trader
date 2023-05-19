# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
# -------------------------------------------------------------------------------------------------

import asyncio

import pytest

from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.asyncio()
async def test_binance_websocket_client():
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client = BinanceWebSocketClient(
        loop=loop,
        clock=clock,
        logger=Logger(clock=clock),
        handler=print,
        base_url="wss://fstream.binance.com",
    )

    client.subscribe_book_ticker("BTCUSDT-PERP")

    await client.connect(start=True)
    await asyncio.sleep(4)
    await client.close()
