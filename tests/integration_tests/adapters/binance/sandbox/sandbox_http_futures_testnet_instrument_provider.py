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
import os

import pytest

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


@pytest.mark.asyncio()
async def test_binance_futures_testnet_instrument_provider():
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client = get_cached_binance_http_client(
        loop=loop,
        clock=clock,
        logger=Logger(clock=clock),
        account_type=BinanceAccountType.FUTURES_USDT,
        key=os.getenv("BINANCE_FUTURES_TESTNET_API_KEY"),
        secret=os.getenv("BINANCE_FUTURES_TESTNET_API_SECRET"),
        is_testnet=True,
    )
    await client.connect()

    provider = BinanceFuturesInstrumentProvider(
        client=client,
        clock=clock,
        logger=Logger(clock=clock),
    )

    # await provider.load_all_async()
    btcusdt_perp = InstrumentId(Symbol("BTCUSDT-PERP"), BINANCE_VENUE)
    await provider.load_ids_async(instrument_ids=[btcusdt_perp])
    await provider.load_all_async()

    print(provider.count)

    await client.disconnect()
