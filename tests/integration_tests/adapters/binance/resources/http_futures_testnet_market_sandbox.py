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

from nautilus_trader.adapters.binance.core.enums import BinanceAccountType
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.http.api.market import BinanceMarketHttpAPI
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.asyncio
async def test_binance_futures_testnet_market_http_client():
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client = get_cached_binance_http_client(
        loop=loop,
        clock=clock,
        logger=Logger(clock=clock),
        key=os.getenv("BINANCE_TESTNET_API_KEY"),
        secret=os.getenv("BINANCE_TESTNET_API_SECRET"),
        base_url="https://testnet.binancefuture.com",
        is_testnet=True,
    )
    await client.connect()

    account_type = BinanceAccountType.FUTURES_USDT
    market = BinanceMarketHttpAPI(client=client, account_type=account_type)
    response = await market.exchange_info(symbol="BTCUSDT")
    print(json.dumps(response, indent=4))

    provider = BinanceInstrumentProvider(
        client=client,
        logger=Logger(clock=clock),
        account_type=account_type,
    )

    await provider.load_all_async()

    print(provider.count)

    await client.disconnect()
