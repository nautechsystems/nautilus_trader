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

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.factories import get_bybit_http_client
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.identifiers import InstrumentId


@pytest.mark.asyncio()
async def test_bybit_instrument_provider():
    clock = LiveClock()
    client = get_bybit_http_client(
        clock=clock,
        key=os.getenv("BYBIT_API_KEY"),
        secret=os.getenv("BYBIT_API_SECRET"),
        is_testnet=False,
    )

    provider = BybitInstrumentProvider(
        client=client,
        clock=clock,
        product_types=[
            BybitProductType.SPOT,
            BybitProductType.LINEAR,
            BybitProductType.INVERSE,
            BybitProductType.OPTION,
        ],
    )

    # await provider.load_all_async()
    ethusdt_linear = InstrumentId.from_str("ETHUSDT-LINEAR.BYBIT")
    await provider.load_ids_async(instrument_ids=[ethusdt_linear])
    await provider.load_all_async()

    print(provider.list_all())
    print(provider.count)
