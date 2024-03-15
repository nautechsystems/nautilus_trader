# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd

from nautilus_trader.adapters.databento.factories import get_cached_databento_http_client
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.identifiers import InstrumentId


async def test_databento_instrument_provider():
    http_client = get_cached_databento_http_client()
    clock = LiveClock()

    provider = DatabentoInstrumentProvider(
        http_client=http_client,
        clock=clock,
    )

    await provider.load_async(InstrumentId.from_str("ESH4.GLBX"))

    instrument_ids = [
        # InstrumentId.from_str("ESZ3.XCME"),
        InstrumentId.from_str("ESH4.GLBX"),
        InstrumentId.from_str("ESM4.GLBX"),
        # InstrumentId.from_str("AAPL.XNAS"),
    ]
    await provider.load_ids_async(instrument_ids)

    instruments = await provider.get_range(
        instrument_ids=instrument_ids,
        start=(pd.Timestamp.utcnow() - pd.Timedelta(days=5)).date().isoformat(),
    )

    print(instruments)


if __name__ == "__main__":
    asyncio.run(test_databento_instrument_provider())
