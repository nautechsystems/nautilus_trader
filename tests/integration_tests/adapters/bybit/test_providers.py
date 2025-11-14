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

from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
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

    provider = BybitInstrumentProvider(
        client=mock_http_client,
        product_types=(nautilus_pyo3.BybitProductType.LINEAR,),
    )

    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.providers.instruments_from_pyo3",
        lambda _values: [instrument],
    )

    # Act
    await provider.load_all_async()

    # Assert
    mock_http_client.request_instruments.assert_awaited_once_with(
        nautilus_pyo3.BybitProductType.LINEAR,
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

    provider = BybitInstrumentProvider(
        client=mock_http_client,
        product_types=(nautilus_pyo3.BybitProductType.SPOT,),
    )

    btc = instrument.base_currency
    usd = instrument.quote_currency
    other_instrument = type(instrument)(
        instrument_id=InstrumentId(Symbol("ETHUSDT-SPOT"), BYBIT_VENUE),
        raw_symbol=Symbol("ETHUSDT"),
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
        "nautilus_trader.adapters.bybit.providers.instruments_from_pyo3",
        lambda _values: [instrument, other_instrument],
    )

    # Act
    await provider.load_ids_async([instrument.id])

    # Assert
    mock_http_client.request_instruments.assert_awaited_once_with(
        nautilus_pyo3.BybitProductType.SPOT,
        "BTCUSDT",
    )
    assert provider.get_all().get(instrument.id) is instrument
    assert provider.get_all().get(other_instrument.id) is None
    assert provider.instruments_pyo3() == pyo3_instruments


@pytest.mark.asyncio
async def test_load_ids_async_propagates_exceptions(instrument):
    # Arrange
    mock_http_client = MagicMock()
    mock_http_client.request_instruments = AsyncMock(
        side_effect=RuntimeError("Network error"),
    )

    provider = BybitInstrumentProvider(
        client=mock_http_client,
        product_types=(nautilus_pyo3.BybitProductType.SPOT,),
    )

    # Act & Assert
    with pytest.raises(RuntimeError, match="Network error"):
        await provider.load_ids_async([instrument.id])


@pytest.mark.asyncio
async def test_load_ids_async_rejects_unconfigured_product_type(instrument):
    # Arrange
    mock_http_client = MagicMock()
    mock_http_client.request_instruments = AsyncMock(return_value=[])

    # Provider configured for LINEAR only
    provider = BybitInstrumentProvider(
        client=mock_http_client,
        product_types=(nautilus_pyo3.BybitProductType.LINEAR,),
    )

    # Act & Assert - trying to load SPOT instrument should fail
    with pytest.raises(
        ValueError,
        match=r"has product type.*Spot.*which is not in configured product types",
    ):
        await provider.load_ids_async([instrument.id])

    # Verify no HTTP request was made
    mock_http_client.request_instruments.assert_not_awaited()


@pytest.mark.asyncio
async def test_load_ids_async_handles_option_without_suffix(monkeypatch):
    # Arrange
    mock_http_client = MagicMock()
    pyo3_instruments = [MagicMock(name="py_option")]
    mock_http_client.request_instruments = AsyncMock(return_value=pyo3_instruments)

    provider = BybitInstrumentProvider(
        client=mock_http_client,
        product_types=(nautilus_pyo3.BybitProductType.OPTION,),
    )

    # Option instrument without -OPTION suffix (legacy or manually created)
    option_id = InstrumentId.from_str("BTC-280325-100000-C.BYBIT")

    # Create a mock option instrument that will be returned
    from nautilus_trader.model.currencies import BTC
    from nautilus_trader.model.currencies import USDC
    from nautilus_trader.model.enums import OptionKind
    from nautilus_trader.model.instruments import CryptoOption

    option_instrument = CryptoOption(
        instrument_id=option_id,
        raw_symbol=Symbol("BTC-280325-100000-C"),
        underlying=BTC,
        quote_currency=USDC,
        settlement_currency=USDC,
        is_inverse=False,
        activation_ns=0,
        expiration_ns=0,
        strike_price=Price.from_str("100000"),
        option_kind=OptionKind.CALL,
        price_precision=2,
        size_precision=3,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.001"),
        ts_event=0,
        ts_init=0,
    )

    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.providers.instruments_from_pyo3",
        lambda _values: [option_instrument],
    )

    # Act
    await provider.load_ids_async([option_id])

    # Assert - should try with OPTION product type
    mock_http_client.request_instruments.assert_awaited_once_with(
        nautilus_pyo3.BybitProductType.OPTION,
        "BTC-280325-100000-C",
    )
    assert provider.get_all().get(option_instrument.id) is option_instrument
