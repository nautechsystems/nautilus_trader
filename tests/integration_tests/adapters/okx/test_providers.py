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

from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


@pytest.mark.asyncio
async def test_load_all_async_populates_provider(monkeypatch, instrument):
    # Arrange
    mock_http_client = MagicMock()
    pyo3_instruments = [MagicMock(name="py_inst")]
    mock_http_client.request_instruments = AsyncMock(return_value=pyo3_instruments)

    provider = OKXInstrumentProvider(
        client=mock_http_client,
        instrument_types=(nautilus_pyo3.OKXInstrumentType.SPOT,),
    )

    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.providers.instruments_from_pyo3",
        lambda _values: [instrument],
    )

    # Act
    await provider.load_all_async()

    # Assert
    mock_http_client.request_instruments.assert_awaited_once_with(
        nautilus_pyo3.OKXInstrumentType.SPOT,
        None,
    )
    assert provider.instruments_pyo3() == pyo3_instruments
    assert provider.get_all().get(instrument.id) is instrument


@pytest.mark.asyncio
async def test_load_ids_async_filters_results(monkeypatch, instrument):
    # Arrange
    mock_http_client = MagicMock()
    pyo3_instruments = [MagicMock(name="py_a"), MagicMock(name="py_b")]
    mock_http_client.request_instruments = AsyncMock(return_value=pyo3_instruments)

    provider = OKXInstrumentProvider(
        client=mock_http_client,
        instrument_types=(nautilus_pyo3.OKXInstrumentType.SPOT,),
    )

    btc = instrument.base_currency
    usd = instrument.quote_currency
    other_instrument = type(instrument)(
        instrument_id=InstrumentId(Symbol("ETH-USD"), OKX_VENUE),
        raw_symbol=Symbol("ETH-USD"),
        base_currency=btc,
        quote_currency=usd,
        price_precision=2,
        size_precision=4,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.0001"),
        ts_event=0,
        ts_init=0,
    )

    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.providers.instruments_from_pyo3",
        lambda _values: [instrument, other_instrument],
    )

    # Act
    await provider.load_ids_async([instrument.id])

    # Assert
    mock_http_client.request_instruments.assert_awaited_once_with(
        nautilus_pyo3.OKXInstrumentType.SPOT,
        None,
    )
    assert provider.get_all().get(instrument.id) is instrument
    assert provider.get_all().get(other_instrument.id) is None
    assert provider.instruments_pyo3() == pyo3_instruments
