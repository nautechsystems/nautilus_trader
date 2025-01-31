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

from nautilus_trader.adapters.databento.factories import get_cached_databento_http_client
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core import nautilus_pyo3


async def test_databento_instrument_provider():
    http_client = get_cached_databento_http_client()
    clock = LiveClock()

    provider = DatabentoInstrumentProvider(
        http_client=http_client,
        clock=clock,
    )

    # await provider.load_async(InstrumentId.from_str("ESH4.GLBX"))

    # SR3Z4.CME SR3M5.CME and CLM5.CME
    instrument_ids = [
        nautilus_pyo3.InstrumentId.from_str("SR3Z4.GLBX"),
        nautilus_pyo3.InstrumentId.from_str("SR3M5.GLBX"),
        nautilus_pyo3.InstrumentId.from_str("CLM5.GLBX"),
        # nautilus_pyo3.InstrumentId.from_str("AAPL.XNAS"),
    ]
    await provider.load_ids_async(instrument_ids)
    instruments = provider.list_all()

    # instruments = await provider.get_range(
    #     instrument_ids=instrument_ids,
    #     start=(pd.Timestamp.utcnow() - pd.Timedelta(days=5)).date().isoformat(),
    # )

    print(instruments)
    await asyncio.sleep(1.0)


if __name__ == "__main__":
    asyncio.run(test_databento_instrument_provider())
