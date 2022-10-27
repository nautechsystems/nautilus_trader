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
# -------------------------------------------------------------------------------------------------

import asyncio
import os

import pytest

from nautilus_trader.adapters.ftx.websocket.client import FTXWebSocketClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger


@pytest.mark.asyncio
async def test_ftx_websocket_client():
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client = FTXWebSocketClient(
        loop=loop,
        clock=clock,
        logger=LiveLogger(loop=loop, clock=clock),
        msg_handler=print,
        reconnect_handler=print,
        key=os.getenv("FTX_API_KEY"),
        secret=os.getenv("FTX_API_SECRET"),
        subaccount=os.getenv("FTX_SUBACCOUNT"),
    )

    await client.connect(start=True)

    # await client.subscribe_markets()
    await client.subscribe_orderbook("ETH-PERP")
    await asyncio.sleep(3)
    await client.disconnect()
    await client.close()
