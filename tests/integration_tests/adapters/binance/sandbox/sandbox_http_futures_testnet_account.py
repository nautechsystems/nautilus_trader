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

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.asyncio()
async def test_binance_spot_account_http_client():
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client = get_cached_binance_http_client(
        loop=loop,
        clock=clock,
        logger=Logger(clock=clock),
        account_type=BinanceAccountType.FUTURES_USDT,
        key=os.getenv("BINANCE_FUTURES_TESTNET_API_KEY"),
        secret=os.getenv("BINANCE_FUTURES_TESTNET_API_SECRET"),
    )
    await client.connect()

    http_account = BinanceFuturesAccountHttpAPI(
        client=client,
        account_type=BinanceAccountType.FUTURES_USDT,
    )

    info = await http_account.account()
    print(info)
    trades = await http_account.get_account_trades(symbol="ETHUSDT")

    ############################################################################
    # NEW ORDER
    ############################################################################
    # response = await account.new_order_futures(
    #     symbol="ETHUSDT",
    #     side="SELL",
    #     type="LIMIT",
    #     quantity="0.01",
    #     time_in_force="GTC",
    #     price="3000",
    #     # iceberg_qty="0.005",
    #     # stop_price="3200",
    #     # working_type="CONTRACT_PRICE",
    #     # new_client_order_id="O-20211120-021300-001-001-1",
    #     recv_window=5000,
    # )

    ############################################################################
    # NEW ORDER
    ############################################################################
    # response = await account.new_order_futures(
    #     symbol="ETHUSDT",
    #     side="SELL",
    #     type="TAKE_PROFIT_MARKET",
    #     quantity="0.01",
    #     time_in_force="GTC",
    #     # price="3000",
    #     # iceberg_qty="0.005",
    #     stop_price="3200",
    #     working_type="CONTRACT_PRICE",
    #     # new_client_order_id="O-20211120-021300-001-001-1",
    #     recv_window=5000,
    # )

    ############################################################################
    # CANCEL ORDER
    ############################################################################
    # response = await account.cancel_order(
    #     symbol="ETHUSDT",
    #     orig_client_order_id="9YDq1gEAGjBkZmMbTSX1ww",
    #     # new_client_order_id=str(uuid.uuid4()),
    #     recv_window=5000,
    # )

    print(trades)

    await client.disconnect()
