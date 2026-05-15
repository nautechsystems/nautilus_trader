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
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.execution import HyperliquidExecutionClient
from nautilus_trader.adapters.hyperliquid.execution import _is_transport_error
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import OrderList
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.hyperliquid.conftest import _create_ws_mock


@pytest.fixture
def exec_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        ws_client = _create_ws_mock()
        ws_iter = iter([ws_client])

        monkeypatch.setattr(
            "nautilus_trader.adapters.hyperliquid.execution.nautilus_pyo3.HyperliquidWebSocketClient",
            lambda *args, **kwargs: next(ws_iter),
        )

        # Skip account registration wait in tests
        monkeypatch.setattr(
            "nautilus_trader.adapters.hyperliquid.execution.HyperliquidExecutionClient._await_account_registered",
            AsyncMock(),
        )

        mock_http_client.reset_mock()
        mock_http_client.get_user_address = MagicMock(
            return_value="0x1234567890abcdef1234567890abcdef12345678",
        )
        mock_http_client.get_spot_fill_coin_mapping = MagicMock(return_value={})
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = []

        config = HyperliquidExecClientConfig(
            private_key="0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            **(config_kwargs or {}),
        )

        client = HyperliquidExecutionClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, ws_client, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_account_address_used_for_user_address(exec_client_builder, monkeypatch):
    # Arrange
    agent_account = "0xabcdef1234567890abcdef1234567890abcdef12"
    client, ws_client, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"account_address": agent_account},
    )

    # Assert
    assert client._user_address == agent_account


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
        ws_client.subscribe_order_updates.assert_awaited_once()
        ws_client.subscribe_user_events.assert_awaited_once()
    finally:
        await client._disconnect()

    # Assert
    ws_client.close.assert_awaited_once()


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
    ws_client.close.assert_awaited()


@pytest.mark.asyncio
async def test_account_id_set_on_initialization(exec_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    # Assert - Hyperliquid sets account_id during initialization
    assert client.account_id.value == "HYPERLIQUID-master"

    # Act - connect should not change the account_id
    await client._connect()

    try:
        # Assert
        assert client.account_id.value == "HYPERLIQUID-master"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_generate_order_status_reports_converts_results(
    exec_client_builder,
    monkeypatch,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    expected_report.client_order_id = ClientOrderId("O-123")
    expected_report.venue_order_id = None
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.OrderStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    pyo3_report = MagicMock()
    http_client.request_order_status_reports.return_value = [pyo3_report]

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
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
async def test_generate_order_status_reports_handles_failure(
    exec_client_builder,
    monkeypatch,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_order_status_reports.side_effect = Exception("boom")

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
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
@pytest.mark.parametrize(
    ("client_order_id", "venue_order_id"),
    [
        (ClientOrderId("O-1"), VenueOrderId("123")),
        (ClientOrderId("O-2"), None),
        (None, VenueOrderId("456")),
    ],
)
async def test_generate_order_status_report_forwards_identifiers(
    exec_client_builder,
    monkeypatch,
    client_order_id,
    venue_order_id,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    expected_report.client_order_id = client_order_id
    expected_report.venue_order_id = venue_order_id
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.OrderStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    pyo3_report = MagicMock()
    http_client.request_order_status_report.return_value = pyo3_report

    command = GenerateOrderStatusReport(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    report = await client.generate_order_status_report(command)

    # Assert
    http_client.request_order_status_report.assert_awaited_once_with(
        venue_order_id=venue_order_id.value if venue_order_id else None,
        client_order_id=client_order_id.value if client_order_id else None,
    )
    assert report is expected_report


@pytest.mark.asyncio
async def test_generate_order_status_report_returns_none_when_helper_returns_none(
    exec_client_builder,
    monkeypatch,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_order_status_report.return_value = None

    command = GenerateOrderStatusReport(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        client_order_id=ClientOrderId("O-9"),
        venue_order_id=VenueOrderId("999"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    report = await client.generate_order_status_report(command)

    # Assert
    http_client.request_order_status_report.assert_awaited_once()
    assert report is None


@pytest.mark.asyncio
async def test_generate_order_status_report_requires_identifier(
    exec_client_builder,
    monkeypatch,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    command = GenerateOrderStatusReport(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        client_order_id=None,
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    report = await client.generate_order_status_report(command)

    # Assert
    http_client.request_order_status_report.assert_not_awaited()
    assert report is None


@pytest.mark.asyncio
async def test_generate_order_status_report_handles_failure(
    exec_client_builder,
    monkeypatch,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_order_status_report.side_effect = Exception("boom")

    command = GenerateOrderStatusReport(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        client_order_id=ClientOrderId("O-10"),
        venue_order_id=VenueOrderId("1000"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    report = await client.generate_order_status_report(command)

    # Assert
    assert report is None


@pytest.mark.asyncio
async def test_generate_fill_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    expected_report.client_order_id = ClientOrderId("O-123")
    expected_report.venue_order_id = None
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.FillReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_fill_reports.return_value = [MagicMock()]

    command = GenerateFillReports(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
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
async def test_generate_fill_reports_handles_failure(exec_client_builder, monkeypatch):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_fill_reports.side_effect = Exception("boom")

    command = GenerateFillReports(
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        venue_order_id=None,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_fill_reports(command)

    # Assert
    assert reports == []


@pytest.mark.asyncio
async def test_generate_position_status_reports_converts_results(
    exec_client_builder,
    monkeypatch,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.PositionStatusReport.from_pyo3",
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
async def test_generate_position_status_reports_handles_failure(
    exec_client_builder,
    monkeypatch,
):
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


@pytest.mark.asyncio
async def test_submit_limit_order(exec_client_builder, monkeypatch, instrument):
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
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
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

        # Assert - Hyperliquid uses HTTP for order submission
        http_client.submit_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_rejection(exec_client_builder, monkeypatch, instrument):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    client.generate_order_rejected = MagicMock()
    http_client.submit_order.side_effect = Exception("Order rejected: Insufficient margin")

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
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
        # Act - Should not raise, but handle gracefully
        await client._submit_order(command)

        # Assert - Order rejection is emitted with the venue reason
        http_client.submit_order.assert_awaited_once()
        client.generate_order_rejected.assert_called_once()
        reason = client.generate_order_rejected.call_args.kwargs["reason"]
        assert "Insufficient margin" in reason
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order_by_client_id(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
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
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
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

        # Assert - Hyperliquid uses HTTP for order cancellation
        http_client.cancel_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order_by_venue_id(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
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
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("HYPERLIQUID-12345"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._cancel_order(command)

        # Assert
        http_client.cancel_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_order_rejection(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    # Arrange
    client, ws_client, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )
    await client._connect()

    client.generate_order_cancel_rejected = MagicMock()
    http_client.cancel_order.side_effect = Exception("Order already filled")

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("HYPERLIQUID-12345"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act - Should not raise
        await client._cancel_order(command)

        # Assert - Cancel rejection is emitted with the venue reason
        http_client.cancel_order.assert_awaited_once()
        client.generate_order_cancel_rejected.assert_called_once()
        reason = client.generate_order_cancel_rejected.call_args.kwargs["reason"]
        assert "Order already filled" in reason
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_all_orders_no_open_orders(
    exec_client_builder,
    monkeypatch,
    instrument,
):
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

    try:
        # Act
        await client._cancel_all_orders(command)

        # Assert - No orders to cancel means no HTTP calls
        http_client.cancel_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("side", "trigger_str", "quote_bid", "quote_ask"),
    [
        # SELL stop far below current market
        (OrderSide.SELL, "95000.0", "100000.0", "100001.0"),
        # BUY stop far above current market
        (OrderSide.BUY, "105000.0", "100000.0", "100001.0"),
        # SELL stop close to market
        (OrderSide.SELL, "99500.0", "100000.0", "100001.0"),
        # BUY stop close to market
        (OrderSide.BUY, "100500.0", "100000.0", "100001.0"),
    ],
)
async def test_submit_stop_market_derives_price_from_trigger(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
    side,
    trigger_str,
    quote_bid,
    quote_ask,
):
    """
    Verify limit_px is derived from trigger_price, not the current quote.
    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    quote = QuoteTick(
        instrument_id=instrument.id,
        bid_price=Price.from_str(quote_bid),
        ask_price=Price.from_str(quote_ask),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    cache.add_quote_tick(quote)

    order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=side,
        quantity=Quantity.from_str("0.00100"),
        trigger_price=Price.from_str(trigger_str),
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

        # Assert
        http_client.submit_order.assert_awaited_once()
        call_kwargs = http_client.submit_order.call_args.kwargs
        submitted_price = Decimal(str(call_kwargs["price"]))
        trigger_price = Decimal(trigger_str)

        # Hyperliquid constraint: SELL limit_px <= triggerPx, BUY >= triggerPx
        if side == OrderSide.SELL:
            assert submitted_price <= trigger_price
        else:
            assert submitted_price >= trigger_price

        # Price is derived from trigger (within 1%), not from the distant quote
        assert abs(submitted_price - trigger_price) / trigger_price < Decimal("0.01")

        # Decimal places must not exceed instrument price_precision
        price_str = str(submitted_price)
        if "." in price_str:
            actual_decimals = len(price_str.split(".")[1])
        else:
            actual_decimals = 0
        assert actual_decimals <= instrument.price_precision

        # Trigger price is forwarded to the HTTP client
        assert call_kwargs["trigger_price"] is not None
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("side", "trigger_str", "quote_bid", "quote_ask"),
    [
        # SELL stop below market (ETH precision=2)
        (OrderSide.SELL, "2470.00", "2600.00", "2601.00"),
        # BUY stop above market (ETH precision=2)
        (OrderSide.BUY, "2750.00", "2600.00", "2601.00"),
    ],
)
async def test_submit_stop_market_eth_derives_price_from_trigger(
    exec_client_builder,
    monkeypatch,
    eth_instrument,
    cache,
    side,
    trigger_str,
    quote_bid,
    quote_ask,
):
    """
    Verify trigger-based derivation with ETH (price_precision=2).
    """
    # Arrange
    cache.add_instrument(eth_instrument)
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    quote = QuoteTick(
        instrument_id=eth_instrument.id,
        bid_price=Price.from_str(quote_bid),
        ask_price=Price.from_str(quote_ask),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    cache.add_quote_tick(quote)

    order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=eth_instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=side,
        quantity=Quantity.from_str("0.0100"),
        trigger_price=Price.from_str(trigger_str),
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

        # Assert
        http_client.submit_order.assert_awaited_once()
        call_kwargs = http_client.submit_order.call_args.kwargs
        submitted_price = Decimal(str(call_kwargs["price"]))
        trigger_price = Decimal(trigger_str)

        if side == OrderSide.SELL:
            assert submitted_price <= trigger_price
        else:
            assert submitted_price >= trigger_price

        assert abs(submitted_price - trigger_price) / trigger_price < Decimal("0.01")

        # Decimal places must not exceed instrument price_precision
        price_str = str(submitted_price)
        if "." in price_str:
            actual_decimals = len(price_str.split(".")[1])
        else:
            actual_decimals = 0
        assert actual_decimals <= eth_instrument.price_precision
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("side", "trigger_str", "quote_bid", "quote_ask"),
    [
        # SELL MIT below current market
        (OrderSide.SELL, "95000.0", "100000.0", "100001.0"),
        # BUY MIT above current market
        (OrderSide.BUY, "105000.0", "100000.0", "100001.0"),
    ],
)
async def test_submit_market_if_touched_derives_price_from_trigger(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
    side,
    trigger_str,
    quote_bid,
    quote_ask,
):
    """
    Verify MarketIfTouched also derives limit_px from trigger_price.
    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    quote = QuoteTick(
        instrument_id=instrument.id,
        bid_price=Price.from_str(quote_bid),
        ask_price=Price.from_str(quote_ask),
        bid_size=Quantity.from_int(1),
        ask_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    cache.add_quote_tick(quote)

    order = MarketIfTouchedOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=side,
        quantity=Quantity.from_str("0.00100"),
        trigger_price=Price.from_str(trigger_str),
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

        # Assert
        http_client.submit_order.assert_awaited_once()
        call_kwargs = http_client.submit_order.call_args.kwargs
        submitted_price = Decimal(str(call_kwargs["price"]))
        trigger_price = Decimal(trigger_str)

        if side == OrderSide.SELL:
            assert submitted_price <= trigger_price
        else:
            assert submitted_price >= trigger_price

        assert abs(submitted_price - trigger_price) / trigger_price < Decimal("0.01")

        price_str = str(submitted_price)
        if "." in price_str:
            actual_decimals = len(price_str.split(".")[1])
        else:
            actual_decimals = 0
        assert actual_decimals <= instrument.price_precision
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_list_calls_batch_path(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Verify _submit_order_list uses the batch submit_orders path.
    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-BATCH-001"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.00100"),
        trigger_price=Price.from_str("95000.0"),
        trigger_type=TriggerType.LAST_PRICE,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    order_list = OrderList(
        order_list_id=OrderListId("OL-001"),
        orders=[order],
    )

    command = SubmitOrderList(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order_list=order_list,
        position_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order_list(command)

        # Assert - batch path calls submit_orders, not submit_order
        http_client.submit_orders.assert_awaited_once()
        http_client.submit_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_limit_order(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        quantity=Quantity.from_str("0.00200"),
        price=Price.from_str("51000.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert
        http_client.modify_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_after_partial_fill_sends_remaining_qty(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Hyperliquid modify is cancel-replace; the new venue order must carry the engine's
    remaining quantity (target_total - already_filled), not the absolute total.
    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PF-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    submitted = TestEventStubs.order_submitted(order=order)
    order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=order,
        venue_order_id=VenueOrderId("12345"),
    )
    order.apply(accepted)
    fill = TestEventStubs.order_filled(
        order=order,
        instrument=instrument,
        last_qty=Quantity.from_str("0.00040"),
        last_px=Price.from_str("50000.0"),
    )
    order.apply(fill)
    cache.add_order(order, None)
    assert order.filled_qty == Quantity.from_str("0.00040")

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        # Same absolute total as the original order; the venue must receive
        # `target_total - filled = 0.00060`, not `0.00100`.
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("51000.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert
        http_client.modify_order.assert_awaited_once()
        sent_quantity = http_client.modify_order.await_args.kwargs["quantity"]
        assert sent_quantity == nautilus_pyo3.Quantity.from_str("0.00060")
        # Marker tracks the user-intended absolute total so the WS
        # cancel-replace promotion can emit OrderUpdated with that value.
        assert client._pending_modify_target_qty[order.client_order_id.value] == Quantity.from_str(
            "0.00100",
        )
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_rejected_when_target_qty_not_greater_than_filled(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    The adapter rejects a modify when the target absolute quantity is at or below the
    order's already-filled quantity, since Hyperliquid cancel-replace cannot represent a
    non-positive replacement size.
    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PF-002"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    submitted = TestEventStubs.order_submitted(order=order)
    order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=order,
        venue_order_id=VenueOrderId("12345"),
    )
    order.apply(accepted)
    fill = TestEventStubs.order_filled(
        order=order,
        instrument=instrument,
        last_qty=Quantity.from_str("0.00050"),
        last_px=Price.from_str("50000.0"),
    )
    order.apply(fill)
    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        quantity=Quantity.from_str("0.00050"),  # equal to filled, not greater
        price=Price.from_str("51000.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert - rejected, no HTTP call
        http_client.modify_order.assert_not_awaited()
        assert order.client_order_id.value not in client._pending_modify_keys
        assert order.client_order_id.value not in client._pending_modify_target_qty
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_rejected_when_not_in_cache(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    command = ModifyOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-UNKNOWN"),
        venue_order_id=VenueOrderId("12345"),
        quantity=Quantity.from_str("0.00200"),
        price=Price.from_str("51000.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert - rejected, no HTTP call
        http_client.modify_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_rejected_when_no_venue_order_id(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    # No venue_order_id on order or command
    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        quantity=Quantity.from_str("0.00200"),
        price=Price.from_str("51000.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert - rejected, no HTTP call
        http_client.modify_order.assert_not_awaited()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_stop_market_uses_trigger_price_as_fallback(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    StopMarket has no limit price; trigger_price is used as the price field.
    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-STOP-001"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.00100"),
        trigger_price=Price.from_str("95000.0"),
        trigger_type=TriggerType.LAST_PRICE,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        quantity=Quantity.from_str("0.00200"),
        price=None,
        trigger_price=Price.from_str("94000.0"),
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        # Act
        await client._modify_order(command)

        # Assert
        http_client.modify_order.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_rejection_on_http_error(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    http_client.modify_order.side_effect = Exception("Modify rejected: Invalid order")

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    cache.add_order(order, None)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        quantity=Quantity.from_str("0.00200"),
        price=Price.from_str("51000.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        # Act - should not raise
        await client._modify_order(command)

        # Assert - rejection handled internally, no stale in-flight marker
        http_client.modify_order.assert_awaited_once()
        assert order.client_order_id.value not in client._pending_modify_keys
        assert order.client_order_id.value not in client._pending_modify_target_qty
    finally:
        await client._disconnect()


def _build_status_report_pyo3(
    client,
    instrument,
    client_order_id,
    venue_order_id,
    order_status,
    price,
    quantity,
):
    return nautilus_pyo3.OrderStatusReport(
        account_id=nautilus_pyo3.AccountId(client.account_id.value),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        venue_order_id=nautilus_pyo3.VenueOrderId(venue_order_id),
        client_order_id=nautilus_pyo3.ClientOrderId(client_order_id.value),
        order_side=nautilus_pyo3.OrderSide.BUY,
        order_type=nautilus_pyo3.OrderType.LIMIT,
        time_in_force=nautilus_pyo3.TimeInForce.GTC,
        order_status=order_status,
        quantity=nautilus_pyo3.Quantity.from_str(quantity),
        filled_qty=nautilus_pyo3.Quantity.from_str("0"),
        price=nautilus_pyo3.Price.from_str(price) if price is not None else None,
        ts_accepted=0,
        ts_last=0,
        report_id=nautilus_pyo3.UUID4(),
        ts_init=0,
    )


@pytest.mark.asyncio
async def test_modify_order_cancel_replace_emits_updated_not_canceled(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Reproduces GH-3827.

    Hyperliquid replies to a modify with ACCEPTED(new venue_order_id) followed by
    CANCELED(old venue_order_id), both under the same client_order_id. The adapter must
    emit a single OrderUpdated for the replacement leg and suppress the stale CANCELED
    for the old leg. Detection is based on the cached venue_order_id diverging from the
    report's venue_order_id.

    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-20260409-080047-001-000-1"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("375273671786")
    new_voi = VenueOrderId("375273716474")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="53893.0",
        quantity="0.00020",
    )
    canceled_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        old_voi.value,
        nautilus_pyo3.OrderStatus.CANCELED,
        price="56730.0",
        quantity="0.00020",
    )

    try:
        # Act
        client._handle_order_status_report_pyo3(accepted_report)
        client._handle_order_status_report_pyo3(canceled_report)

        # Assert
        updated_events = [e for e in captured if isinstance(e, OrderUpdated)]
        canceled_events = [e for e in captured if isinstance(e, OrderCanceled)]

        assert len(updated_events) == 1
        assert updated_events[0].venue_order_id == new_voi
        assert updated_events[0].price == Price.from_str("53893.0")
        assert updated_events[0].quantity == Quantity.from_str("0.00020")
        assert len(canceled_events) == 0

        assert cache.venue_order_id(order.client_order_id) == new_voi
        assert order.client_order_id.value not in client._terminal_orders
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_cancel_replace_uses_target_qty_after_partial_fill(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    The cancel-replace ACCEPTED must emit OrderUpdated with the user's absolute total,
    not the venue's remaining-quantity view.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PF-CR-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("11111")
    new_voi = VenueOrderId("22222")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)

    # Simulate the in-flight modify state set by `_modify_order` for a target
    # absolute total of 0.00100 with a prior fill of 0.00040.
    target_total_qty = Quantity.from_str("0.00100")
    venue_remaining = "0.00060"
    client._pending_modify_keys[order.client_order_id.value] = old_voi.value
    client._pending_modify_target_qty[order.client_order_id.value] = target_total_qty

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="51000.0",
        quantity=venue_remaining,
    )

    try:
        # Act
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert
        updated_events = [e for e in captured if isinstance(e, OrderUpdated)]
        assert len(updated_events) == 1
        assert updated_events[0].venue_order_id == new_voi
        # OrderUpdated carries the engine's absolute total, not the venue's
        # remaining-quantity view (would be 0.00060).
        assert updated_events[0].quantity == target_total_qty

        assert order.client_order_id.value not in client._pending_modify_keys
        assert order.client_order_id.value not in client._pending_modify_target_qty
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_cancel_replace_falls_back_to_cached_order_price(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    When a cancel-replace ACCEPTED report omits the `price` field, the adapter must fall
    back to the cached order's price so the emitted `OrderUpdated` still carries an
    accurate price.

    See GH-3827.

    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-FALLBACK-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("5000")
    new_voi = VenueOrderId("5001")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price=None,
        quantity="0.00020",
    )

    try:
        # Act
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert
        updated_events = [e for e in captured if isinstance(e, OrderUpdated)]
        assert len(updated_events) == 1
        assert updated_events[0].venue_order_id == new_voi
        assert updated_events[0].price == order.price
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_recovers_after_timed_out_modify(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    If a modify HTTP call fails (transport timeout or wrapped error) but the exchange
    actually accepted the modify, the eventual WS ACCEPTED(new_voi) must still be
    translated into an OrderUpdated.

    The adapter relies purely on the cached venue_order_id diverging from the report's
    venue_order_id, so no in-flight state tracking is needed.

    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    http_client.modify_order.side_effect = ValueError(
        "error sending request for url (...): operation timed out",
    )

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-TO-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("2222")
    new_voi = VenueOrderId("3333")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)

    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=old_voi,
        quantity=order.quantity,
        price=Price.from_str("53893.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="53893.0",
        quantity="0.00020",
    )

    try:
        # Act - modify call fails, but venue delivers the replacement via WS
        await client._modify_order(command)
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert
        updated_events = [e for e in captured if isinstance(e, OrderUpdated)]
        assert len(updated_events) == 1
        assert updated_events[0].venue_order_id == new_voi
        assert cache.venue_order_id(order.client_order_id) == new_voi
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_modify_order_cancel_replace_handles_cancel_before_accept(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    If Hyperliquid delivers CANCELED(old_voi) before the replacement ACCEPTED(new_voi)
    for an in-flight modify, the adapter must suppress the old leg's cancel and still
    route the subsequent ACCEPTED as OrderUpdated.

    The pending-modify marker is populated before the modify HTTP call and cleared on
    failure, so the race branch never fires on a failed modify.

    """
    # Arrange
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-RACE-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("1000")
    new_voi = VenueOrderId("2000")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)

    modify_command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=old_voi,
        quantity=order.quantity,
        price=Price.from_str("53893.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    canceled_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        old_voi.value,
        nautilus_pyo3.OrderStatus.CANCELED,
        price="56730.0",
        quantity="0.00020",
    )
    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="53893.0",
        quantity="0.00020",
    )

    try:
        # Act - modify call populates the pending marker before the HTTP await
        await client._modify_order(modify_command)
        assert client._pending_modify_keys[order.client_order_id.value] == old_voi.value

        # CANCELED(old_voi) arrives before the replacement ACCEPTED
        client._handle_order_status_report_pyo3(canceled_report)
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert
        updated_events = [e for e in captured if isinstance(e, OrderUpdated)]
        canceled_events = [e for e in captured if isinstance(e, OrderCanceled)]

        assert len(updated_events) == 1
        assert updated_events[0].venue_order_id == new_voi
        assert len(canceled_events) == 0
        assert cache.venue_order_id(order.client_order_id) == new_voi
        assert order.client_order_id.value not in client._terminal_orders
        assert order.client_order_id.value not in client._pending_modify_keys
    finally:
        await client._disconnect()


def _build_canceled_event_pyo3(
    client,
    instrument,
    client_order_id,
    venue_order_id,
):
    event = OrderCanceled(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        account_id=client.account_id,
        event_id=TestIdStubs.uuid(),
        ts_event=0,
        ts_init=0,
    )
    return nautilus_pyo3.OrderCanceled.from_dict(OrderCanceled.to_dict(event))


@pytest.mark.asyncio
async def test_handle_order_canceled_pyo3_suppresses_cancel_before_accept(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    The WS direct OrderCanceled handler must suppress the old leg of an in-flight
    cancel-replace modify so a spurious OrderCanceled does not fire while
    `_pending_modify_keys` still tracks the old venue_order_id.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-WS-RACE-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("3000")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)
    client._pending_modify_keys[order.client_order_id.value] = old_voi.value

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    pyo3_event = _build_canceled_event_pyo3(client, instrument, order.client_order_id, old_voi)

    try:
        # Act
        client._handle_order_canceled_pyo3(pyo3_event)

        # Assert
        assert captured == []
        assert order.client_order_id.value not in client._terminal_orders
        assert client._pending_modify_keys[order.client_order_id.value] == old_voi.value
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_order_canceled_pyo3_suppresses_stale_cancel_after_replacement(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Once the replacement ACCEPTED has advanced the cached venue_order_id past the WS
    direct CANCELED's venue_order_id, the handler must drop the stale leg even though
    `_pending_modify_keys` was already cleared by the OrderUpdated path.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-WS-STALE-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("4000")
    new_voi = VenueOrderId("5000")
    cache.add_venue_order_id(order.client_order_id, new_voi)
    client._accepted_orders.add(order.client_order_id.value)

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    pyo3_event = _build_canceled_event_pyo3(client, instrument, order.client_order_id, old_voi)

    try:
        # Act
        client._handle_order_canceled_pyo3(pyo3_event)

        # Assert
        assert captured == []
        assert order.client_order_id.value not in client._terminal_orders
        assert cache.venue_order_id(order.client_order_id) == new_voi
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_order_canceled_pyo3_emits_for_genuine_cancel(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    A genuine WS CANCELED with no in-flight modify and a matching cached venue_order_id
    must still emit OrderCanceled and mark the order terminal.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-WS-CXL-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    voi = VenueOrderId("6000")
    cache.add_venue_order_id(order.client_order_id, voi)
    client._accepted_orders.add(order.client_order_id.value)

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    pyo3_event = _build_canceled_event_pyo3(client, instrument, order.client_order_id, voi)

    try:
        # Act
        client._handle_order_canceled_pyo3(pyo3_event)

        # Assert
        canceled_events = [e for e in captured if isinstance(e, OrderCanceled)]
        assert len(canceled_events) == 1
        assert canceled_events[0].venue_order_id == voi
        assert order.client_order_id.value in client._terminal_orders
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_after_failed_modify_still_emits_canceled(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    If a modify fails and the strategy subsequently cancels the unchanged order, the
    eventual CANCELED(old_voi) must still emit OrderCanceled.

    The stale cancel suppression only kicks in once the cached venue_order_id has been
    advanced to a different value by a replacement ACCEPTED.

    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-CXL-003"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    voi = VenueOrderId("9999")
    cache.add_venue_order_id(order.client_order_id, voi)
    client._accepted_orders.add(order.client_order_id.value)

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    canceled_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        nautilus_pyo3.OrderStatus.CANCELED,
        price="56730.0",
        quantity="0.00020",
    )

    try:
        # Act
        client._handle_order_status_report_pyo3(canceled_report)

        # Assert
        canceled_events = [e for e in captured if isinstance(e, OrderCanceled)]
        assert len(canceled_events) == 1
        assert canceled_events[0].venue_order_id == voi
        assert order.client_order_id.value in client._terminal_orders
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_submit_order_list_converts_to_pyo3(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    """
    Verify _submit_order_list converts Cython orders to PyO3 before passing them to the
    Rust client (regression test for GH-3763).
    """
    # Arrange
    client, ws_client, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    entry = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-20260328-001-000-1"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    take_profit = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-20260328-001-000-2"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("55000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    stop_loss = StopMarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-20260328-001-000-3"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.00100"),
        trigger_price=Price.from_str("45000.0"),
        trigger_type=TriggerType.LAST_PRICE,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    order_list = OrderList(
        order_list_id=OrderListId("OL-001"),
        orders=[entry, take_profit, stop_loss],
    )

    command = SubmitOrderList(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        order_list=order_list,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
    )

    try:
        # Act
        await client._submit_order_list(command)

        # Assert
        http_client.submit_orders.assert_awaited_once()
        submitted = http_client.submit_orders.call_args[0][0]

        assert len(submitted) == 3
        assert isinstance(submitted[0], nautilus_pyo3.MarketOrder)
        assert isinstance(submitted[1], nautilus_pyo3.LimitOrder)
        assert isinstance(submitted[2], nautilus_pyo3.StopMarketOrder)
    finally:
        await client._disconnect()


def _build_fill_report_pyo3(
    client,
    instrument,
    client_order_id,
    venue_order_id,
    trade_id,
    last_qty,
    last_px,
):
    return nautilus_pyo3.FillReport(
        account_id=nautilus_pyo3.AccountId(client.account_id.value),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        venue_order_id=nautilus_pyo3.VenueOrderId(venue_order_id),
        trade_id=nautilus_pyo3.TradeId(trade_id),
        order_side=nautilus_pyo3.OrderSide.BUY,
        last_qty=nautilus_pyo3.Quantity.from_str(last_qty),
        last_px=nautilus_pyo3.Price.from_str(last_px),
        commission=nautilus_pyo3.Money.from_str("0.00 USD"),
        liquidity_side=nautilus_pyo3.LiquiditySide.TAKER,
        ts_event=0,
        client_order_id=nautilus_pyo3.ClientOrderId(client_order_id.value),
        report_id=nautilus_pyo3.UUID4(),
        ts_init=0,
    )


@pytest.mark.asyncio
async def test_handle_fill_report_buffers_during_cancel_replace(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Reproduces the fill leg of GH-3972.

    A fill carrying the replacement's new venue_order_id can arrive on the WebSocket
    before the matching ACCEPTED has been promoted to OrderUpdated. The handler must
    buffer the fill (no OrderFilled, trade_id not consumed by the dedup set) so the
    engine never sees a fill against stale local order state.

    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-FILL-RACE-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("9000")
    new_voi = VenueOrderId("9001")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)
    client._pending_modify_keys[order.client_order_id.value] = old_voi.value

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        "T-RACE-1",
        "0.00020",
        "53893.0",
    )

    try:
        # Act
        client._handle_fill_report_pyo3(fill)

        # Assert
        assert captured == []
        assert client._buffered_fills[order.client_order_id.value] == [fill]
        assert "T-RACE-1" not in client._processed_trade_ids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_replace_accepted_drains_buffered_fill(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    The replacement ACCEPTED branch must drain any FillReports buffered during the
    cancel-replace window so OrderFilled is emitted in order after the OrderUpdated that
    advances the cached venue_order_id (GH-3972).
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-FILL-RACE-002"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("9100")
    new_voi = VenueOrderId("9101")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)
    client._pending_modify_keys[order.client_order_id.value] = old_voi.value

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        "T-RACE-2",
        "0.00020",
        "53893.0",
    )
    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="53893.0",
        quantity="0.00020",
    )

    try:
        # Act - fill arrives first, then the replacement ACCEPTED drains it
        client._handle_fill_report_pyo3(fill)
        assert order.client_order_id.value in client._buffered_fills
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert
        updated_events = [e for e in captured if isinstance(e, OrderUpdated)]
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]

        assert len(updated_events) == 1
        assert updated_events[0].venue_order_id == new_voi
        assert len(filled_events) == 1
        assert filled_events[0].venue_order_id == new_voi
        assert filled_events[0].last_qty == Quantity.from_str("0.00020")
        assert filled_events[0].last_px == Price.from_str("53893.0")

        # Ordering: OrderUpdated must precede the drained OrderFilled.
        update_index = next(i for i, e in enumerate(captured) if isinstance(e, OrderUpdated))
        fill_index = next(i for i, e in enumerate(captured) if isinstance(e, OrderFilled))
        assert update_index < fill_index

        assert order.client_order_id.value not in client._buffered_fills
        assert order.client_order_id.value not in client._pending_modify_keys
        assert cache.venue_order_id(order.client_order_id) == new_voi
        assert "T-RACE-2" in client._processed_trade_ids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_fill_report_passes_through_when_voi_matches_cached(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    A fill whose venue_order_id matches the cached value must not be buffered
    even if a modify is in flight: it belongs to the still-current leg and the
    engine can apply it against the live local order state (GH-3972).
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-FILL-RACE-003"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("9200")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)
    client._pending_modify_keys[order.client_order_id.value] = old_voi.value

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        old_voi.value,
        "T-RACE-3",
        "0.00020",
        "56730.0",
    )

    try:
        # Act
        client._handle_fill_report_pyo3(fill)

        # Assert
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].venue_order_id == old_voi
        assert order.client_order_id.value not in client._buffered_fills
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_stale_old_leg_fill_after_cancel_replace_falls_through(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    GH-3972 regression guard.

    A delayed old-leg fill arriving after the cancel-replace promotion has
    already advanced the cached venue_order_id must NOT be buffered. Buffering
    it would strand the fill forever (no further ACCEPTED on this cid would
    drain it). The `_pending_modify_keys` requirement is what prevents this:
    the cancel-replace ACCEPTED clears the marker, so the buffer guard does
    not fire on cached_voi mismatch alone. The fill falls through and emits
    `OrderFilled` with the (now stale) old VOI; the engine rejects on
    venue_order_id mismatch and reconciliation recovers from there.

    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-FILL-STALE"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("STALE-OLD")
    new_voi = VenueOrderId("STALE-NEW")
    # Cancel-replace already promoted: cached_voi advanced and the marker was
    # cleared on the ACCEPTED.
    cache.add_venue_order_id(order.client_order_id, new_voi)
    client._accepted_orders.add(order.client_order_id.value)
    assert order.client_order_id.value not in client._pending_modify_keys

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    # Delayed old-leg fill arrives via WS reordering across feeds.
    fill = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        old_voi.value,
        "T-STALE-1",
        "0.00020",
        "56730.0",
    )

    try:
        # Act
        client._handle_fill_report_pyo3(fill)

        # Assert: the fill must not be buffered (would strand forever);
        # it falls through to normal emission with the old VOI.
        assert order.client_order_id.value not in client._buffered_fills
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].venue_order_id == old_voi
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_cancel_replace_drains_multiple_buffered_fills_in_arrival_order(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Multiple partial fills buffered during the cancel-replace window must be re-
    dispatched in arrival order so the engine observes the correct cumulative fill
    sequence on the replacement leg (GH-3972).
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-FILL-RACE-MULTI"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("MULTI-OLD")
    new_voi = VenueOrderId("MULTI-NEW")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)
    client._pending_modify_keys[order.client_order_id.value] = old_voi.value

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill_a = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        "T-MULTI-A",
        "0.00010",
        "53800.0",
    )
    fill_b = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        "T-MULTI-B",
        "0.00010",
        "53850.0",
    )
    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="53850.0",
        quantity="0.00020",
    )

    try:
        # Act
        client._handle_fill_report_pyo3(fill_a)
        client._handle_fill_report_pyo3(fill_b)
        assert len(client._buffered_fills[order.client_order_id.value]) == 2
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert
        updated_events = [e for e in captured if isinstance(e, OrderUpdated)]
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(updated_events) == 1
        assert len(filled_events) == 2

        # Arrival order: A (53800.0) before B (53850.0). A reversed drain or
        # single-element overwrite mutation would change this sequence.
        assert filled_events[0].trade_id.value == "T-MULTI-A"
        assert filled_events[0].last_px == Price.from_str("53800.0")
        assert filled_events[1].trade_id.value == "T-MULTI-B"
        assert filled_events[1].last_px == Price.from_str("53850.0")

        # OrderUpdated must precede both Filled events.
        update_index = next(i for i, e in enumerate(captured) if isinstance(e, OrderUpdated))
        first_fill_index = next(i for i, e in enumerate(captured) if isinstance(e, OrderFilled))
        assert update_index < first_fill_index

        assert order.client_order_id.value not in client._buffered_fills
        assert "T-MULTI-A" in client._processed_trade_ids
        assert "T-MULTI-B" in client._processed_trade_ids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_filled_marker_then_buffered_fill_drain_runs_tail_cleanup(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    GH-3972: when a FILLED status marker for the replacement leg arrived first,
    the buffer guard returns before the tail cleanup, but the eventual
    cancel-replace ACCEPTED drain must re-dispatch the fill so the tail cleanup
    fires and `_pending_filled` / the cloid mapping are evicted.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-FILL-RACE-TAIL"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    cache.add_order(order, None)

    old_voi = VenueOrderId("TAIL-OLD")
    new_voi = VenueOrderId("TAIL-NEW")
    cache.add_venue_order_id(order.client_order_id, old_voi)
    client._accepted_orders.add(order.client_order_id.value)
    client._pending_modify_keys[order.client_order_id.value] = old_voi.value
    # Simulate an earlier FILLED status marker for the replacement leg that
    # deferred the cloid cleanup to the matching FillReport.
    client._pending_filled.add(order.client_order_id.value)

    cleanup_calls: list = []
    original_cleanup = client._cleanup_cloid_mapping
    monkeypatch.setattr(
        client,
        "_cleanup_cloid_mapping",
        lambda cid: cleanup_calls.append(cid) or original_cleanup(cid),
    )

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        "T-TAIL-1",
        "0.00020",
        "53893.0",
    )
    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        new_voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="53893.0",
        quantity="0.00020",
    )

    try:
        # Act 1: fill arrives first; buffer guard fires.
        client._handle_fill_report_pyo3(fill)

        # Tail cleanup must NOT have fired yet (guard returned early).
        assert order.client_order_id.value in client._pending_filled
        assert cleanup_calls == []
        assert client._buffered_fills[order.client_order_id.value] == [fill]

        # Act 2: cancel-replace ACCEPTED arrives, drains the buffer.
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert: drained fill ran the full pipeline, including tail cleanup.
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].venue_order_id == new_voi
        assert order.client_order_id.value not in client._pending_filled
        assert order.client_order_id.value not in client._buffered_fills
        # `_cleanup_cloid_mapping` is called once by the drained fill's tail
        # (terminal cleanup may also pop the buffered_fills entry, but that is
        # already empty by then).
        assert cleanup_calls == [order.client_order_id]
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_buffered_fills_cleared_on_terminal_cleanup(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Terminal cleanup must drop any buffered fills so a stranded entry cannot outlive the
    cloid mapping it was keyed on (GH-3972).
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    cid = ClientOrderId("O-FILL-RACE-004")
    fill = _build_fill_report_pyo3(
        client,
        instrument,
        cid,
        "9300",
        "T-RACE-4",
        "0.00020",
        "56730.0",
    )
    client._buffered_fills[cid.value] = [fill]

    try:
        # Act
        client._cleanup_cloid_mapping(cid)

        # Assert
        assert cid.value not in client._buffered_fills
    finally:
        await client._disconnect()


def _build_order_accepted_pyo3(client, instrument, client_order_id, venue_order_id):
    return nautilus_pyo3.OrderAccepted(
        trader_id=nautilus_pyo3.TraderId(TestIdStubs.trader_id().value),
        strategy_id=nautilus_pyo3.StrategyId(TestIdStubs.strategy_id().value),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        client_order_id=nautilus_pyo3.ClientOrderId(client_order_id.value),
        venue_order_id=nautilus_pyo3.VenueOrderId(venue_order_id),
        account_id=nautilus_pyo3.AccountId(client.account_id.value),
        event_id=nautilus_pyo3.UUID4(),
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )


@pytest.mark.asyncio
async def test_handle_fill_report_buffers_when_order_not_in_cache(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    A FillReport arriving before the order has been added to the local cache must be
    buffered in `_pending_fills` rather than dropped, so it can be replayed once the
    order becomes known.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    cid = ClientOrderId("O-PENDING-001")
    # Force the non-external path while the cache has no order, exercising the race.
    monkeypatch.setattr(client, "_is_external_order", lambda _: False)

    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill = _build_fill_report_pyo3(
        client,
        instrument,
        cid,
        "9400",
        "T-PENDING-1",
        "0.00020",
        "56730.0",
    )

    try:
        # Act
        client._handle_fill_report_pyo3(fill)

        # Assert
        assert captured == []
        assert client._pending_fills[cid.value] == [fill]
        assert "T-PENDING-1" not in client._processed_trade_ids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_order_accepted_drains_pending_fill(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    The dedicated OrderAccepted WS event must drain any FillReport that was buffered
    while the order was not yet in cache, producing OrderFilled in order.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PENDING-002"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    voi = VenueOrderId("9410")
    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        "T-PENDING-2",
        "0.00020",
        "56730.0",
    )
    accepted_msg = _build_order_accepted_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
    )

    try:
        monkeypatch.setattr(client, "_is_external_order", lambda _: False)

        # Act 1: fill arrives before order known, buffered.
        client._handle_fill_report_pyo3(fill)
        assert client._pending_fills[order.client_order_id.value] == [fill]

        # Act 2: order is now added to cache; OrderAccepted drains the buffer.
        cache.add_order(order, None)
        client._handle_order_accepted_pyo3(accepted_msg)

        # Assert
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].venue_order_id == voi
        assert filled_events[0].trade_id.value == "T-PENDING-2"
        assert order.client_order_id.value not in client._pending_fills
        assert "T-PENDING-2" in client._processed_trade_ids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_order_status_accepted_drains_pending_fill(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    The ACCEPTED branch of OrderStatusReport must drain any FillReport buffered while
    the order was not yet in cache.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PENDING-003"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00020"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    voi = VenueOrderId("9420")
    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        "T-PENDING-3",
        "0.00020",
        "56730.0",
    )
    accepted_report = _build_status_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        nautilus_pyo3.OrderStatus.ACCEPTED,
        price="56730.0",
        quantity="0.00020",
    )

    try:
        monkeypatch.setattr(client, "_is_external_order", lambda _: False)

        # Act 1: fill arrives before order known, buffered.
        client._handle_fill_report_pyo3(fill)
        assert client._pending_fills[order.client_order_id.value] == [fill]

        # Act 2: order is now in cache; OrderStatusReport(ACCEPTED) drains.
        cache.add_order(order, None)
        client._handle_order_status_report_pyo3(accepted_report)

        # Assert
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].venue_order_id == voi
        assert filled_events[0].trade_id.value == "T-PENDING-3"
        assert order.client_order_id.value not in client._pending_fills
        assert "T-PENDING-3" in client._processed_trade_ids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_inline_auto_accept_drains_pending_fill(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    When a later FillReport arrives and finds the order in cache but not yet accepted in
    the local state machine, the inline auto-accept path must drain any previously
    buffered FillReports for the same order.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PENDING-004"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00040"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    voi = VenueOrderId("9430")
    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill_a = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        "T-PENDING-4A",
        "0.00020",
        "56730.0",
    )
    fill_b = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        "T-PENDING-4B",
        "0.00020",
        "56735.0",
    )

    try:
        monkeypatch.setattr(client, "_is_external_order", lambda _: False)

        # Act 1: fill A arrives before order known, buffered.
        client._handle_fill_report_pyo3(fill_a)
        assert client._pending_fills[order.client_order_id.value] == [fill_a]

        # Act 2: order is now in cache; fill B finds order and triggers inline drain.
        cache.add_order(order, None)
        client._handle_fill_report_pyo3(fill_b)

        # Assert: both fills processed, A before B (drain runs before B's own emit).
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].trade_id.value == "T-PENDING-4A"
        assert filled_events[1].trade_id.value == "T-PENDING-4B"
        assert order.client_order_id.value not in client._pending_fills
        assert "T-PENDING-4A" in client._processed_trade_ids
        assert "T-PENDING-4B" in client._processed_trade_ids
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_multiple_pending_fills_drained_in_arrival_order(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Multiple FillReports buffered while the order is not in cache must be re-dispatched
    in arrival order so the engine observes the correct cumulative fill sequence.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-PENDING-MULTI"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00040"),
        price=Price.from_str("56730.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    voi = VenueOrderId("9440")
    captured: list = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: captured.append(event))

    fill_a = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        "T-PENDING-MA",
        "0.00010",
        "56720.0",
    )
    fill_b = _build_fill_report_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
        "T-PENDING-MB",
        "0.00010",
        "56725.0",
    )
    accepted_msg = _build_order_accepted_pyo3(
        client,
        instrument,
        order.client_order_id,
        voi.value,
    )

    try:
        monkeypatch.setattr(client, "_is_external_order", lambda _: False)

        # Act 1: two fills arrive, both buffered.
        client._handle_fill_report_pyo3(fill_a)
        client._handle_fill_report_pyo3(fill_b)
        assert len(client._pending_fills[order.client_order_id.value]) == 2

        # Act 2: order added, OrderAccepted drains both in order.
        cache.add_order(order, None)
        client._handle_order_accepted_pyo3(accepted_msg)

        # Assert
        filled_events = [e for e in captured if isinstance(e, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].trade_id.value == "T-PENDING-MA"
        assert filled_events[0].last_px == Price.from_str("56720.0")
        assert filled_events[1].trade_id.value == "T-PENDING-MB"
        assert filled_events[1].last_px == Price.from_str("56725.0")
        assert order.client_order_id.value not in client._pending_fills
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_pending_fills_cleared_on_terminal_cleanup(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
):
    """
    Terminal cleanup must drop any pending fills so a stranded entry cannot outlive the
    cloid mapping it was keyed on.
    """
    # Arrange
    client, _, _, _ = exec_client_builder(monkeypatch)
    await client._connect()

    cid = ClientOrderId("O-PENDING-005")
    fill = _build_fill_report_pyo3(
        client,
        instrument,
        cid,
        "9450",
        "T-PENDING-5",
        "0.00020",
        "56730.0",
    )
    client._pending_fills[cid.value] = [fill]

    try:
        # Act
        client._cleanup_cloid_mapping(cid)

        # Assert
        assert cid.value not in client._pending_fills
    finally:
        await client._disconnect()


@pytest.mark.parametrize(
    ("exc", "expected"),
    [
        (TimeoutError("connect"), True),
        (OSError("connection reset"), True),
        (ValueError("transport error: HTTP client error: refused"), True),
        (ValueError("IO error: broken pipe"), True),
        (ValueError("timeout"), True),
        (ValueError("bad request: invalid payload"), False),
        (ValueError("exchange error: insufficient margin"), False),
        (ValueError("auth error: invalid signature"), False),
        (Exception("Order already filled"), False),
    ],
)
def test_is_transport_error_classifier(exc, expected):
    assert _is_transport_error(exc) is expected


def _make_limit_order(instrument, coid: str = "O-TXP-001") -> LimitOrder:
    return LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId(coid),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.00100"),
        price=Price.from_str("50000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )


def _accept_order(order: LimitOrder, voi: str) -> None:
    order.apply(TestEventStubs.order_submitted(order=order))
    order.apply(
        TestEventStubs.order_accepted(order=order, venue_order_id=VenueOrderId(voi)),
    )


_TRANSPORT_EXC_CASES = [
    pytest.param(TimeoutError("connect timeout"), id="native-timeout"),
    pytest.param(
        ValueError("transport error: HTTP client error: connection refused"),
        id="pyo3-transport",
    ),
]


@pytest.mark.asyncio
@pytest.mark.parametrize("exc", _TRANSPORT_EXC_CASES)
async def test_submit_order_transport_failure_does_not_reject(
    exec_client_builder,
    monkeypatch,
    instrument,
    exc,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    client.generate_order_rejected = MagicMock()
    http_client.submit_order.side_effect = exc

    order = _make_limit_order(instrument, coid="O-TXP-SUBMIT")
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

        http_client.submit_order.assert_awaited_once()
        client.generate_order_rejected.assert_not_called()
        assert order.client_order_id.value not in client._terminal_orders
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize("exc", _TRANSPORT_EXC_CASES)
async def test_submit_order_list_transport_failure_does_not_reject(
    exec_client_builder,
    monkeypatch,
    instrument,
    exc,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    client.generate_order_rejected = MagicMock()
    http_client.submit_orders.side_effect = exc

    order = _make_limit_order(instrument, coid="O-TXP-LIST")
    order_list = OrderList(order_list_id=OrderListId("OL-TXP"), orders=[order])
    command = SubmitOrderList(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order_list=order_list,
        position_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._submit_order_list(command)

        http_client.submit_orders.assert_awaited_once()
        client.generate_order_rejected.assert_not_called()
        assert order.client_order_id.value not in client._terminal_orders
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize("exc", _TRANSPORT_EXC_CASES)
async def test_cancel_order_transport_failure_does_not_reject(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
    exc,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    client.generate_order_cancel_rejected = MagicMock()
    http_client.cancel_order.side_effect = exc

    order = _make_limit_order(instrument, coid="O-TXP-CANCEL")
    _accept_order(order, voi="9001")
    cache.add_order(order, None)

    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._cancel_order(command)

        http_client.cancel_order.assert_awaited_once()
        client.generate_order_cancel_rejected.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize("exc", _TRANSPORT_EXC_CASES)
async def test_cancel_all_orders_transport_failure_does_not_reject(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
    exc,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    client.generate_order_cancel_rejected = MagicMock()
    http_client.cancel_order.side_effect = exc

    order_a = _make_limit_order(instrument, coid="O-TXP-ALL-A")
    order_b = _make_limit_order(instrument, coid="O-TXP-ALL-B")
    _accept_order(order_a, voi="9101")
    _accept_order(order_b, voi="9102")
    cache.add_order(order_a, None)
    cache.add_order(order_b, None)
    cache.update_order(order_a)
    cache.update_order(order_b)

    command = CancelAllOrders(
        trader_id=order_a.trader_id,
        strategy_id=order_a.strategy_id,
        instrument_id=instrument.id,
        order_side=OrderSide.NO_ORDER_SIDE,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._cancel_all_orders(command)

        assert http_client.cancel_order.await_count == 2
        client.generate_order_cancel_rejected.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize("exc", _TRANSPORT_EXC_CASES)
async def test_batch_cancel_orders_transport_failure_does_not_reject(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
    exc,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    client.generate_order_cancel_rejected = MagicMock()
    http_client.cancel_order.side_effect = exc

    order_a = _make_limit_order(instrument, coid="O-TXP-BATCH-A")
    order_b = _make_limit_order(instrument, coid="O-TXP-BATCH-B")
    _accept_order(order_a, voi="9201")
    _accept_order(order_b, voi="9202")
    cache.add_order(order_a, None)
    cache.add_order(order_b, None)

    cancels = [
        CancelOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
            client_id=None,
        )
        for order in (order_a, order_b)
    ]
    command = BatchCancelOrders(
        trader_id=order_a.trader_id,
        strategy_id=order_a.strategy_id,
        instrument_id=instrument.id,
        cancels=cancels,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        client_id=None,
    )

    try:
        await client._batch_cancel_orders(command)

        assert http_client.cancel_order.await_count == 2
        client.generate_order_cancel_rejected.assert_not_called()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize("exc", _TRANSPORT_EXC_CASES)
async def test_modify_order_transport_failure_preserves_pending_state(
    exec_client_builder,
    monkeypatch,
    instrument,
    cache,
    exc,
):
    client, _, http_client, _ = exec_client_builder(monkeypatch)
    await client._connect()

    client.generate_order_modify_rejected = MagicMock()
    http_client.modify_order.side_effect = exc

    order = _make_limit_order(instrument, coid="O-TXP-MODIFY")
    _accept_order(order, voi="9301")
    cache.add_order(order, None)

    target_qty = Quantity.from_str("0.00200")
    command = ModifyOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
        quantity=target_qty,
        price=Price.from_str("51000.0"),
        trigger_price=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    try:
        await client._modify_order(command)

        http_client.modify_order.assert_awaited_once()
        client.generate_order_modify_rejected.assert_not_called()
        assert (
            client._pending_modify_keys[order.client_order_id.value] == order.venue_order_id.value
        )
        assert client._pending_modify_target_qty[order.client_order_id.value] == target_qty
    finally:
        await client._disconnect()
