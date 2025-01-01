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

import asyncio

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.common.component import LiveClock


async def test_polymarket_instrument_provider():
    clock = LiveClock()
    client = get_polymarket_http_client()

    provider = PolymarketInstrumentProvider(
        client=client,
        clock=clock,
    )

    filters = {
        "next_cursor": "MTEyMDA=",
        "is_active": True,
    }

    await provider.load_all_async(filters=filters)

    instruments = provider.list_all()
    await provider.load_async(instruments[0].id)


if __name__ == "__main__":
    asyncio.run(test_polymarket_instrument_provider())
