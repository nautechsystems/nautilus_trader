# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import InstrumentId


async def test_databento_instrument_provider():
    http_client = get_cached_databento_http_client()
    clock = LiveClock()

    provider = DatabentoInstrumentProvider(
        http_client=http_client,
        clock=clock,
        logger=Logger(
            clock=clock,
            level_stdout=LogLevel.DEBUG,
        ),
    )

    instrument_ids = [
        InstrumentId.from_str("ESZ3.GLBX"),
        InstrumentId.from_str("ESH4.GLBX"),
        InstrumentId.from_str("ESM4.GLBX"),
    ]
    await provider.load_ids_async(instrument_ids)


if __name__ == "__main__":
    asyncio.run(test_databento_instrument_provider())
