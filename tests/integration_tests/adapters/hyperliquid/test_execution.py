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
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import OrderList
from nautilus_trader.model.orders import StopMarketOrder
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
            testnet=False,
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

        # Assert - Order rejection is handled internally
        http_client.submit_order.assert_awaited_once()
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

        # Assert - Rejection is handled internally
        http_client.cancel_order.assert_awaited_once()
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

        # Assert - rejection handled internally
        http_client.modify_order.assert_awaited_once()
    finally:
        await client._disconnect()
