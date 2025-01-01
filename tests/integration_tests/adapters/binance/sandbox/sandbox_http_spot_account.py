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
from nautilus_trader.adapters.binance.spot.http.account import BinanceSpotAccountHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio()
async def test_binance_spot_account_http_client():
    clock = LiveClock()

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=BinanceAccountType.SPOT,
        api_key=os.getenv("BINANCE_API_KEY"),
        api_secret=os.getenv("BINANCE_API_SECRET"),
    )

    http_account = BinanceSpotAccountHttpAPI(client=client)

    ############################################################################
    # ACCOUNT STATUS
    ############################################################################
    response = await http_account.account(recv_window=5000)
    print(response)
    # print(json.dumps(response, indent=4))

    ############################################################################
    # NEW ORDER
    ############################################################################
    # response = await http_account.new_order(
    #     symbol="ETHUSDT",
    #     side="SELL",
    #     type="MARKET",
    #     quantity="0.005",
    #     # time_in_force="GTC",
    #     # price="4300",
    #     # iceberg_qty="0.005",
    #     # stop_price="4200",
    #     # new_client_order_id="Z01",
    #     recv_window=5000,
    # )
    # print(response)
    # response = await http_account.cancel_order(
    #     symbol="ETHUSDT",
    #     orig_client_order_id="TLgKPsHY1f0rMqBDIOa24Y",
    #     #new_client_order_id=str(uuid.uuid4()),
    #     recv_window=5000,
    # )
    # print(response)
