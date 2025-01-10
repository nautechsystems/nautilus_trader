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

import os

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio()
async def test_binance_spot_account_http_client():
    clock = LiveClock()

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=BinanceAccountType.USDT_FUTURE,
        api_key=os.getenv("BINANCE_FUTURES_TESTNET_API_KEY"),
        api_secret=os.getenv("BINANCE_FUTURES_TESTNET_API_SECRET"),
        is_testnet=True,
    )

    http_account = BinanceFuturesAccountHttpAPI(
        client=client,
        clock=clock,
        account_type=BinanceAccountType.USDT_FUTURE,
    )

    await http_account.query_futures_hedge_mode()

    # trades = await http_account.get_account_trades(symbol="ETHUSDT")

    ############################################################################
    # NEW ORDER
    ############################################################################
    # await http_account.new_order(
    #     symbol="ETHUSDT",
    #     side=BinanceOrderSide.BUY,
    #     order_type=BinanceOrderType.LIMIT,
    #     quantity="0.01",
    #     time_in_force=BinanceTimeInForce.GTC,
    #     price="1000",
    #     # iceberg_qty="0.005",
    #     # stop_price="3200",
    #     # working_type="CONTRACT_PRICE",
    #     # new_client_order_id="O-20211120-021300-001-001-1",
    #     recv_window="5000",
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

    # print(trades)
