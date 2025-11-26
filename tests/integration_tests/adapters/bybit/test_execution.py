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
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pandas as pd
import pytest

from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.execution import BybitExecutionClient
from nautilus_trader.common.component import TestClock
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.bybit.conftest import _create_ws_mock


@pytest.fixture
def exec_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None, clock=None):
        ws_private_client = _create_ws_mock()
        ws_trade_client = _create_ws_mock()
        ws_iter = iter([ws_private_client, ws_trade_client])

        monkeypatch.setattr(
            "nautilus_trader.adapters.bybit.execution.nautilus_pyo3.BybitWebSocketClient.new_private",
            lambda *args, **kwargs: next(ws_iter),
        )
        monkeypatch.setattr(
            "nautilus_trader.adapters.bybit.execution.nautilus_pyo3.BybitWebSocketClient.new_trade",
            lambda *args, **kwargs: next(ws_iter),
        )

        # Skip account registration wait in tests
        monkeypatch.setattr(
            "nautilus_trader.adapters.bybit.execution.BybitExecutionClient._await_account_registered",
            AsyncMock(),
        )

        mock_http_client.reset_mock()
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        # Return empty list to avoid PyO3 type conversion issues in tests
        mock_instrument_provider.instruments_pyo3.return_value = []

        config = BybitExecClientConfig(
            api_key="test_api_key",
            api_secret="test_api_secret",
            product_types=(nautilus_pyo3.BybitProductType.LINEAR,),
            **(config_kwargs or {}),
        )

        client = BybitExecutionClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock or live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, ws_private_client, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_success(exec_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
        http_client.request_account_state.assert_awaited_once()
        ws_client.connect.assert_awaited_once()
        ws_client.wait_until_active.assert_awaited_once_with(timeout_secs=10.0)
        ws_client.subscribe_orders.assert_awaited_once()
        ws_client.subscribe_executions.assert_awaited_once()
        ws_client.subscribe_positions.assert_awaited_once()
        ws_client.subscribe_wallet.assert_awaited_once()
    finally:
        await client._disconnect()

    # Assert
    ws_client.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_generate_order_status_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.OrderStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    pyo3_report = MagicMock()
    http_client.request_order_status_reports.return_value = [pyo3_report]

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_order_status_reports(command)

    # Assert
    http_client.request_order_status_reports.assert_awaited_once()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_order_status_reports_handles_failure(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_order_status_reports.side_effect = Exception("boom")

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        start=None,
        end=None,
        open_only=False,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_order_status_reports(command)

    # Assert
    assert reports == []


@pytest.mark.asyncio
async def test_generate_fill_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.FillReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_fill_reports.return_value = [MagicMock()]

    command = GenerateFillReports(
        instrument_id=InstrumentId(Symbol("BTCUSDT-SPOT"), BYBIT_VENUE),
        venue_order_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_fill_reports(command)

    # Assert
    http_client.request_fill_reports.assert_awaited_once()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_position_status_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.PositionStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_position_status_reports.return_value = [MagicMock()]

    command = GeneratePositionStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    http_client.request_position_status_reports.assert_awaited_once()
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_generate_position_status_reports_handles_failure(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_position_status_reports.side_effect = Exception("boom")

    command = GeneratePositionStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert
    assert reports == []


# ============================================================================
# LIFECYCLE TESTS
# ============================================================================


@pytest.mark.asyncio
async def test_disconnect_success(exec_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    # Act
    await client._disconnect()

    # Assert
    http_client.cancel_all_requests.assert_called_once()
    ws_client.close.assert_awaited()


@pytest.mark.asyncio
async def test_account_id_set_on_initialization(exec_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    # Assert - Bybit sets account_id to UNIFIED during initialization
    # (unlike BitMEX which updates it from account state)
    assert client.account_id.value == "BYBIT-UNIFIED"

    # Act - connect should not change the account_id
    await client._connect()

    try:
        # Assert - account_id remains UNIFIED after connection
        assert client.account_id.value == "BYBIT-UNIFIED"
    finally:
        await client._disconnect()


# ============================================================================
# ORDER SUBMISSION TESTS
# ============================================================================


@pytest.mark.asyncio
async def test_submit_market_order(exec_client_builder, monkeypatch, instrument):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    # Get the trade WebSocket client (second one created)
    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()

    await client._connect()

    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        time_in_force=TimeInForce.IOC,
        reduce_only=False,
        quote_quantity=False,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order(command)

        # Assert - Bybit uses WebSocket for order submission, not HTTP
        ws_trade_client.submit_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_limit_order(exec_client_builder, monkeypatch, instrument):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order(command)

        # Assert - Bybit uses WebSocket for order submission
        ws_trade_client.submit_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_stop_market_order(exec_client_builder, monkeypatch, instrument):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()

    await client._connect()

    order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        trigger_price=Price.from_str("51000.00"),
        trigger_type=TriggerType.LAST_PRICE,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order(command)

        # Assert - Bybit uses WebSocket for order submission
        ws_trade_client.submit_order.assert_awaited_once()
    finally:
        await client._disconnect()


# ============================================================================
# ORDER MODIFICATION TESTS
# ============================================================================


@pytest.mark.asyncio
async def test_modify_order_price(exec_client_builder, monkeypatch, instrument, cache):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.modify_order = AsyncMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Add order to cache
    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("BYBIT-12345"),
        quantity=order.quantity,
        price=Price.from_str("51000.00"),  # New price
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert - Bybit uses WebSocket for order modification
        ws_trade_client.modify_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_quantity(exec_client_builder, monkeypatch, instrument, cache):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.modify_order = AsyncMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("BYBIT-12345"),
        quantity=Quantity.from_str("0.200"),  # New quantity
        price=order.price,
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert - Bybit uses WebSocket for order modification
        ws_trade_client.modify_order.assert_awaited_once()
    finally:
        await client._disconnect()


# ============================================================================
# ORDER CANCELLATION TESTS
# ============================================================================


@pytest.mark.asyncio
async def test_cancel_order_by_client_id(exec_client_builder, monkeypatch, instrument, cache):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.cancel_order = AsyncMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._cancel_order(command)

        # Assert - Bybit uses WebSocket for order cancellation
        ws_trade_client.cancel_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order_by_venue_id(exec_client_builder, monkeypatch, instrument, cache):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.cancel_order = AsyncMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("BYBIT-12345"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._cancel_order(command)

        # Assert - Bybit uses WebSocket for order cancellation
        ws_trade_client.cancel_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_all_orders(exec_client_builder, monkeypatch, instrument):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    command = CancelAllOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        order_side=OrderSide.NO_ORDER_SIDE,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    http_client.cancel_all_orders.return_value = []

    try:
        # Act
        await client._cancel_all_orders(command)

        # Assert
        http_client.cancel_all_orders.assert_awaited_once()
    finally:
        await client._disconnect()


# ============================================================================
# ORDER REJECTION AND ERROR HANDLING TESTS
# ============================================================================


@pytest.mark.asyncio
async def test_submit_order_rejection(exec_client_builder, monkeypatch, instrument, msgbus):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        time_in_force=TimeInForce.IOC,
        reduce_only=False,
        quote_quantity=False,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    # Simulate order rejection
    http_client.submit_order.side_effect = Exception("Order rejected: Insufficient margin")

    try:
        # Act/Assert - Should not raise, but handle gracefully
        await client._submit_order(command)

        # The order should be rejected via the message bus
        # (Implementation detail - error is logged and event generated)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_rejection(exec_client_builder, monkeypatch, instrument, cache):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("BYBIT-12345"),
        quantity=Quantity.from_str("0.200"),
        price=Price.from_str("51000.00"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    http_client.modify_order.side_effect = Exception("Order not found")

    try:
        # Act/Assert - Should handle gracefully
        await client._modify_order(command)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order_rejection(exec_client_builder, monkeypatch, instrument, cache):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("BYBIT-12345"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    http_client.cancel_order.side_effect = Exception("Order already filled")

    try:
        # Act/Assert - Should handle gracefully
        await client._cancel_order(command)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_denied_reduce_only_spot(
    exec_client_builder,
    monkeypatch,
    msgbus,
):
    # Arrange - Use SPOT instrument
    spot_instrument = CryptoPerpetual(
        instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
        raw_symbol=Symbol("BTCUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        max_quantity=Quantity.from_str("1000"),
        min_quantity=Quantity.from_str("0.000001"),
        max_notional=None,
        min_notional=Money(1.00, USDT),
        max_price=Price.from_str("1000000.00"),
        min_price=Price.from_str("0.01"),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
        ts_event=0,
        ts_init=0,
    )

    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    await client._connect()

    # Create a LIMIT order with REDUCE_ONLY on SPOT (invalid)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=spot_instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        reduce_only=True,  # Invalid for SPOT
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()

    try:
        # Act
        await client._submit_order(command)

        # Assert - Order should be denied, not submitted to WebSocket
        ws_trade_client.submit_order.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_with_is_leverage(
    exec_client_builder,
    monkeypatch,
):
    # Arrange - Use SPOT instrument
    spot_instrument = CryptoPerpetual(
        instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
        raw_symbol=Symbol("BTCUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        max_quantity=Quantity.from_str("1000"),
        min_quantity=Quantity.from_str("0.000001"),
        max_notional=None,
        min_notional=Money(1.00, USDT),
        max_price=Price.from_str("1000000.00"),
        min_price=Price.from_str("0.01"),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
        ts_event=0,
        ts_init=0,
    )

    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()

    await client._connect()

    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=spot_instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        time_in_force=TimeInForce.IOC,
        reduce_only=False,
        quote_quantity=False,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
        params={"is_leverage": True},
    )

    try:
        # Act
        await client._submit_order(command)

        # Assert - is_leverage=True should be passed through
        ws_trade_client.submit_order.assert_awaited_once()
        call_kwargs = ws_trade_client.submit_order.call_args[1]
        assert call_kwargs["is_leverage"] is True
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_with_is_quote_quantity(
    exec_client_builder,
    monkeypatch,
):
    # Arrange - Use SPOT instrument
    spot_instrument = CryptoPerpetual(
        instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
        raw_symbol=Symbol("BTCUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        max_quantity=Quantity.from_str("1000"),
        min_quantity=Quantity.from_str("0.000001"),
        max_notional=None,
        min_notional=Money(1.00, USDT),
        max_price=Price.from_str("1000000.00"),
        min_price=Price.from_str("0.01"),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
        ts_event=0,
        ts_init=0,
    )

    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()

    await client._connect()

    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=spot_instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        time_in_force=TimeInForce.IOC,
        reduce_only=False,
        quote_quantity=True,  # This should be passed through
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order(command)

        # Assert - is_quote_quantity=True should be passed through
        ws_trade_client.submit_order.assert_awaited_once()
        call_kwargs = ws_trade_client.submit_order.call_args[1]
        assert call_kwargs["is_quote_quantity"] is True
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_order_rejected_pyo3_conversion(
    exec_client_builder,
    monkeypatch,
    instrument,
    msgbus,
):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    pyo3_event = nautilus_pyo3.OrderRejected(
        trader_id=nautilus_pyo3.TraderId(TestIdStubs.trader_id().value),
        strategy_id=nautilus_pyo3.StrategyId(TestIdStubs.strategy_id().value),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        client_order_id=nautilus_pyo3.ClientOrderId("O-123456"),
        account_id=nautilus_pyo3.AccountId(TestIdStubs.account_id().value),
        reason="InsufficientMargin",
        event_id=nautilus_pyo3.UUID4(),
        ts_event=123456789,
        ts_init=123456789,
        reconciliation=False,
    )

    try:
        # Act - Should not raise AttributeError about 'from_pyo3'
        client._handle_order_rejected_pyo3(pyo3_event)

        # Assert - Event should be converted and sent
        assert msgbus.sent_count > 0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_order_cancel_rejected_pyo3_conversion(
    exec_client_builder,
    monkeypatch,
    instrument,
    msgbus,
):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    pyo3_event = nautilus_pyo3.OrderCancelRejected(
        trader_id=nautilus_pyo3.TraderId(TestIdStubs.trader_id().value),
        strategy_id=nautilus_pyo3.StrategyId(TestIdStubs.strategy_id().value),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        client_order_id=nautilus_pyo3.ClientOrderId("O-123456"),
        venue_order_id=nautilus_pyo3.VenueOrderId("BYBIT-12345"),
        reason="OrderNotFound",
        event_id=nautilus_pyo3.UUID4(),
        ts_event=123456789,
        ts_init=123456789,
        reconciliation=False,
        account_id=nautilus_pyo3.AccountId(TestIdStubs.account_id().value),
    )

    try:
        # Act - Should not raise AttributeError about 'from_pyo3'
        client._handle_order_cancel_rejected_pyo3(pyo3_event)

        # Assert - Event should be converted and sent
        assert msgbus.sent_count > 0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_order_modify_rejected_pyo3_conversion(
    exec_client_builder,
    monkeypatch,
    instrument,
    msgbus,
):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    pyo3_event = nautilus_pyo3.OrderModifyRejected(
        trader_id=nautilus_pyo3.TraderId(TestIdStubs.trader_id().value),
        strategy_id=nautilus_pyo3.StrategyId(TestIdStubs.strategy_id().value),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        client_order_id=nautilus_pyo3.ClientOrderId("O-123456"),
        reason="OrderNotFound",
        event_id=nautilus_pyo3.UUID4(),
        ts_event=123456789,
        ts_init=123456789,
        reconciliation=False,
        venue_order_id=nautilus_pyo3.VenueOrderId("BYBIT-12345"),
        account_id=nautilus_pyo3.AccountId(TestIdStubs.account_id().value),
    )

    try:
        # Act - Should not raise AttributeError about 'from_pyo3'
        client._handle_order_modify_rejected_pyo3(pyo3_event)

        # Assert - Event should be converted and sent
        assert msgbus.sent_count > 0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_repay_spot_borrow_handles_api_errors_gracefully(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    # Use TestClock with time outside blackout window (04:00-05:30 UTC) so repayment logic runs
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )
    http_client.get_spot_borrow_amount = AsyncMock(return_value=100.0)
    http_client.repay_spot_borrow = AsyncMock(side_effect=Exception("API Error"))
    bought_qty = nautilus_pyo3.Quantity(50.0, 2)

    try:
        # Act - Should not raise, just log error
        await client._repay_spot_borrow_if_needed("BTC", bought_qty)

        # Assert - Method was called despite error
        http_client.get_spot_borrow_amount.assert_called_once_with("BTC")
        # Should repay min(100, 50) = 50
        assert http_client.repay_spot_borrow.call_count == 1
        call_args = http_client.repay_spot_borrow.call_args
        assert call_args[0][0] == "BTC"
        assert float(call_args[0][1]) == 50.0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_repay_spot_borrow_skips_when_no_borrow(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    # Use TestClock with time outside blackout window (04:00-05:30 UTC) so repayment logic runs
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )
    http_client.get_spot_borrow_amount = AsyncMock(return_value=0.0)
    http_client.repay_spot_borrow = AsyncMock()
    bought_qty = nautilus_pyo3.Quantity(10.0, 2)

    try:
        # Act
        await client._repay_spot_borrow_if_needed("ETH", bought_qty)

        # Assert - Should check borrow amount but not call repay
        http_client.get_spot_borrow_amount.assert_called_once_with("ETH")
        http_client.repay_spot_borrow.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_repay_spot_borrow_calls_repay_when_borrow_exists(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    # Use TestClock with time outside blackout window (04:00-05:30 UTC) so repayment logic runs
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )
    http_client.get_spot_borrow_amount = AsyncMock(return_value=250.5)
    http_client.repay_spot_borrow = AsyncMock()
    bought_qty = nautilus_pyo3.Quantity(100.0, 2)

    try:
        # Act
        await client._repay_spot_borrow_if_needed("BTC", bought_qty)

        # Assert - Should check borrow amount and call repay
        http_client.get_spot_borrow_amount.assert_called_once_with("BTC")
        # Should repay min(250.5, 100) = 100
        assert http_client.repay_spot_borrow.call_count == 1
        call_args = http_client.repay_spot_borrow.call_args
        assert call_args[0][0] == "BTC"
        assert float(call_args[0][1]) == 100.0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_repay_spot_borrow_repays_partial_when_bought_less_than_borrowed(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    # Use TestClock with time outside blackout window (04:00-05:30 UTC) so repayment logic runs
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )
    http_client.get_spot_borrow_amount = AsyncMock(return_value=500.0)
    http_client.repay_spot_borrow = AsyncMock()
    bought_qty = nautilus_pyo3.Quantity(150.0, 2)

    try:
        # Act - Bought 150, but borrowed 500
        await client._repay_spot_borrow_if_needed("ETH", bought_qty)

        # Assert - Should only repay what we bought (150), not full borrow (500)
        http_client.get_spot_borrow_amount.assert_called_once_with("ETH")
        assert http_client.repay_spot_borrow.call_count == 1
        call_args = http_client.repay_spot_borrow.call_args
        assert call_args[0][0] == "ETH"
        assert float(call_args[0][1]) == 150.0
    finally:
        await client._disconnect()


def test_is_repay_blackout_window_during_hour_4(monkeypatch, exec_client_builder):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 04:15:00", tz="UTC").value)
    client, _, _, _ = exec_client_builder(monkeypatch, clock=test_clock)

    # Act
    result = client._is_repay_blackout_window()

    # Assert - 04:15 UTC is in blackout window
    assert result is True


def test_is_repay_blackout_window_during_hour_5_before_30min(monkeypatch, exec_client_builder):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 05:29:00", tz="UTC").value)
    client, _, _, _ = exec_client_builder(monkeypatch, clock=test_clock)

    # Act
    result = client._is_repay_blackout_window()

    # Assert - 05:29 UTC is in blackout window
    assert result is True


def test_is_repay_blackout_window_after_blackout(monkeypatch, exec_client_builder):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 05:30:00", tz="UTC").value)
    client, _, _, _ = exec_client_builder(monkeypatch, clock=test_clock)

    # Act
    result = client._is_repay_blackout_window()

    # Assert - 05:30 UTC is AFTER blackout window
    assert result is False


def test_is_repay_blackout_window_outside_blackout(monkeypatch, exec_client_builder):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, _, _ = exec_client_builder(monkeypatch, clock=test_clock)

    # Act
    result = client._is_repay_blackout_window()

    # Assert - 10:00 UTC is outside blackout window
    assert result is False


@pytest.mark.asyncio
async def test_auto_repayment_skipped_during_blackout_window(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    # Use TestClock with time during blackout window (04:30 UTC)
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 04:30:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )
    http_client.repay_spot_borrow = AsyncMock()
    bought_qty = nautilus_pyo3.Quantity(1.0, 2)

    try:
        # Act
        await client._repay_spot_borrow_if_needed("BTC", bought_qty)

        # Assert - Repayment was NOT called during blackout window
        http_client.repay_spot_borrow.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_repay_accepts_decimal_type_from_fill_accumulation(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )
    http_client.get_spot_borrow_amount = AsyncMock(return_value=Decimal("100.0"))
    http_client.repay_spot_borrow = AsyncMock()

    bought_qty = nautilus_pyo3.Quantity(0.08, 5)

    try:
        # Act
        await client._repay_spot_borrow_if_needed("ETH", bought_qty)

        # Assert - Should handle Decimal type correctly
        http_client.get_spot_borrow_amount.assert_called_once_with("ETH")
        assert http_client.repay_spot_borrow.call_count == 1
        call_args = http_client.repay_spot_borrow.call_args
        assert call_args[0][0] == "ETH"
        assert isinstance(call_args[0][1], nautilus_pyo3.Quantity)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_fill_report_tracks_partial_fills_for_spot_buy(
    monkeypatch,
    exec_client_builder,
    cache,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )

    spot_instrument = CurrencyPair(
        instrument_id=InstrumentId.from_str("ETHUSDT-SPOT.BYBIT"),
        raw_symbol=Symbol("ETHUSDT"),
        base_currency=ETH,
        quote_currency=USDT,
        price_precision=2,
        size_precision=5,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.00001"),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
    )
    cache.add_instrument(spot_instrument)

    # Create a BUY order for SPOT
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=spot_instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("3000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    http_client.get_spot_borrow_amount = AsyncMock(return_value=Decimal(0))
    http_client.repay_spot_borrow = AsyncMock()

    # Create partial fill report (50% of order)
    fill_report = nautilus_pyo3.FillReport(
        account_id=nautilus_pyo3.AccountId("BYBIT-UNIFIED"),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(spot_instrument.id.value),
        venue_order_id=nautilus_pyo3.VenueOrderId("BYBIT-789"),
        trade_id=nautilus_pyo3.TradeId("T-001"),
        order_side=nautilus_pyo3.OrderSide.BUY,
        last_qty=nautilus_pyo3.Quantity(0.050, 5),
        last_px=nautilus_pyo3.Price(3000.00, 2),
        commission=nautilus_pyo3.Money.from_str("0.01 USDT"),
        liquidity_side=nautilus_pyo3.LiquiditySide.TAKER,
        ts_event=0,
        client_order_id=nautilus_pyo3.ClientOrderId("O-123456"),
        report_id=nautilus_pyo3.UUID4(),
        ts_init=0,
    )

    try:
        # Act - Process first partial fill
        client._handle_fill_report_pyo3(fill_report)

        # Assert - Fill should be tracked, but not trigger repayment yet
        assert order.client_order_id in client._order_filled_qty
        assert client._order_filled_qty[order.client_order_id] == Decimal("0.050")
        http_client.repay_spot_borrow.assert_not_called()

        # Act - Process second partial fill (completes the order)
        fill_report2 = nautilus_pyo3.FillReport(
            account_id=nautilus_pyo3.AccountId("BYBIT-UNIFIED"),
            instrument_id=nautilus_pyo3.InstrumentId.from_str(spot_instrument.id.value),
            venue_order_id=nautilus_pyo3.VenueOrderId("BYBIT-789"),
            trade_id=nautilus_pyo3.TradeId("T-002"),
            order_side=nautilus_pyo3.OrderSide.BUY,
            last_qty=nautilus_pyo3.Quantity(0.050, 5),
            last_px=nautilus_pyo3.Price(3000.00, 2),
            commission=nautilus_pyo3.Money.from_str("0.01 USDT"),
            liquidity_side=nautilus_pyo3.LiquiditySide.TAKER,
            ts_event=0,
            client_order_id=nautilus_pyo3.ClientOrderId("O-123456"),
            report_id=nautilus_pyo3.UUID4(),
            ts_init=0,
        )
        client._handle_fill_report_pyo3(fill_report2)

        # Give async task time to execute
        await asyncio.sleep(0.1)

        # Assert - Order should be removed from tracking after full fill
        assert order.client_order_id not in client._order_filled_qty
        # Repayment check should have been called (even though borrow is 0)
        http_client.get_spot_borrow_amount.assert_called_once_with("ETH")
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_fill_report_ignores_non_spot_orders(
    monkeypatch,
    exec_client_builder,
    cache,
    instrument,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True},
        clock=test_clock,
    )

    # Create a BUY order for LINEAR (not SPOT)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,  # LINEAR instrument
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)
    cache.add_instrument(instrument)

    http_client.get_spot_borrow_amount = AsyncMock()
    http_client.repay_spot_borrow = AsyncMock()

    fill_report = nautilus_pyo3.FillReport(
        account_id=nautilus_pyo3.AccountId("BYBIT-UNIFIED"),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        venue_order_id=nautilus_pyo3.VenueOrderId("BYBIT-789"),
        trade_id=nautilus_pyo3.TradeId("T-001"),
        order_side=nautilus_pyo3.OrderSide.BUY,
        last_qty=nautilus_pyo3.Quantity.from_str("0.100"),
        last_px=nautilus_pyo3.Price.from_str("50000.00"),
        commission=nautilus_pyo3.Money.from_str("0.01 USDT"),
        liquidity_side=nautilus_pyo3.LiquiditySide.TAKER,
        ts_event=0,
        client_order_id=nautilus_pyo3.ClientOrderId("O-123456"),
        report_id=nautilus_pyo3.UUID4(),
        ts_init=0,
    )

    try:
        # Act - Process fill for LINEAR order
        client._handle_fill_report_pyo3(fill_report)

        # Assert - Should NOT track or trigger repayment for LINEAR
        assert order.client_order_id not in client._order_filled_qty
        http_client.get_spot_borrow_amount.assert_not_called()
    finally:
        await client._disconnect()
