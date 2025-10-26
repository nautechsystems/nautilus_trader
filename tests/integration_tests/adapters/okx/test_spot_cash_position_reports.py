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
"""
Tests for OKX SPOT position reports from wallet balances.

These tests verify wallet balance-based position reporting for SPOT CASH (non-leveraged)
trading where `use_spot_margin=False`. For spot margin trading (`use_spot_margin=True`),
the positions API is used instead (same as SWAP/FUTURES instruments).

"""

from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.okx.conftest import _create_ws_mock


@pytest.fixture()
def exec_client_with_spot_positions(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    monkeypatch,
):
    """
    Fixture for execution client with spot position reports enabled for SPOT CASH (non-
    leveraged).

    Uses wallet balance calculation (cash_bal - liab) for position reporting.

    """
    private_ws = _create_ws_mock()
    business_ws = _create_ws_mock()
    ws_iter = iter([private_ws, business_ws])

    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.nautilus_pyo3.OKXWebSocketClient.with_credentials",
        lambda *args, **kwargs: next(ws_iter),
    )

    mock_http_client.reset_mock()
    mock_instrument_provider.initialize.reset_mock()
    mock_instrument_provider.instruments_pyo3.reset_mock()
    mock_instrument_provider.instruments_pyo3.return_value = [MagicMock(name="py_instrument")]

    # Set the mock provider's instrument_types to SPOT
    mock_instrument_provider.instrument_types = (nautilus_pyo3.OKXInstrumentType.SPOT,)

    config = OKXExecClientConfig(
        api_key="test_api_key",
        api_secret="test_api_secret",
        api_passphrase="test_passphrase",
        instrument_types=(nautilus_pyo3.OKXInstrumentType.SPOT,),
        use_spot_cash_position_reports=True,  # Enable spot position reports
        use_spot_margin=False,  # Test non-leveraged spot (wallet balance calculation)
    )

    client = OKXExecutionClient(
        loop=event_loop,
        client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        clock=live_clock,
        instrument_provider=mock_instrument_provider,
        config=config,
        name=None,
    )

    return client, private_ws, business_ws, mock_http_client, mock_instrument_provider


@pytest.mark.asyncio
async def test_spot_position_reports_disabled_by_default_returns_flat(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    monkeypatch,
):
    """
    Test that SPOT position reports are disabled by default and return FLAT reports.
    """
    # Arrange
    private_ws = _create_ws_mock()
    business_ws = _create_ws_mock()
    ws_iter = iter([private_ws, business_ws])

    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.nautilus_pyo3.OKXWebSocketClient.with_credentials",
        lambda *args, **kwargs: next(ws_iter),
    )

    mock_http_client.reset_mock()
    mock_instrument_provider.initialize.reset_mock()
    mock_instrument_provider.instruments_pyo3.return_value = [MagicMock()]
    mock_instrument_provider.instrument_types = (nautilus_pyo3.OKXInstrumentType.SPOT,)

    config = OKXExecClientConfig(
        api_key="test_api_key",
        api_secret="test_api_secret",
        api_passphrase="test_passphrase",
        instrument_types=(nautilus_pyo3.OKXInstrumentType.SPOT,),
        use_spot_cash_position_reports=False,  # Disabled (default)
    )

    client = OKXExecutionClient(
        loop=event_loop,
        client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        clock=live_clock,
        instrument_provider=mock_instrument_provider,
        config=config,
        name=None,
    )

    instrument = TestInstrumentProvider.btcusdt_binance()
    client._cache.add_instrument(instrument)

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    report = reports[0]
    assert report.position_side == PositionSide.FLAT
    assert report.quantity == Quantity.zero(instrument.size_precision)


@pytest.mark.asyncio
async def test_spot_position_reports_long_position_from_positive_balance(
    exec_client_with_spot_positions,
):
    """Test LONG position created from positive wallet balance (cash_bal - liab > 0)."""
    # Arrange
    client, _, _, http_client, instrument_provider = exec_client_with_spot_positions

    instrument = TestInstrumentProvider.btcusdt_binance()
    client._cache.add_instrument(instrument)

    # Mock instrument provider to return the BTC instrument
    instrument_provider.get_all.return_value = {instrument.id: instrument}

    # Mock OKX balance response: cash_bal=1.5 BTC, liab=0 BTC -> net=1.5 BTC LONG
    # http_get_balance returns a flattened list of balance details
    okx_balance_detail = SimpleNamespace(
        ccy="BTC",
        cash_bal="1.5",
        liab="0",
    )
    http_client.http_get_balance = AsyncMock(return_value=[okx_balance_detail])

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    report = reports[0]
    assert report.position_side == PositionSide.LONG
    assert report.quantity == Quantity.from_str("1.5")
    assert report.instrument_id == instrument.id


@pytest.mark.asyncio
async def test_spot_position_reports_short_position_from_negative_balance(
    exec_client_with_spot_positions,
):
    """Test SHORT position created from negative balance (borrowing: cash_bal - liab < 0)."""
    # Arrange
    client, _, _, http_client, instrument_provider = exec_client_with_spot_positions

    instrument = TestInstrumentProvider.ethusdt_binance()
    client._cache.add_instrument(instrument)

    instrument_provider.get_all.return_value = {instrument.id: instrument}

    # Mock OKX balance response: cash_bal=5.0 ETH, liab=8.0 ETH -> net=-3.0 ETH SHORT
    # http_get_balance returns a flattened list of balance details
    okx_balance_detail = SimpleNamespace(
        ccy="ETH",
        cash_bal="5.0",
        liab="8.0",
    )
    http_client.http_get_balance = AsyncMock(return_value=[okx_balance_detail])

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    report = reports[0]
    assert report.position_side == PositionSide.SHORT
    assert report.quantity == Quantity.from_str("3.0")  # Absolute value
    assert report.instrument_id == instrument.id


@pytest.mark.asyncio
async def test_spot_position_reports_flat_position_from_zero_balance(
    exec_client_with_spot_positions,
):
    """Test FLAT position when net balance is zero (cash_bal - liab = 0)."""
    # Arrange
    client, _, _, http_client, instrument_provider = exec_client_with_spot_positions

    instrument = TestInstrumentProvider.btcusdt_binance()
    client._cache.add_instrument(instrument)

    instrument_provider.get_all.return_value = {instrument.id: instrument}

    # Mock OKX balance response: cash_bal=2.0 BTC, liab=2.0 BTC -> net=0 BTC FLAT
    # http_get_balance returns a flattened list of balance details
    okx_balance_detail = SimpleNamespace(
        ccy="BTC",
        cash_bal="2.0",
        liab="2.0",
    )
    http_client.http_get_balance = AsyncMock(return_value=[okx_balance_detail])

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    report = reports[0]
    assert report.position_side == PositionSide.FLAT
    assert report.quantity == Quantity.zero(instrument.size_precision)


@pytest.mark.asyncio
async def test_spot_position_reports_multiple_instruments(
    exec_client_with_spot_positions,
):
    """
    Test generating position reports for multiple instruments with different balances.
    """
    # Arrange
    client, _, _, http_client, instrument_provider = exec_client_with_spot_positions

    btc_usdt = TestInstrumentProvider.btcusdt_binance()
    eth_usdt = TestInstrumentProvider.ethusdt_binance()
    client._cache.add_instrument(btc_usdt)
    client._cache.add_instrument(eth_usdt)

    instrument_provider.get_all.return_value = {
        btc_usdt.id: btc_usdt,
        eth_usdt.id: eth_usdt,
    }

    # Mock OKX balance response with multiple currencies
    # http_get_balance returns a flattened list of balance details
    okx_btc_detail = SimpleNamespace(
        ccy="BTC",
        cash_bal="1.5",
        liab="0.5",  # net = 1.0 BTC LONG
    )
    okx_eth_detail = SimpleNamespace(
        ccy="ETH",
        cash_bal="10.0",
        liab="15.0",  # net = -5.0 ETH SHORT
    )
    http_client.http_get_balance = AsyncMock(return_value=[okx_btc_detail, okx_eth_detail])

    command = GeneratePositionStatusReports(
        instrument_id=None,  # Query all instruments
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 2

    # Find BTC report
    btc_report = next((r for r in reports if r.instrument_id.symbol.value.startswith("BTC")), None)
    assert btc_report is not None
    assert btc_report.position_side == PositionSide.LONG
    assert btc_report.quantity == Quantity.from_str("1.0")

    # Find ETH report
    eth_report = next((r for r in reports if r.instrument_id.symbol.value.startswith("ETH")), None)
    assert eth_report is not None
    assert eth_report.position_side == PositionSide.SHORT
    # Check approximate equality due to precision rounding (5.0 may become 4.99999)
    assert abs(float(eth_report.quantity) - 5.0) < 0.001


@pytest.mark.asyncio
async def test_spot_position_reports_handles_empty_balance_response(
    exec_client_with_spot_positions,
):
    """
    Test handling when OKX returns empty balance data.
    """
    # Arrange
    client, _, _, http_client, instrument_provider = exec_client_with_spot_positions

    instrument = TestInstrumentProvider.btcusdt_binance()
    client._cache.add_instrument(instrument)

    # Mock empty OKX balance response
    http_client.http_get_balance = AsyncMock(return_value=[])

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert - should return empty list (no warnings or errors)
    assert len(reports) == 0


@pytest.mark.asyncio
async def test_spot_position_reports_handles_missing_currency(
    exec_client_with_spot_positions,
):
    """
    Test handling when requested currency is not in wallet balance.
    """
    # Arrange
    client, _, _, http_client, instrument_provider = exec_client_with_spot_positions

    btc_usdt = TestInstrumentProvider.btcusdt_binance()
    client._cache.add_instrument(btc_usdt)

    # Mock OKX balance response with only ETH (no BTC)
    # http_get_balance returns a flattened list of balance details
    okx_eth_detail = SimpleNamespace(
        ccy="ETH",
        cash_bal="10.0",
        liab="0",
    )
    http_client.http_get_balance = AsyncMock(return_value=[okx_eth_detail])

    command = GeneratePositionStatusReports(
        instrument_id=btc_usdt.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert - should return FLAT position for BTC
    assert len(reports) == 1
    report = reports[0]
    assert report.position_side == PositionSide.FLAT
    assert report.quantity == Quantity.zero(btc_usdt.size_precision)


@pytest.mark.asyncio
async def test_spot_position_reports_handles_exceptions(
    exec_client_with_spot_positions,
):
    """
    Test error handling when balance query fails.
    """
    # Arrange
    client, _, _, http_client, _ = exec_client_with_spot_positions

    instrument = TestInstrumentProvider.btcusdt_binance()
    client._cache.add_instrument(instrument)

    # Mock http_get_balance to raise an exception
    http_client.http_get_balance = AsyncMock(side_effect=Exception("API error"))

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert - should return empty list and log exception
    assert len(reports) == 0


@pytest.mark.asyncio
async def test_spot_position_reports_emits_per_instrument_with_same_base(
    exec_client_with_spot_positions,
):
    """
    Test that multiple pairs with same base currency each get a position report.

    The execution engine tracks positions per InstrumentId, so even though BTC/USDT and
    BTC/EUR share the same BTC wallet, each instrument needs its own position report for
    reconciliation. Both reports will show the same 1.5 BTC wallet balance.

    """
    # Arrange
    client, _, _, http_client, instrument_provider = exec_client_with_spot_positions

    # Create multiple BTC pairs: BTC/USDT, BTC/EUR
    btc_usdt = TestInstrumentProvider.btcusdt_binance()

    # Create BTC/EUR by modifying a copy
    btc_eur = TestInstrumentProvider.btcusdt_binance()
    btc_eur = btc_eur.__class__(
        instrument_id=InstrumentId(Symbol("BTCEUR"), Venue("BINANCE")),
        raw_symbol=Symbol("BTCEUR"),
        base_currency=btc_eur.base_currency,
        quote_currency=btc_eur.quote_currency,
        price_precision=btc_eur.price_precision,
        size_precision=btc_eur.size_precision,
        price_increment=btc_eur.price_increment,
        size_increment=btc_eur.size_increment,
        lot_size=btc_eur.lot_size,
        max_quantity=btc_eur.max_quantity,
        min_quantity=btc_eur.min_quantity,
        max_price=btc_eur.max_price,
        min_price=btc_eur.min_price,
        margin_init=btc_eur.margin_init,
        margin_maint=btc_eur.margin_maint,
        maker_fee=btc_eur.maker_fee,
        taker_fee=btc_eur.taker_fee,
        ts_event=btc_eur.ts_event,
        ts_init=btc_eur.ts_init,
        info=btc_eur.info,
    )

    client._cache.add_instrument(btc_usdt)
    client._cache.add_instrument(btc_eur)

    instrument_provider.get_all.return_value = {
        btc_usdt.id: btc_usdt,
        btc_eur.id: btc_eur,
    }

    # Mock OKX balance response: 1.5 BTC in wallet
    # http_get_balance returns a flattened list of balance details
    okx_btc_detail = SimpleNamespace(
        ccy="BTC",
        cash_bal="1.5",
        liab="0",
    )
    http_client.http_get_balance = AsyncMock(return_value=[okx_btc_detail])

    command = GeneratePositionStatusReports(
        instrument_id=None,  # Query all instruments
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert - should get TWO reports (one per instrument), both showing 1.5 BTC
    assert len(reports) == 2

    # Both reports should be LONG with 1.5 BTC quantity
    for report in reports:
        assert report.position_side == PositionSide.LONG
        assert report.quantity == Quantity.from_str("1.5")
        assert report.instrument_id.symbol.value.startswith("BTC")

    # Verify we got reports for both instruments
    reported_instruments = {r.instrument_id for r in reports}
    assert btc_usdt.id in reported_instruments
    assert btc_eur.id in reported_instruments
