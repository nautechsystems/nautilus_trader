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

from __future__ import annotations

import pytest

from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


class StubLighterHttpClient:
    def __init__(self, instruments, market_indices: dict[str, int]) -> None:
        self._instruments = instruments
        self._market_indices = market_indices

    async def load_instrument_definitions(self):
        return self._instruments

    def get_market_index(self, instrument_id: InstrumentId) -> int | None:
        return self._market_indices.get(getattr(instrument_id, "value", str(instrument_id)))


@pytest.mark.asyncio
async def test_load_all_caches_market_index() -> None:
    instrument = TestInstrumentProviderPyo3.ethusdt_perp_binance()
    client = StubLighterHttpClient([instrument], {instrument.id.value: 7})

    provider = LighterInstrumentProvider(client, InstrumentProviderConfig())
    await provider.load_all_async()

    assert provider.market_index_for(instrument.id) == 7
    assert provider.instruments_pyo3() == [instrument]
    assert provider.find(instrument.id) is not None


@pytest.mark.asyncio
async def test_filters_market_indices() -> None:
    instrument = TestInstrumentProviderPyo3.ethusdt_perp_binance()
    client = StubLighterHttpClient([instrument], {instrument.id.value: 3})

    provider = LighterInstrumentProvider(client, InstrumentProviderConfig())
    await provider.load_all_async(filters={"market_indices": ["2"]})

    assert provider.market_index_for(instrument.id) is None
    assert provider.count == 0
