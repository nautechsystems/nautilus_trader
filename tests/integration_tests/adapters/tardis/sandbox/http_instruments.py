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

import pandas as pd

from nautilus_trader.adapters.tardis.factories import get_tardis_http_client
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import CryptoPerpetual


async def run():
    nautilus_pyo3.init_tracing()
    _guard = init_logging(level_stdout=LogLevel.TRACE)

    http_client = get_tardis_http_client()

    # pyo3_instrument = await http_client.instrument("okex", "ETH-USDT")
    # print(f"Received: {pyo3_instrument[0].id}")

    pyo3_instruments = await http_client.instruments(
        "bitmex",
        base_currency=["BTC"],
        quote_currency=["USD"],
        instrument_type=["perpetual"],
        # active=True,
        # start=pd.Timestamp("2021-01-01").value,
        # end=pd.Timestamp("2022-01-01").value,
        effective=pd.Timestamp("2020-08-01 08:00:00").value,
    )

    for pyo3_inst in pyo3_instruments:
        inst = CryptoPerpetual.from_pyo3(pyo3_inst)  # Remove/change this if not filtering for perps
        print(repr(inst))
        print(pd.Timestamp(inst.ts_event))

    print(f"Received: {len(pyo3_instruments)} instruments")


if __name__ == "__main__":
    asyncio.run(run())
