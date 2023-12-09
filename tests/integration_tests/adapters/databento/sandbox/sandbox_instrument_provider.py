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

import os

import databento
import pytest

from nautilus_trader.adapters.databento.factories import get_cached_databento_http_client
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import InstrumentId


@pytest.mark.asyncio()
async def test_binance_futures_testnet_market_http_client():
    clock = LiveClock()

    key = os.getenv("DATABENTO_API_KEY")

    http_client = get_cached_databento_http_client(
        key=key,
        # gateway=gateway,
    )

    live_client = databento.Live(
        key=key,
        # gateway=gateway,
    )

    provider = DatabentoInstrumentProvider(
        http_client=http_client,
        live_client=live_client,
        clock=clock,
        logger=Logger(clock=clock),
    )

    instrument_ids = [
        InstrumentId.from_str("ESM2.GLBX"),
    ]

    provider.load_ids(instrument_ids)
