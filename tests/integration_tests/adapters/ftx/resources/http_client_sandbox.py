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
import json
import os

import pytest

from nautilus_trader.adapters.ftx.factories import get_cached_ftx_http_client
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient

# from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.asyncio
async def test_ftx_http_client():
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client: FTXHttpClient = get_cached_ftx_http_client(
        loop=loop,
        clock=clock,
        logger=Logger(clock=clock),
        key=os.getenv("FTX_API_KEY"),
        secret=os.getenv("FTX_API_SECRET"),
    )
    await client.connect()

    # Test authentication works with account info
    response = await client.get_account_info()
    print(json.dumps(response, indent=4))

    # response = await client.list_markets(
    #     # market="ETH-PERP",
    # )
    # print(json.dumps(response, indent=4))

    # provider = FTXInstrumentProvider(
    #     client=client,
    #     logger=Logger(clock=clock),
    # )
    #
    # await provider.load_all_async()
    # for instrument in provider.get_all().values():
    #     print(instrument)

    # response = await client.get_historical_prices(
    #     market="ETH/USD",
    #     resolution=300,
    # )
    # print(response)

    # response = await client.place_order(
    #     market="ETH/USD",
    #     side="sell",
    #     size="0.01",
    #     type="market",
    #     #price="5500",
    #     client_id="011",
    #     # post_only=True,
    #     # reduce_only=True,
    # )
    # print(json.dumps(response, indent=4))

    await client.disconnect()
