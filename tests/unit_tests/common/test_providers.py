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

from unittest.mock import MagicMock

import pytest

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD = TestIdStubs.audusd_id()
GBPUSD = TestIdStubs.gbpusd_id()
USDJPY = TestIdStubs.usdjpy_id()


def _make_instrument(instrument_id: InstrumentId) -> Instrument:
    """
    Create a mock instrument with the given ID.
    """
    mock = MagicMock(spec=Instrument)
    mock.id = instrument_id
    return mock


class _StubProvider(InstrumentProvider):
    """
    Concrete provider that records load_all_async calls and adds pre-configured
    instruments.
    """

    def __init__(self, instruments: list[Instrument] | None = None) -> None:
        super().__init__()
        self._stub_instruments = instruments or []
        self.load_all_calls: list[dict | None] = []

    async def load_all_async(self, filters: dict | None = None) -> None:
        self.load_all_calls.append(filters)
        for inst in self._stub_instruments:
            self.add(inst)


class TestInstrumentProvider:
    def setup(self):
        # Fixture Setup
        self.provider = InstrumentProvider()

    def test_get_all_when_no_instruments_returns_empty_dict(self):
        # Arrange, Act
        result = self.provider.get_all()

        # Assert
        assert result == {}

    def test_find_when_no_instruments_returns_none(self):
        # Arrange, Act
        result = self.provider.find(AUDUSD)

        # Assert
        assert result is None


class TestLoadIdsAsync:
    @pytest.mark.asyncio
    async def test_empty_list_returns_immediately(self):
        # Arrange
        provider = _StubProvider()

        # Act
        await provider.load_ids_async([])

        # Assert
        assert provider.load_all_calls == []
        assert provider.get_all() == {}

    @pytest.mark.asyncio
    async def test_single_id_retains_only_requested(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        inst_b = _make_instrument(GBPUSD)
        provider = _StubProvider(instruments=[inst_a, inst_b])

        # Act
        await provider.load_ids_async([AUDUSD])

        # Assert
        assert provider.find(AUDUSD) is inst_a
        assert provider.find(GBPUSD) is None

    @pytest.mark.asyncio
    async def test_multiple_ids_retains_only_requested(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        inst_b = _make_instrument(GBPUSD)
        inst_c = _make_instrument(USDJPY)
        provider = _StubProvider(instruments=[inst_a, inst_b, inst_c])

        # Act
        await provider.load_ids_async([AUDUSD, USDJPY])

        # Assert
        assert provider.find(AUDUSD) is inst_a
        assert provider.find(USDJPY) is inst_c
        assert provider.find(GBPUSD) is None

    @pytest.mark.asyncio
    async def test_requested_id_not_in_loaded_gives_empty(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        provider = _StubProvider(instruments=[inst_a])

        # Act
        await provider.load_ids_async([GBPUSD])

        # Assert
        assert provider.find(GBPUSD) is None
        assert provider.find(AUDUSD) is None

    @pytest.mark.asyncio
    async def test_preserves_previously_loaded_instruments(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        inst_b = _make_instrument(GBPUSD)
        provider = _StubProvider(instruments=[inst_a, inst_b])
        provider.add(inst_a)

        # Act
        await provider.load_ids_async([GBPUSD])

        # Assert
        assert provider.find(GBPUSD) is inst_b
        assert provider.find(AUDUSD) is inst_a

    @pytest.mark.asyncio
    async def test_successive_calls_accumulate(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        inst_b = _make_instrument(GBPUSD)
        provider = _StubProvider(instruments=[inst_a, inst_b])

        # Act
        await provider.load_ids_async([AUDUSD])
        await provider.load_ids_async([GBPUSD])

        # Assert
        assert provider.find(AUDUSD) is inst_a
        assert provider.find(GBPUSD) is inst_b
        assert len(provider.load_all_calls) == 2

    @pytest.mark.asyncio
    async def test_filters_passed_to_load_all(self):
        # Arrange
        provider = _StubProvider()
        filters = {"exchange": "SIM"}

        # Act
        await provider.load_ids_async([AUDUSD], filters=filters)

        # Assert
        assert provider.load_all_calls == [filters]

    @pytest.mark.asyncio
    async def test_exception_propagates(self):
        # Arrange
        provider = _StubProvider()
        provider.load_all_async = MagicMock(side_effect=RuntimeError("API error"))

        # Act & Assert
        with pytest.raises(RuntimeError, match="API error"):
            await provider.load_ids_async([AUDUSD])

    @pytest.mark.asyncio
    async def test_duplicate_ids_handled(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        provider = _StubProvider(instruments=[inst_a])

        # Act
        await provider.load_ids_async([AUDUSD, AUDUSD])

        # Assert
        assert provider.find(AUDUSD) is inst_a
        assert provider.count == 1


class TestLoadAsync:
    @pytest.mark.asyncio
    async def test_instrument_already_loaded_skips_fetch(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        provider = _StubProvider()
        provider.add(inst_a)

        # Act
        await provider.load_async(AUDUSD)

        # Assert
        assert provider.load_all_calls == []
        assert provider.find(AUDUSD) is inst_a

    @pytest.mark.asyncio
    async def test_instrument_not_loaded_fetches(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        inst_b = _make_instrument(GBPUSD)
        provider = _StubProvider(instruments=[inst_a, inst_b])

        # Act
        await provider.load_async(AUDUSD)

        # Assert
        assert provider.find(AUDUSD) is inst_a
        assert provider.find(GBPUSD) is None
        assert len(provider.load_all_calls) == 1

    @pytest.mark.asyncio
    async def test_filters_passed_through(self):
        # Arrange
        inst_a = _make_instrument(AUDUSD)
        provider = _StubProvider(instruments=[inst_a])
        filters = {"exchange": "SIM"}

        # Act
        await provider.load_async(AUDUSD, filters=filters)

        # Assert
        assert provider.load_all_calls == [filters]

    @pytest.mark.asyncio
    async def test_exception_propagates(self):
        # Arrange
        provider = _StubProvider()
        provider.load_all_async = MagicMock(side_effect=RuntimeError("API error"))

        # Act & Assert
        with pytest.raises(RuntimeError, match="API error"):
            await provider.load_async(AUDUSD)
