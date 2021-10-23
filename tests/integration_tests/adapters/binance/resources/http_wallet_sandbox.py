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
# -------------------------------------------------------------------------------------------------

import asyncio
import json
import os

import pytest

from nautilus_trader.adapters.binance.factories import get_binance_http_client
from nautilus_trader.adapters.binance.http.api.wallet import BinanceWalletHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.asyncio
async def test_binance_spot_wallet_http_client():
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client = get_binance_http_client(
        loop=loop,
        clock=clock,
        logger=Logger(clock=clock),
        key=os.getenv("BINANCE_API_KEY"),
        secret=os.getenv("BINANCE_API_SECRET"),
    )

    wallet = BinanceWalletHttpAPI(client=client)
    await client.connect()
    response = await wallet.trade_fee(symbol="BTCUSDT")
    print(json.dumps(json.loads(response), indent=4))

    await client.disconnect()
