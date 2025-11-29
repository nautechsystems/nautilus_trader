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

from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
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

    provider = KrakenInstrumentProvider(
        http_client_spot=mock_http_client,
        product_types=[KrakenProductType.SPOT],
    )

    monkeypatch.setattr(
        "nautilus_trader.adapters.kraken.providers.instruments_from_pyo3",
        lambda _values: [instrument],
    )

    # Act
    await provider.load_all_async()

    # Assert
    mock_http_client.request_instruments.assert_awaited_once()
    assert provider.instruments_pyo3() == pyo3_instruments
    assert provider.get_all().get(instrument.id) is instrument


@pytest.mark.asyncio
async def test_load_ids_async_loads_all_instruments(monkeypatch, instrument, venue):
    # Arrange
    mock_http_client = MagicMock()
    pyo3_instruments = [MagicMock(name="py_a"), MagicMock(name="py_b")]
    mock_http_client.request_instruments = AsyncMock(return_value=pyo3_instruments)

    provider = KrakenInstrumentProvider(
        http_client_spot=mock_http_client,
        product_types=[KrakenProductType.SPOT],
    )

    btc = instrument.base_currency
    usd = instrument.quote_currency
    other_instrument = type(instrument)(
        instrument_id=InstrumentId(Symbol("ETH/USDT"), venue),
        raw_symbol=Symbol("ETHUSDT"),
        base_currency=btc,
        quote_currency=usd,
        price_precision=1,
        size_precision=8,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.00000001"),
        ts_event=0,
        ts_init=0,
    )

    monkeypatch.setattr(
        "nautilus_trader.adapters.kraken.providers.instruments_from_pyo3",
        lambda _values: [instrument, other_instrument],
    )

    # Act
    await provider.load_ids_async([instrument.id])

    # Assert
    mock_http_client.request_instruments.assert_awaited_once()
    assert provider.get_all().get(instrument.id) is instrument
    # Note: Kraken loads all instruments and filters client-side
    assert provider.get_all().get(other_instrument.id) is other_instrument
    assert provider.instruments_pyo3() == pyo3_instruments


@pytest.mark.asyncio
async def test_load_ids_async_propagates_exceptions(instrument):
    # Arrange
    mock_http_client = MagicMock()
    mock_http_client.request_instruments = AsyncMock(
        side_effect=RuntimeError("Network error"),
    )

    provider = KrakenInstrumentProvider(
        http_client_spot=mock_http_client,
        product_types=[KrakenProductType.SPOT],
    )

    # Act & Assert
    with pytest.raises(RuntimeError, match="Network error"):
        await provider.load_ids_async([instrument.id])


@pytest.mark.asyncio
async def test_load_async_delegates_to_load_ids(monkeypatch, instrument):
    # Arrange
    mock_http_client = MagicMock()
    pyo3_instruments = [MagicMock(name="py_inst")]
    mock_http_client.request_instruments = AsyncMock(return_value=pyo3_instruments)

    provider = KrakenInstrumentProvider(
        http_client_spot=mock_http_client,
        product_types=[KrakenProductType.SPOT],
    )

    monkeypatch.setattr(
        "nautilus_trader.adapters.kraken.providers.instruments_from_pyo3",
        lambda _values: [instrument],
    )

    # Act
    await provider.load_async(instrument.id)

    # Assert
    mock_http_client.request_instruments.assert_awaited_once()
    assert provider.get_all().get(instrument.id) is instrument


@pytest.mark.asyncio
async def test_product_types_returns_copy(instrument):
    # Arrange
    mock_http_client = MagicMock()
    provider = KrakenInstrumentProvider(
        http_client_spot=mock_http_client,
        product_types=[KrakenProductType.SPOT],
    )

    # Act
    product_types = provider.product_types
    product_types.append(KrakenProductType.FUTURES)  # Try to modify

    # Assert
    assert provider.product_types == [KrakenProductType.SPOT]  # Should be unchanged
