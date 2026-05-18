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

from decimal import Decimal
from unittest.mock import ANY
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pandas as pd
import pytest

from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.execution import BybitExecutionClient
from nautilus_trader.adapters.bybit.execution import _is_confirmed_submit_rejection_error
from nautilus_trader.adapters.bybit.execution import _parse_bybit_tp_sl_params
from nautilus_trader.common.component import TestClock
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import OrderList
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.bybit.conftest import _create_ws_mock
from tests.integration_tests.adapters.bybit.conftest import create_bybit_linear_perpetual


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
    expected_report.client_order_id = None  # External order short-circuit in cache helper
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
async def test_generate_order_status_reports_caches_local_venue_position_id(
    exec_client_builder,
    monkeypatch,
    cache,
    instrument,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-HEDGE-RECON"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100000"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    venue_position_id = PositionId("BTCUSDT-SPOT.BYBIT-LONG")
    expected_report = MagicMock()
    expected_report.client_order_id = order.client_order_id
    expected_report.venue_position_id = venue_position_id
    expected_report.order_status = OrderStatus.ACCEPTED
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.OrderStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_order_status_reports.return_value = [MagicMock()]
    command = GenerateOrderStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        open_only=True,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_order_status_reports(command)

    # Assert
    assert reports == [expected_report]
    assert client._order_position_ids[order.client_order_id] == venue_position_id


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
        kwargs = ws_trade_client.submit_order.call_args.kwargs
        assert isinstance(kwargs["order_side"], nautilus_pyo3.OrderSide)
        assert isinstance(kwargs["order_type"], nautilus_pyo3.OrderType)
        assert isinstance(kwargs["time_in_force"], nautilus_pyo3.TimeInForce)
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
        kwargs = ws_trade_client.submit_order.call_args.kwargs
        assert isinstance(kwargs["order_side"], nautilus_pyo3.OrderSide)
        assert isinstance(kwargs["order_type"], nautilus_pyo3.OrderType)
        assert isinstance(kwargs["time_in_force"], nautilus_pyo3.TimeInForce)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_limit_order_with_bbo_params(exec_client_builder, monkeypatch):
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()

    await client._connect()

    instrument = create_bybit_linear_perpetual()
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-BBO-1"),
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
        params={"bbo_side_type": "queue", "bbo_level": 3},
    )

    try:
        await client._submit_order(command)

        ws_trade_client.submit_order.assert_awaited_once()
        kwargs = ws_trade_client.submit_order.call_args.kwargs
        assert kwargs["price"] is None
        assert kwargs["bbo_side_type"] == "Queue"
        assert kwargs["bbo_level"] == "3"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_market_order_with_bbo_params_denied(
    exec_client_builder,
    monkeypatch,
):
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()
    mock_generate_denied = MagicMock()
    monkeypatch.setattr(client, "generate_order_denied", mock_generate_denied)

    await client._connect()

    instrument = create_bybit_linear_perpetual()
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-BBO-2"),
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
        params={"bbo_side_type": "Queue", "bbo_level": "1"},
    )

    try:
        await client._submit_order(command)

        ws_trade_client.submit_order.assert_not_awaited()
        mock_generate_denied.assert_called_once()
        assert "not supported" in mock_generate_denied.call_args.kwargs["reason"]
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_list_with_bbo_params(exec_client_builder, monkeypatch):
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.build_place_order_params = MagicMock(side_effect=[MagicMock(), MagicMock()])
    ws_trade_client.batch_place_orders = AsyncMock()

    await client._connect()

    instrument = create_bybit_linear_perpetual()
    orders = [
        LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId("O-BBO-LIST-1"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.100"),
            price=Price.from_str("50000.00"),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        ),
        LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId("O-BBO-LIST-2"),
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("0.100"),
            price=Price.from_str("50100.00"),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        ),
    ]
    order_list = OrderList(TestIdStubs.order_list_id(), orders)
    command = SubmitOrderList(
        trader_id=orders[0].trader_id,
        strategy_id=orders[0].strategy_id,
        order_list=order_list,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
        params={"bbo_side_type": "counterparty", "bbo_level": 5},
    )

    try:
        await client._submit_order_list(command)

        assert ws_trade_client.build_place_order_params.call_count == 2
        for call in ws_trade_client.build_place_order_params.call_args_list:
            assert call.kwargs["price"] is None
            assert call.kwargs["bbo_side_type"] == "Counterparty"
            assert call.kwargs["bbo_level"] == "5"
        ws_trade_client.batch_place_orders.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_list_with_bbo_params_denies_unsupported_order_type(
    exec_client_builder,
    monkeypatch,
):
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.build_place_order_params = MagicMock()
    ws_trade_client.batch_place_orders = AsyncMock()
    mock_generate_denied = MagicMock()
    monkeypatch.setattr(client, "generate_order_denied", mock_generate_denied)

    await client._connect()

    instrument = create_bybit_linear_perpetual()
    orders = [
        LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId("O-BBO-LIST-3"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.100"),
            price=Price.from_str("50000.00"),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        ),
        MarketOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId("O-BBO-LIST-4"),
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("0.100"),
            time_in_force=TimeInForce.IOC,
            reduce_only=False,
            quote_quantity=False,
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        ),
    ]
    order_list = OrderList(TestIdStubs.order_list_id(), orders)
    command = SubmitOrderList(
        trader_id=orders[0].trader_id,
        strategy_id=orders[0].strategy_id,
        order_list=order_list,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
        params={"bbo_side_type": "Queue", "bbo_level": "1"},
    )

    try:
        await client._submit_order_list(command)

        ws_trade_client.build_place_order_params.assert_not_called()
        ws_trade_client.batch_place_orders.assert_not_awaited()
        assert mock_generate_denied.call_count == 2
        for call in mock_generate_denied.call_args_list:
            assert "not supported" in call.kwargs["reason"]
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
        kwargs = ws_trade_client.submit_order.call_args.kwargs
        assert isinstance(kwargs["order_side"], nautilus_pyo3.OrderSide)
        assert isinstance(kwargs["order_type"], nautilus_pyo3.OrderType)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_limit_order_with_take_profit_stop_loss(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    """
    Limit order with take_profit + stop_loss in params routes through the native TP/SL
    path: build_place_order_params is called with correct Price objects and the result
    is submitted via batch_place_orders.
    """
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.build_place_order_params = MagicMock(return_value=MagicMock())
    ws_trade_client.batch_place_orders = AsyncMock()

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
        params={"take_profit": "55000.00", "stop_loss": "47000.00"},
    )

    try:
        await client._submit_order(command)

        ws_trade_client.build_place_order_params.assert_called_once()
        call_kwargs = ws_trade_client.build_place_order_params.call_args.kwargs
        assert call_kwargs["take_profit"] == nautilus_pyo3.Price.from_str("55000.00")
        assert call_kwargs["stop_loss"] == nautilus_pyo3.Price.from_str("47000.00")
        ws_trade_client.batch_place_orders.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_market_order_with_take_profit_only(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    """
    Only take_profit in params also triggers the native TP/SL path; stop_loss is not
    set.
    """
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.build_place_order_params = MagicMock(return_value=MagicMock())
    ws_trade_client.batch_place_orders = AsyncMock()

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
        params={"take_profit": "55000.00"},
    )

    try:
        await client._submit_order(command)

        ws_trade_client.build_place_order_params.assert_called_once()
        call_kwargs = ws_trade_client.build_place_order_params.call_args.kwargs
        assert call_kwargs["take_profit"] == nautilus_pyo3.Price.from_str("55000.00")
        assert call_kwargs.get("stop_loss") is None
        ws_trade_client.batch_place_orders.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_with_tp_sl_enum_fields(exec_client_builder, monkeypatch, instrument):
    """
    Trigger types and order types are applied as string attributes on the params object
    after build_place_order_params returns (fields are mutable via PyO3 get+set).
    """
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    fake_order_params = MagicMock()
    ws_trade_client = client._ws_trade_client
    ws_trade_client.build_place_order_params = MagicMock(return_value=fake_order_params)
    ws_trade_client.batch_place_orders = AsyncMock()

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
        params={
            "take_profit": "55000.00",
            "stop_loss": "47000.00",
            "tp_trigger_by": "MarkPrice",
            "sl_trigger_by": "LastPrice",
            "tp_order_type": "Limit",
            "sl_order_type": "Market",
            "tp_limit_price": "54990.00",
            "sl_trigger_price": "46990.00",
            "close_on_trigger": True,
        },
    )

    try:
        await client._submit_order(command)

        # Enum/price fields must be set directly on the returned params object.
        assert fake_order_params.tp_trigger_by == "MarkPrice"
        assert fake_order_params.sl_trigger_by == "LastPrice"
        assert fake_order_params.tp_order_type == "Limit"
        assert fake_order_params.sl_order_type == "Market"
        assert fake_order_params.tp_limit_price == "54990.00"
        assert fake_order_params.sl_trigger_price == "46990.00"
        assert fake_order_params.close_on_trigger is True
        ws_trade_client.batch_place_orders.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_with_invalid_tp_trigger_type_emits_order_denied(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    """
    An unrecognised tp_trigger_by value must emit generate_order_denied before
    generate_order_submitted — the order must never reach the exchange.
    """
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()
    ws_trade_client.batch_place_orders = AsyncMock()

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
        params={"take_profit": "55000.00", "tp_trigger_by": "invalid_type"},
    )

    try:
        await client._submit_order(command)

        # Nothing must be sent to the exchange.
        ws_trade_client.submit_order.assert_not_awaited()
        ws_trade_client.batch_place_orders.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_with_invalid_sl_order_type_emits_order_denied(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    """
    An unrecognised sl_order_type value must emit generate_order_denied.
    """
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()
    ws_trade_client.batch_place_orders = AsyncMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.SELL,
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
        params={"stop_loss": "47000.00", "sl_order_type": "Stop"},  # "Stop" is not valid
    )

    try:
        await client._submit_order(command)

        ws_trade_client.submit_order.assert_not_awaited()
        ws_trade_client.batch_place_orders.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.parametrize(
    "params",
    [
        {"take_profit": "nan"},
        {"stop_loss": "inf"},
        {"take_profit": "1e309"},
        {"take_profit": "-1.0"},
        {"take_profit": "abc"},
        {"stop_loss": "not_a_price", "take_profit": "55000.00"},
    ],
)
def test_parse_tp_sl_rejects_invalid_prices(params):
    with pytest.raises(ValueError, match="Invalid Bybit price"):
        _parse_bybit_tp_sl_params(params)


@pytest.mark.parametrize(
    "params",
    [
        {"take_profit": "55000.00", "tp_order_type": "Limit"},
        {"stop_loss": "47000.00", "sl_order_type": "Limit"},
    ],
)
def test_parse_tp_sl_rejects_limit_without_limit_price(params):
    with pytest.raises(
        ValueError,
        match=r"'tp_limit_price' was not provided|'sl_limit_price' was not provided",
    ):
        _parse_bybit_tp_sl_params(params)


@pytest.mark.parametrize(
    "params",
    [
        {"take_profit": "55000.00", "tp_limit_price": "54990.00"},
        {"stop_loss": "47000.00", "sl_limit_price": "46990.00"},
    ],
)
def test_parse_tp_sl_rejects_limit_price_without_limit_type(params):
    with pytest.raises(
        ValueError,
        match=r"requires 'tp_order_type' to be 'Limit'|requires 'sl_order_type' to be 'Limit'",
    ):
        _parse_bybit_tp_sl_params(params)


@pytest.mark.parametrize(
    "params",
    [
        {"tp_trigger_by": "MarkPrice"},
        {"tp_order_type": "Market"},
        {"tp_limit_price": "54990.00", "tp_order_type": "Limit"},
        {"sl_trigger_by": "IndexPrice"},
    ],
)
def test_parse_tp_sl_rejects_orphaned_override_fields(params):
    with pytest.raises(ValueError, match="override fields require"):
        _parse_bybit_tp_sl_params(params)


def test_parse_tp_sl_valid_full_params():
    result = _parse_bybit_tp_sl_params(
        {
            "take_profit": "55000.00",
            "stop_loss": "47000.00",
            "tp_trigger_by": "MarkPrice",
            "sl_trigger_by": "IndexPrice",
            "tp_order_type": "Limit",
            "tp_limit_price": "54990.00",
            "sl_order_type": "Market",
            "close_on_trigger": True,
            "is_leverage": True,
        },
    )
    assert result["take_profit"] == "55000.00"
    assert result["stop_loss"] == "47000.00"
    assert result["tp_trigger_by"] == "MarkPrice"
    assert result["sl_trigger_by"] == "IndexPrice"
    assert result["tp_order_type"] == "Limit"
    assert result["tp_limit_price"] == "54990.00"
    assert result["sl_order_type"] == "Market"
    assert result["close_on_trigger"] is True
    assert result["is_leverage"] is True


def test_parse_tp_sl_valid_bbo_params():
    result = _parse_bybit_tp_sl_params({"bbo_side_type": "counterparty", "bbo_level": 5})

    assert result["bbo_side_type"] == "Counterparty"
    assert result["bbo_level"] == "5"


@pytest.mark.parametrize(
    ("params", "expected"),
    [
        ({"bbo_side_type": "Queue"}, "must be provided together"),
        ({"bbo_level": "1"}, "must be provided together"),
        ({"bbo_side_type": "invalid", "bbo_level": "1"}, "Invalid Bybit BBO side type"),
        ({"bbo_side_type": "Queue", "bbo_level": "6"}, "Invalid Bybit BBO level"),
        ({"bbo_side_type": 1, "bbo_level": "1"}, "Invalid type for 'bbo_side_type'"),
        ({"bbo_side_type": "Queue", "bbo_level": True}, "Invalid type for 'bbo_level'"),
    ],
)
def test_parse_tp_sl_rejects_invalid_bbo_params(params, expected):
    with pytest.raises(ValueError) as exc_info:
        _parse_bybit_tp_sl_params(params)

    assert expected in str(exc_info.value)


def test_parse_tp_sl_none_params_returns_defaults():
    result = _parse_bybit_tp_sl_params(None)
    assert result == {"is_leverage": False}


def test_parse_tp_sl_empty_params_returns_defaults():
    result = _parse_bybit_tp_sl_params({})
    assert result == {"is_leverage": False}


@pytest.mark.parametrize("idx", [0, 1, 2])
def test_parse_tp_sl_position_idx_valid(idx):
    result = _parse_bybit_tp_sl_params({"position_idx": idx})
    assert result["position_idx"] == idx


@pytest.mark.parametrize("idx", [3, -1, "1", True])
def test_parse_tp_sl_position_idx_invalid(idx):
    with pytest.raises(ValueError, match="position_idx"):
        _parse_bybit_tp_sl_params({"position_idx": idx})


@pytest.mark.parametrize(
    ("side", "is_reduce_only", "expected"),
    [
        (OrderSide.BUY, False, nautilus_pyo3.BybitPositionIdx.BUY_HEDGE),
        (OrderSide.SELL, False, nautilus_pyo3.BybitPositionIdx.SELL_HEDGE),
        (OrderSide.SELL, True, nautilus_pyo3.BybitPositionIdx.BUY_HEDGE),
        (OrderSide.BUY, True, nautilus_pyo3.BybitPositionIdx.SELL_HEDGE),
    ],
)
def test_resolve_position_idx_hedge_mode(
    exec_client_builder,
    monkeypatch,
    side,
    is_reduce_only,
    expected,
):
    instrument_id = InstrumentId(Symbol("LTCUSDT-LINEAR"), BYBIT_VENUE)
    client, *_ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "position_mode": {
                instrument_id.symbol.value: nautilus_pyo3.BybitPositionMode.BOTH_SIDES,
            },
        },
    )

    assert client._resolve_position_idx(instrument_id, side, is_reduce_only, None) == expected


def test_resolve_position_idx_one_way_mode(exec_client_builder, monkeypatch):
    instrument_id = InstrumentId(Symbol("LTCUSDT-LINEAR"), BYBIT_VENUE)
    client, *_ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "position_mode": {
                instrument_id.symbol.value: nautilus_pyo3.BybitPositionMode.MERGED_SINGLE,
            },
        },
    )

    assert (
        client._resolve_position_idx(instrument_id, OrderSide.BUY, False, None)
        == nautilus_pyo3.BybitPositionIdx.ONE_WAY
    )


def test_resolve_position_idx_manual_override_wins(exec_client_builder, monkeypatch):
    instrument_id = InstrumentId(Symbol("LTCUSDT-LINEAR"), BYBIT_VENUE)
    client, *_ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "position_mode": {
                instrument_id.symbol.value: nautilus_pyo3.BybitPositionMode.BOTH_SIDES,
            },
        },
    )

    assert (
        client._resolve_position_idx(instrument_id, OrderSide.BUY, False, 2)
        == nautilus_pyo3.BybitPositionIdx.SELL_HEDGE
    )


def test_resolve_position_idx_returns_none_when_unconfigured(exec_client_builder, monkeypatch):
    instrument_id = InstrumentId(Symbol("LTCUSDT-LINEAR"), BYBIT_VENUE)
    client, *_ = exec_client_builder(monkeypatch)

    assert client._resolve_position_idx(instrument_id, OrderSide.BUY, False, None) is None


def test_resolve_position_idx_returns_none_when_symbol_not_in_map(
    exec_client_builder,
    monkeypatch,
):
    instrument_id = InstrumentId(Symbol("LTCUSDT-LINEAR"), BYBIT_VENUE)
    client, *_ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "position_mode": {
                "ETHUSDT-LINEAR": nautilus_pyo3.BybitPositionMode.BOTH_SIDES,
            },
        },
    )

    assert client._resolve_position_idx(instrument_id, OrderSide.BUY, False, None) is None


@pytest.mark.parametrize("symbol", ["BTCUSDT-SPOT", "BTC-30JUN25-100000-C-OPTION"])
def test_resolve_position_idx_returns_none_for_non_derivative_products(
    exec_client_builder,
    monkeypatch,
    symbol,
):
    instrument_id = InstrumentId(Symbol(symbol), BYBIT_VENUE)
    client, *_ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "position_mode": {
                symbol: nautilus_pyo3.BybitPositionMode.MERGED_SINGLE,
            },
        },
    )

    # Manual override must also be ignored for non-derivative products.
    assert client._resolve_position_idx(instrument_id, OrderSide.BUY, False, None) is None
    assert client._resolve_position_idx(instrument_id, OrderSide.BUY, False, 1) is None


@pytest.mark.parametrize(
    ("position_idx", "expected"),
    [
        (None, None),
        (nautilus_pyo3.BybitPositionIdx.ONE_WAY, None),
        (nautilus_pyo3.BybitPositionIdx.BUY_HEDGE, "LTCUSDT-LINEAR.BYBIT-LONG"),
        (nautilus_pyo3.BybitPositionIdx.SELL_HEDGE, "LTCUSDT-LINEAR.BYBIT-SHORT"),
    ],
)
def test_make_hedge_venue_position_id(position_idx, expected):
    instrument_id = nautilus_pyo3.InstrumentId.from_str("LTCUSDT-LINEAR.BYBIT")

    result = nautilus_pyo3.bybit_make_hedge_venue_position_id(instrument_id, position_idx)
    assert (str(result) if result is not None else None) == expected


def test_cache_order_position_id_caches_only_hedge_indexes(exec_client_builder, monkeypatch):
    client, *_ = exec_client_builder(monkeypatch)
    order = MagicMock()
    order.instrument_id = InstrumentId(Symbol("LTCUSDT-LINEAR"), BYBIT_VENUE)
    order.client_order_id = ClientOrderId("O-001")

    client._cache_order_position_id(order, nautilus_pyo3.BybitPositionIdx.BUY_HEDGE)

    assert client._order_position_ids[order.client_order_id] == PositionId(
        "LTCUSDT-LINEAR.BYBIT-LONG",
    )

    client._cache_order_position_id(order, nautilus_pyo3.BybitPositionIdx.ONE_WAY)

    assert order.client_order_id not in client._order_position_ids


def test_handle_order_status_report_caches_venue_position_id(
    exec_client_builder,
    monkeypatch,
    cache,
    instrument,
):
    client, *_ = exec_client_builder(monkeypatch)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-HEDGE-STATUS"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100000"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order.apply(TestEventStubs.order_submitted(order=order))
    cache.add_order(order, None)

    venue_position_id = PositionId("BTCUSDT-SPOT.BYBIT-LONG")
    report = MagicMock()
    report.client_order_id = order.client_order_id
    report.instrument_id = order.instrument_id
    report.venue_order_id = VenueOrderId("BYBIT-ORDER-001")
    report.venue_position_id = venue_position_id
    report.order_status = OrderStatus.ACCEPTED
    report.ts_last = 0
    report.is_order_updated.return_value = False
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.OrderStatusReport.from_pyo3",
        lambda _: report,
    )

    client.generate_order_accepted = MagicMock()

    client._handle_order_status_report_pyo3(MagicMock())

    assert client._order_position_ids[order.client_order_id] == venue_position_id
    client.generate_order_accepted.assert_called_once()


def test_handle_order_status_report_submitted_bbo_emits_accepted_then_updated(
    exec_client_builder,
    monkeypatch,
    cache,
    instrument,
):
    # Arrange
    client, *_ = exec_client_builder(monkeypatch)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-BBO-SUBMITTED"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100000"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order.apply(TestEventStubs.order_submitted(order=order))
    cache.add_order(order, None)

    venue_order_id = VenueOrderId("BYBIT-BBO-001")
    resolved_price = Price.from_str("49995.00")

    report = MagicMock()
    report.client_order_id = order.client_order_id
    report.instrument_id = order.instrument_id
    report.venue_order_id = venue_order_id
    report.venue_position_id = None
    report.order_status = OrderStatus.ACCEPTED
    report.quantity = order.quantity
    report.price = resolved_price
    report.trigger_price = None
    report.ts_last = 0

    def is_order_updated(order_arg):
        return order_arg.price != resolved_price

    report.is_order_updated.side_effect = is_order_updated
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.OrderStatusReport.from_pyo3",
        lambda _: report,
    )

    call_order: list[str] = []
    client.generate_order_accepted = MagicMock(
        side_effect=lambda **_kwargs: call_order.append("accepted"),
    )
    client.generate_order_updated = MagicMock(
        side_effect=lambda **_kwargs: call_order.append("updated"),
    )

    # Act
    client._handle_order_status_report_pyo3(MagicMock())

    # Assert
    assert call_order == ["accepted", "updated"]
    client.generate_order_accepted.assert_called_once_with(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=venue_order_id,
        ts_event=0,
    )
    client.generate_order_updated.assert_called_once_with(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=venue_order_id,
        quantity=order.quantity,
        price=resolved_price,
        trigger_price=None,
        ts_event=0,
    )


def test_handle_order_status_report_accepted_with_diff_emits_only_updated(
    exec_client_builder,
    monkeypatch,
    cache,
    instrument,
):
    # Arrange
    client, *_ = exec_client_builder(monkeypatch)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-AMEND"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100000"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order.apply(TestEventStubs.order_submitted(order=order))
    order.apply(
        TestEventStubs.order_accepted(
            order=order,
            venue_order_id=VenueOrderId("BYBIT-AMEND-001"),
        ),
    )
    cache.add_order(order, None)

    new_price = Price.from_str("50100.00")
    report = MagicMock()
    report.client_order_id = order.client_order_id
    report.instrument_id = order.instrument_id
    report.venue_order_id = order.venue_order_id
    report.venue_position_id = None
    report.order_status = OrderStatus.ACCEPTED
    report.quantity = order.quantity
    report.price = new_price
    report.trigger_price = None
    report.ts_last = 0
    report.is_order_updated.return_value = True
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.OrderStatusReport.from_pyo3",
        lambda _: report,
    )

    client.generate_order_accepted = MagicMock()
    client.generate_order_updated = MagicMock()

    # Act
    client._handle_order_status_report_pyo3(MagicMock())

    # Assert
    client.generate_order_accepted.assert_not_called()
    client.generate_order_updated.assert_called_once()


@pytest.mark.parametrize(
    "terminal_status",
    [OrderStatus.REJECTED, OrderStatus.CANCELED, OrderStatus.EXPIRED],
)
def test_cache_report_position_id_clears_terminal_reports(
    exec_client_builder,
    monkeypatch,
    terminal_status,
):
    client, *_ = exec_client_builder(monkeypatch)
    client_order_id = ClientOrderId("O-HEDGE-TERMINAL")
    report = MagicMock()
    report.client_order_id = client_order_id
    report.venue_position_id = PositionId("BTCUSDT-SPOT.BYBIT-LONG")
    report.order_status = terminal_status
    client._order_position_ids[client_order_id] = report.venue_position_id
    client._order_filled_qty[client_order_id] = Quantity.from_str("0.050000")

    client._cache_report_position_id(report)

    assert client_order_id not in client._order_position_ids
    assert client_order_id not in client._order_filled_qty


def test_cache_report_position_id_filled_preserves_caches_for_pending_fills(
    exec_client_builder,
    monkeypatch,
):
    # FILLED order status reports must not pop the hedge cache or the spot-borrow
    # accumulator. The matching fill report drives both lifecycles.
    client, *_ = exec_client_builder(monkeypatch)
    client_order_id = ClientOrderId("O-HEDGE-FILLED")
    venue_position_id = PositionId("BTCUSDT-SPOT.BYBIT-LONG")
    accumulator = Quantity.from_str("0.050000")
    client._order_position_ids[client_order_id] = venue_position_id
    client._order_filled_qty[client_order_id] = accumulator

    report = MagicMock()
    report.client_order_id = client_order_id
    report.venue_position_id = venue_position_id
    report.order_status = OrderStatus.FILLED

    client._cache_report_position_id(report)

    assert client._order_position_ids[client_order_id] == venue_position_id
    assert client._order_filled_qty[client_order_id] == accumulator


def test_handle_fill_report_uses_cached_position_id_across_partial_fills(
    exec_client_builder,
    monkeypatch,
    msgbus,
    exec_engine,
    cache,
    instrument,
):
    # Drives real generate_order_filled so order.filled_qty is updated synchronously.
    # Locks the multi-partial-fill behavior: position_id stays cached until the order
    # is fully filled, then the cache entry is cleared.
    client, *_ = exec_client_builder(monkeypatch)
    cache.add_instrument(instrument)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-HEDGE-FILL"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100000"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order.apply(TestEventStubs.order_submitted(order=order))
    order.apply(
        TestEventStubs.order_accepted(
            order=order,
            venue_order_id=VenueOrderId("BYBIT-ORDER-001"),
        ),
    )
    cache.add_order(order, None)

    fills_received: list = []

    def apply_fill(event):
        fills_received.append(event)
        order.apply(event)

    msgbus.deregister(endpoint="ExecEngine.process", handler=exec_engine.process)
    msgbus.register(endpoint="ExecEngine.process", handler=apply_fill)

    venue_position_id = PositionId("BTCUSDT-SPOT.BYBIT-LONG")
    client._order_position_ids[order.client_order_id] = venue_position_id

    def fill_report(trade_id: str, last_qty: str) -> nautilus_pyo3.FillReport:
        return nautilus_pyo3.FillReport(
            account_id=nautilus_pyo3.AccountId("BYBIT-UNIFIED"),
            instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
            venue_order_id=nautilus_pyo3.VenueOrderId("BYBIT-ORDER-001"),
            trade_id=nautilus_pyo3.TradeId(trade_id),
            order_side=nautilus_pyo3.OrderSide.BUY,
            last_qty=nautilus_pyo3.Quantity.from_str(last_qty),
            last_px=nautilus_pyo3.Price.from_str("50000.00"),
            commission=nautilus_pyo3.Money.from_str("0.01 USDT"),
            liquidity_side=nautilus_pyo3.LiquiditySide.TAKER,
            ts_event=0,
            client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
            report_id=nautilus_pyo3.UUID4(),
            ts_init=0,
        )

    # Partial fill 1: 0.030 of 0.100
    client._handle_fill_report_pyo3(fill_report("T-001", "0.030000"))
    assert order.filled_qty == Quantity.from_str("0.030000")
    assert client._order_position_ids[order.client_order_id] == venue_position_id

    # Partial fill 2: cumulative 0.060
    client._handle_fill_report_pyo3(fill_report("T-002", "0.030000"))
    assert order.filled_qty == Quantity.from_str("0.060000")
    assert client._order_position_ids[order.client_order_id] == venue_position_id

    # Partial fill 3: cumulative 0.090, still partial, position must remain cached
    client._handle_fill_report_pyo3(fill_report("T-003", "0.030000"))
    assert order.filled_qty == Quantity.from_str("0.090000")
    assert client._order_position_ids[order.client_order_id] == venue_position_id

    # Final fill: cumulative 0.100, order fully filled, cache cleared
    client._handle_fill_report_pyo3(fill_report("T-004", "0.010000"))
    assert order.filled_qty == Quantity.from_str("0.100000")
    assert order.client_order_id not in client._order_position_ids

    assert len(fills_received) == 4
    assert all(fill.position_id == venue_position_id for fill in fills_received)


@pytest.mark.asyncio
async def test_submit_order_with_tp_sl_in_demo_mode_emits_order_denied(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
        config_kwargs={"environment": nautilus_pyo3.BybitEnvironment.DEMO},
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()
    ws_trade_client.batch_place_orders = AsyncMock()

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
        params={"take_profit": "55000.00", "stop_loss": "47000.00"},
    )

    try:
        await client._submit_order(command)

        ws_trade_client.submit_order.assert_not_awaited()
        ws_trade_client.batch_place_orders.assert_not_awaited()
        http_client.submit_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_demo_post_submit_lookup_failure_waits_for_reconciliation(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"environment": nautilus_pyo3.BybitEnvironment.DEMO},
    )

    http_client.submit_order = AsyncMock(
        side_effect=ValueError("No order returned after submission"),
    )
    client.generate_order_submitted = MagicMock()
    client.generate_order_rejected = MagicMock()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-UNKNOWN-OUTCOME"),
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

    await client._submit_order(command)

    client.generate_order_submitted.assert_called_once()
    http_client.submit_order.assert_awaited_once()
    client.generate_order_rejected.assert_not_called()


@pytest.mark.asyncio
async def test_submit_order_demo_confirmed_rejection_emits_order_rejected(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"environment": nautilus_pyo3.BybitEnvironment.DEMO},
    )

    http_client.submit_order = AsyncMock(
        side_effect=ValueError("Order rejected: EC_PostOnlyWillTakeLiquidity"),
    )
    client.generate_order_submitted = MagicMock()
    client.generate_order_rejected = MagicMock()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-CONFIRMED-REJECT"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
        post_only=True,
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

    await client._submit_order(command)

    client.generate_order_submitted.assert_called_once()
    http_client.submit_order.assert_awaited_once()
    client.generate_order_rejected.assert_called_once_with(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        reason="Order rejected: EC_PostOnlyWillTakeLiquidity",
        ts_event=ANY,
        due_post_only=True,
    )


@pytest.mark.asyncio
async def test_submit_order_live_ws_failure_waits_for_reconciliation(
    exec_client_builder,
    monkeypatch,
):
    instrument = create_bybit_linear_perpetual()
    client, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "position_mode": {
                instrument.id.symbol.value: nautilus_pyo3.BybitPositionMode.BOTH_SIDES,
            },
        },
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock(
        side_effect=RuntimeError("Network error: connection closed"),
    )
    client.generate_order_submitted = MagicMock()
    client.generate_order_rejected = MagicMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-LIVE-WS-UNKNOWN-OUTCOME"),
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
        await client._submit_order(command)

        client.generate_order_submitted.assert_called_once()
        ws_trade_client.submit_order.assert_awaited_once()
        client.generate_order_rejected.assert_not_called()
        assert client._order_position_ids[order.client_order_id] == PositionId(
            "BTCUSDT-LINEAR.BYBIT-LONG",
        )
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_list_demo_post_submit_lookup_failure_waits_for_reconciliation(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"environment": nautilus_pyo3.BybitEnvironment.DEMO},
    )

    http_client.submit_order = AsyncMock(
        side_effect=ValueError("No order returned after submission"),
    )
    client.generate_order_submitted = MagicMock()
    client.generate_order_rejected = MagicMock()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-LIST-UNKNOWN-OUTCOME"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    command = SubmitOrderList(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order_list=OrderList(TestIdStubs.order_list_id(), [order]),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    await client._submit_order_list(command)

    client.generate_order_submitted.assert_called_once()
    http_client.submit_order.assert_awaited_once()
    client.generate_order_rejected.assert_not_called()


@pytest.mark.asyncio
async def test_submit_order_list_demo_confirmed_rejection_emits_order_rejected(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"environment": nautilus_pyo3.BybitEnvironment.DEMO},
    )

    http_client.submit_order = AsyncMock(
        side_effect=ValueError("Order rejected: EC_PostOnlyWillTakeLiquidity"),
    )
    client.generate_order_submitted = MagicMock()
    client.generate_order_rejected = MagicMock()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-LIST-CONFIRMED-REJECT"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
        post_only=True,
    )
    command = SubmitOrderList(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order_list=OrderList(TestIdStubs.order_list_id(), [order]),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    await client._submit_order_list(command)

    client.generate_order_submitted.assert_called_once()
    http_client.submit_order.assert_awaited_once()
    client.generate_order_rejected.assert_called_once_with(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        reason="Order rejected: EC_PostOnlyWillTakeLiquidity",
        ts_event=ANY,
        due_post_only=True,
    )


@pytest.mark.asyncio
async def test_submit_order_list_live_ws_failure_waits_for_reconciliation(
    exec_client_builder,
    monkeypatch,
):
    instrument = create_bybit_linear_perpetual()
    client, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "position_mode": {
                instrument.id.symbol.value: nautilus_pyo3.BybitPositionMode.BOTH_SIDES,
            },
        },
    )

    ws_trade_client = client._ws_trade_client
    ws_trade_client.build_place_order_params = MagicMock(return_value=MagicMock())
    ws_trade_client.batch_place_orders = AsyncMock(
        side_effect=RuntimeError("Network error: connection closed"),
    )
    client.generate_order_submitted = MagicMock()
    client.generate_order_rejected = MagicMock()

    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-LIST-LIVE-WS-UNKNOWN-OUTCOME"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    command = SubmitOrderList(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order_list=OrderList(TestIdStubs.order_list_id(), [order]),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        await client._submit_order_list(command)

        client.generate_order_submitted.assert_called_once()
        ws_trade_client.build_place_order_params.assert_called_once()
        ws_trade_client.batch_place_orders.assert_awaited_once()
        client.generate_order_rejected.assert_not_called()
        assert client._order_position_ids[order.client_order_id] == PositionId(
            "BTCUSDT-LINEAR.BYBIT-LONG",
        )
    finally:
        await client._disconnect()


@pytest.mark.parametrize(
    ("exc", "expected"),
    [
        (ValueError("Order rejected: EC_PostOnlyWillTakeLiquidity"), True),
        (RuntimeError("Network error: Timed out after 60000ms"), False),
        (RuntimeError("Request canceled: Adapter disconnecting"), False),
        (RuntimeError("Unexpected HTTP status code 500: server error"), False),
        (RuntimeError("Unexpected HTTP status code 400: bad request"), False),
        (ValueError("No order returned after submission"), False),
        (
            ValueError("Order lookup failed after submission: Bybit error 10001: Request error"),
            False,
        ),
        (ValueError("Bybit error 10000: Server Timeout"), False),
        (ValueError("Bybit error 10001: Request parameter error"), False),
    ],
)
def test_is_confirmed_submit_rejection_error(exc, expected):
    assert _is_confirmed_submit_rejection_error(exc) is expected


@pytest.mark.asyncio
async def test_submit_order_with_nan_price_emits_order_denied(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()
    ws_trade_client.batch_place_orders = AsyncMock()

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
        params={"take_profit": "nan"},
    )

    try:
        await client._submit_order(command)

        ws_trade_client.submit_order.assert_not_awaited()
        ws_trade_client.batch_place_orders.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_without_tp_sl_uses_regular_path(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    """
    Order without TP/SL params routes through the regular submit_order path, not batch.
    """
    client, ws_client, http_client, instrument_provider = exec_client_builder(monkeypatch)

    ws_trade_client = client._ws_trade_client
    ws_trade_client.submit_order = AsyncMock()
    ws_trade_client.batch_place_orders = AsyncMock()

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
        await client._submit_order(command)

        ws_trade_client.submit_order.assert_awaited_once()
        ws_trade_client.batch_place_orders.assert_not_awaited()
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


@pytest.mark.asyncio
async def test_repay_spot_borrow_repays_partial_single(
    monkeypatch,
    exec_client_builder,
    cache,
):
    # Arrange
    # Use TestClock with time outside blackout window (04:00-05:30 UTC) so repayment logic runs
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True, "repay_queue_interval_secs": 0.05},
        clock=test_clock,
    )

    # Create a SPOT instrument
    spot_instrument = CurrencyPair(
        instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
        raw_symbol=Symbol("BTCUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
    )
    cache.add_instrument(spot_instrument)

    # Create a BUY order for SPOT - buying 0.5 BTC
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=spot_instrument.id,
        client_order_id=ClientOrderId("O-PARTIAL-REPAY"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.500000"),  # Buying 0.5 BTC
        price=Price.from_str("50000.00"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    # Mock HTTP client to simulate:
    # - Borrowed amount: 2.0 BTC (more than what we're buying)
    # - We're only buying: 0.5 BTC
    # - Expected repayment: 0.5 BTC (not the full 2.0 BTC)
    http_client.get_spot_borrow_amount = AsyncMock(return_value=Decimal("2.000000"))
    http_client.repay_spot_borrow = AsyncMock()

    # Start the repayment queue processor
    await client._connect()

    try:
        # Create a fill report for the full order (0.5 BTC)
        fill_report = nautilus_pyo3.FillReport(
            account_id=nautilus_pyo3.AccountId("BYBIT-UNIFIED"),
            instrument_id=nautilus_pyo3.InstrumentId.from_str(spot_instrument.id.value),
            venue_order_id=nautilus_pyo3.VenueOrderId("BYBIT-PARTIAL-123"),
            trade_id=nautilus_pyo3.TradeId("T-PARTIAL-001"),
            order_side=nautilus_pyo3.OrderSide.BUY,
            last_qty=nautilus_pyo3.Quantity.from_str("0.500000"),
            last_px=nautilus_pyo3.Price.from_str("50000.00"),
            commission=nautilus_pyo3.Money.from_str("0.025 USDT"),
            liquidity_side=nautilus_pyo3.LiquiditySide.TAKER,
            ts_event=0,
            client_order_id=nautilus_pyo3.ClientOrderId("O-PARTIAL-REPAY"),
            report_id=nautilus_pyo3.UUID4(),
            ts_init=0,
        )

        # Act - Process the fill report
        client._handle_fill_report_pyo3(fill_report)

        # Wait for repayment to be processed
        await eventually(lambda: http_client.repay_spot_borrow.called)

        # Assert - Verify the complete flow executed correctly
        # 1. Borrow amount was checked
        http_client.get_spot_borrow_amount.assert_called_once_with("BTC")

        # 2. Repayment was called with the partial amount (0.5 BTC, not full 2.0 BTC)
        assert http_client.repay_spot_borrow.call_count == 1
        call_args = http_client.repay_spot_borrow.call_args
        assert call_args[0][0] == "BTC"  # Currency
        # Should repay min(borrowed=2.0, bought=0.5) = 0.5
        assert float(call_args[0][1]) == 0.5

        # 3. Order tracking should be cleaned up after full fill
        assert order.client_order_id not in client._order_filled_qty
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_repay_spot_borrow_repays_partial_multiple(
    monkeypatch,
    exec_client_builder,
    cache,
):
    # Arrange
    # Use TestClock with time outside blackout window (04:00-05:30 UTC) so repayment logic runs
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"auto_repay_spot_borrows": True, "repay_queue_interval_secs": 0.05},
        clock=test_clock,
    )

    # Create a SPOT instrument
    spot_instrument = CurrencyPair(
        instrument_id=InstrumentId.from_str("ETHUSDT-SPOT.BYBIT"),
        raw_symbol=Symbol("ETHUSDT"),
        base_currency=ETH,
        quote_currency=USDT,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
    )
    cache.add_instrument(spot_instrument)

    # Create multiple BUY orders for SPOT
    # Initial borrowed amount: 1.0 ETH
    # We'll buy in fractions: +15% (+0.15), +30% (+0.30), +40% (+0.40), +5% (+0.05) = 0.9 ETH total
    # This ensures we never repay the full borrowed amount (1.0 ETH)

    orders = []
    order_ids = [
        ("O-PARTIAL-1", "0.150000"),  # 15% of 1.0 = 0.15 ETH
        ("O-PARTIAL-2", "0.300000"),  # 30% of 1.0 = 0.30 ETH
        ("O-PARTIAL-3", "0.400000"),  # 40% of 1.0 = 0.40 ETH
        ("O-PARTIAL-4", "0.050000"),  # 5% of 1.0 = 0.05 ETH
    ]

    for order_id, qty in order_ids:
        order = LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=spot_instrument.id,
            client_order_id=ClientOrderId(order_id),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str(qty),
            price=Price.from_str("3000.00"),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        cache.add_order(order, None)
        orders.append((order, qty))

    # Mock HTTP client to simulate borrowed amount that decreases after each repayment
    # Initial: 1.0 ETH
    # After 1st repay (0.15): 0.85 ETH
    # After 2nd repay (0.30): 0.55 ETH
    # After 3rd repay (0.40): 0.15 ETH
    # After 4th repay (0.05): 0.10 ETH (never reaches 0, so we never repay full amount)
    borrow_amounts = [
        Decimal("1.000000"),  # Initial
        Decimal("0.850000"),  # After 1st repayment
        Decimal("0.550000"),  # After 2nd repayment
        Decimal("0.150000"),  # After 3rd repayment
        Decimal("0.100000"),  # After 4th repayment
    ]
    http_client.get_spot_borrow_amount = AsyncMock(side_effect=borrow_amounts)
    http_client.repay_spot_borrow = AsyncMock()

    # Start the repayment queue processor
    await client._connect()

    try:
        expected_repayments = []

        # Process each order as a separate fill
        for i, (order, qty) in enumerate(orders):
            fill_report = nautilus_pyo3.FillReport(
                account_id=nautilus_pyo3.AccountId("BYBIT-UNIFIED"),
                instrument_id=nautilus_pyo3.InstrumentId.from_str(spot_instrument.id.value),
                venue_order_id=nautilus_pyo3.VenueOrderId(f"BYBIT-MULTI-{i}"),
                trade_id=nautilus_pyo3.TradeId(f"T-MULTI-{i:03d}"),
                order_side=nautilus_pyo3.OrderSide.BUY,
                last_qty=nautilus_pyo3.Quantity.from_str(qty),
                last_px=nautilus_pyo3.Price.from_str("3000.00"),
                commission=nautilus_pyo3.Money.from_str("0.025 USDT"),
                liquidity_side=nautilus_pyo3.LiquiditySide.TAKER,
                ts_event=0,
                client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
                report_id=nautilus_pyo3.UUID4(),
                ts_init=0,
            )

            # Act - Process the fill report
            client._handle_fill_report_pyo3(fill_report)

            # Track expected repayment for this iteration
            expected_repayments.append(float(qty))

            # Wait for repayment to be processed
            expected_count = len(expected_repayments)
            await eventually(lambda c=expected_count: http_client.repay_spot_borrow.call_count >= c)

        # 1. Borrow amount should be checked 4 times (once for each order)
        assert http_client.get_spot_borrow_amount.call_count == 4

        # 2. Repayment should be called 4 times with partial amounts
        assert http_client.repay_spot_borrow.call_count == 4

        # 3. Verify each repayment amount
        for i, expected_amt in enumerate(expected_repayments):
            call_args = http_client.repay_spot_borrow.call_args_list[i]
            assert call_args[0][0] == "ETH"  # Currency
            # Each repayment should be min(borrowed_amount, bought_qty)
            # Since we always have sufficient borrow, repayment = bought_qty
            assert float(call_args[0][1]) == expected_amt

        # 4. Total repaid should be 0.9 ETH (less than 1.0 ETH borrowed)
        total_repaid = sum(expected_repayments)
        assert total_repaid == 0.9  # 0.15 + 0.30 + 0.40 + 0.05 = 0.90

        # 5. All orders should be cleaned up from tracking
        for order, _ in orders:
            assert order.client_order_id not in client._order_filled_qty
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
        config_kwargs={"auto_repay_spot_borrows": True, "repay_queue_interval_secs": 0.05},
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

        # Wait for repayment check to be called
        await eventually(lambda: http_client.get_spot_borrow_amount.called)

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


# ============================================================================
# MARGIN ACTION TESTS
# ============================================================================


@pytest.mark.asyncio
async def test_query_account_borrow_action_success(
    monkeypatch,
    exec_client_builder,
    msgbus,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.borrow_spot = AsyncMock()

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.BORROW, "coin": "USDT", "amount": 1000},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert
        http_client.borrow_spot.assert_awaited_once()
        call_args = http_client.borrow_spot.call_args
        assert call_args[0][0] == "USDT"
        assert float(call_args[0][1]) == 1000.0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_borrow_action_publishes_result(
    monkeypatch,
    exec_client_builder,
    msgbus,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.borrow_spot = AsyncMock()

    received_data = []

    def handler(data):
        received_data.append(data)

    # Subscribe to the margin borrow result topic
    data_type = DataType(nautilus_pyo3.BybitMarginBorrowResult)
    msgbus.subscribe(topic=f"data.{data_type.topic}", handler=handler)

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.BORROW, "coin": "USDT", "amount": 1000},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert - Result was published and received
        assert len(received_data) == 1
        result = received_data[0]
        assert isinstance(result, nautilus_pyo3.BybitMarginBorrowResult)
        assert result.coin == "USDT"
        assert result.amount == "1000"
        assert result.success is True
        assert result.message == ""
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_borrow_action_publishes_failure_result(
    monkeypatch,
    exec_client_builder,
    msgbus,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.borrow_spot = AsyncMock(side_effect=Exception("Insufficient balance"))

    received_data = []

    def handler(data):
        received_data.append(data)

    data_type = DataType(nautilus_pyo3.BybitMarginBorrowResult)
    msgbus.subscribe(topic=f"data.{data_type.topic}", handler=handler)

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.BORROW, "coin": "USDT", "amount": 1000},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert - Failure result was published
        assert len(received_data) == 1
        result = received_data[0]
        assert isinstance(result, nautilus_pyo3.BybitMarginBorrowResult)
        assert result.success is False
        assert "Insufficient balance" in result.message
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_repay_action_publishes_result(
    monkeypatch,
    exec_client_builder,
    msgbus,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.repay_spot_borrow = AsyncMock()

    received_data = []

    def handler(data):
        received_data.append(data)

    data_type = DataType(nautilus_pyo3.BybitMarginRepayResult)
    msgbus.subscribe(topic=f"data.{data_type.topic}", handler=handler)

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.REPAY, "coin": "USDT", "amount": 500},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert
        assert len(received_data) == 1
        result = received_data[0]
        assert isinstance(result, nautilus_pyo3.BybitMarginRepayResult)
        assert result.coin == "USDT"
        assert result.amount == "500"
        assert result.success is True
        assert result.result_status == "SU"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_get_borrow_amount_publishes_result(
    monkeypatch,
    exec_client_builder,
    msgbus,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.get_spot_borrow_amount = AsyncMock(return_value=Decimal("1234.56"))

    received_data = []

    def handler(data):
        received_data.append(data)

    data_type = DataType(nautilus_pyo3.BybitMarginStatusResult)
    msgbus.subscribe(topic=f"data.{data_type.topic}", handler=handler)

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.GET_BORROW_AMOUNT, "coin": "USDT"},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert
        assert len(received_data) == 1
        result = received_data[0]
        assert isinstance(result, nautilus_pyo3.BybitMarginStatusResult)
        assert result.coin == "USDT"
        assert result.borrow_amount == "1234.56"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_borrow_action_failure(
    monkeypatch,
    exec_client_builder,
    msgbus,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.borrow_spot = AsyncMock(side_effect=Exception("Insufficient balance"))

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.BORROW, "coin": "USDT", "amount": 1000},
    )

    try:
        # Act - Should not raise
        await client._query_account(command)

        # Assert - Borrow was attempted
        http_client.borrow_spot.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_borrow_action_missing_params(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.borrow_spot = AsyncMock()

    # Missing 'amount' param
    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.BORROW, "coin": "USDT"},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert - Borrow should NOT be called due to missing param
        http_client.borrow_spot.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_repay_action_success(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.repay_spot_borrow = AsyncMock()

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.REPAY, "coin": "USDT", "amount": 500},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert
        http_client.repay_spot_borrow.assert_awaited_once()
        call_args = http_client.repay_spot_borrow.call_args
        assert call_args[0][0] == "USDT"
        assert float(call_args[0][1]) == 500.0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_repay_action_repay_all(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 10:00:00", tz="UTC").value)
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.repay_spot_borrow = AsyncMock()

    # No 'amount' param means repay all
    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.REPAY, "coin": "USDT"},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert
        http_client.repay_spot_borrow.assert_awaited_once()
        call_args = http_client.repay_spot_borrow.call_args
        assert call_args[0][0] == "USDT"
        assert call_args[0][1] is None  # None means repay all
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_repay_action_blocked_during_blackout(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    test_clock = TestClock()
    test_clock.set_time(pd.Timestamp("2025-01-15 04:30:00", tz="UTC").value)  # Blackout window
    client, _, http_client, _ = exec_client_builder(monkeypatch, clock=test_clock)
    http_client.repay_spot_borrow = AsyncMock()

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.REPAY, "coin": "USDT", "amount": 500},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert - Repay should NOT be called during blackout window
        http_client.repay_spot_borrow.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_get_borrow_amount_success(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.get_spot_borrow_amount = AsyncMock(return_value=Decimal("1234.56"))

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": nautilus_pyo3.BybitMarginAction.GET_BORROW_AMOUNT, "coin": "USDT"},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert
        http_client.get_spot_borrow_amount.assert_awaited_once_with("USDT")
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_unknown_action_falls_back_to_update_state(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    # Mock _update_account_state directly to avoid complex dict mocking
    update_state_mock = AsyncMock()
    monkeypatch.setattr(client, "_update_account_state", update_state_mock)

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
        params={"action": "unknown_action"},
    )

    try:
        # Act
        await client._query_account(command)

        # Assert - Falls back to update account state
        update_state_mock.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_query_account_no_action_updates_account_state(
    monkeypatch,
    exec_client_builder,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    # Mock _update_account_state directly to avoid complex dict mocking
    update_state_mock = AsyncMock()
    monkeypatch.setattr(client, "_update_account_state", update_state_mock)

    command = QueryAccount(
        trader_id=TestIdStubs.trader_id(),
        account_id=client.account_id,
        command_id=TestIdStubs.uuid(),
        client_id=client.id,
        ts_init=0,
    )

    try:
        # Act
        await client._query_account(command)

        # Assert - No action means update account state
        update_state_mock.assert_awaited_once()
    finally:
        await client._disconnect()
