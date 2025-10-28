# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio()
async def test_binance_websocket_client():
    clock = LiveClock()

    loop = asyncio.get_running_loop()

    client = BinanceWebSocketClient(
        clock=clock,
        handler=print,
        base_url="wss://fstream.binance.com",
        loop=loop,
    )

    await client.connect()
    await client.subscribe_book_ticker("BTCUSDT-PERP")

    await asyncio.sleep(4)
    await client.disconnect()
