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

import json

import msgspec
import pytest

from nautilus_trader.adapters.bybit.factories import get_bybit_http_client
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio()
async def test_bybit_account_http_client():
    clock = LiveClock()

    client = get_bybit_http_client(
        clock=clock,
        is_testnet=True,
    )

    http_account = BybitAccountHttpAPI(
        clock=clock,
        client=client,
    )

    ################################################################################
    # Account balance
    ################################################################################
    account_balance = await http_account.query_wallet_balance()
    for item in account_balance:
        print(json.dumps(msgspec.to_builtins(item), indent=4))
