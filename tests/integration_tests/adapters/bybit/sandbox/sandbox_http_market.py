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

from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoGetParams
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.klines import BybitKlinesGetParams
from nautilus_trader.adapters.bybit.endpoints.market.server_time import BybitServerTimeEndpoint
from nautilus_trader.adapters.bybit.factories import get_bybit_http_client
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.common.component import LiveClock
from tests.integration_tests.adapters.bybit.utils.save_struct_to_file import save_struct_to_file


force_create = True if "FORCE_CREATE" in os.environ else False
base_path = "../resources/http_responses/"
base_endpoint = "/v5/market/"


@pytest.fixture(scope="module")
def client() -> BybitHttpClient:
    clock = LiveClock()

    client = get_bybit_http_client(
        clock=clock,
        is_testnet=True,
    )
    return client


@pytest.mark.asyncio()
async def test_sandbox_get_server_time(client: BybitHttpClient) -> None:
    time_endpoint = BybitServerTimeEndpoint(client=client, base_endpoint=base_endpoint)
    server_time = await time_endpoint.get()
    save_struct_to_file(base_path + "server_time.json", server_time, force_create)


@pytest.mark.asyncio()
async def test_sandbox_get_instruments(client: BybitHttpClient) -> None:
    # --- Spot ---
    instruments_spot_endpoint = BybitInstrumentsInfoEndpoint(
        client,
        base_endpoint,
    )
    instruments_spot = await instruments_spot_endpoint.get(
        BybitInstrumentsInfoGetParams(category=BybitProductType.SPOT),
    )
    result_list_spot = [
        item for item in instruments_spot.result.list if item.symbol in ["BTCUSDT", "ETHUSDT"]
    ]
    save_struct_to_file(base_path + "spot/" + "instruments.json", result_list_spot, force_create)

    # --- Linear ---
    instruments_linear_endpoint = BybitInstrumentsInfoEndpoint(
        client,
        base_endpoint,
    )
    instruments_linear = await instruments_linear_endpoint.get(
        BybitInstrumentsInfoGetParams(category=BybitProductType.LINEAR),
    )
    result_list_linear = [
        item for item in instruments_linear.result.list if item.symbol in ["BTCUSDT", "ETHUSDT"]
    ]
    save_struct_to_file(
        base_path + "linear/" + "instruments.json",
        result_list_linear,
        force_create,
    )

    # --- Option ---
    instruments_option_endpoint = BybitInstrumentsInfoEndpoint(
        client,
        base_endpoint,
    )
    instruments_options = await instruments_option_endpoint.get(
        BybitInstrumentsInfoGetParams(category=BybitProductType.OPTION),
    )
    # take first few items
    instruments_options.result.list = instruments_options.result.list[:2]
    save_struct_to_file(
        base_path + "option/" + "instruments.json",
        instruments_options,
        force_create,
    )


@pytest.mark.asyncio()
async def test_sandbox_get_klines(client: BybitHttpClient) -> None:
    klines_endpoint = BybitKlinesEndpoint(client, base_endpoint)
    btc_spot_klines = await klines_endpoint.get(
        BybitKlinesGetParams(
            category="spot",
            symbol="BTCUSDT",
            interval=BybitKlineInterval.DAY_1,
            limit=3,
        ),
    )
    btc_futures_klines = await klines_endpoint.get(
        BybitKlinesGetParams(
            category="linear",
            symbol="BTCUSDT",
            interval=BybitKlineInterval.DAY_1,
            limit=3,
        ),
    )
    save_struct_to_file(base_path + "spot/" + "klines_btc.json", btc_spot_klines, force_create)
    save_struct_to_file(base_path + "linear/" + "klines_btc.json", btc_futures_klines, force_create)
