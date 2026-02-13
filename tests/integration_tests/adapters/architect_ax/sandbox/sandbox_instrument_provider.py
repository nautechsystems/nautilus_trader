# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.architect_ax.factories import get_cached_ax_http_client
from nautilus_trader.adapters.architect_ax.providers import AxInstrumentProvider


async def test_ax_instrument_provider():
    client = get_cached_ax_http_client()

    provider = AxInstrumentProvider(client=client)

    await provider.load_all_async()

    instruments = provider.list_all()
    print(f"Loaded {len(instruments)} instruments")

    for instrument in instruments:
        print(instrument)


if __name__ == "__main__":
    asyncio.run(test_ax_instrument_provider())
