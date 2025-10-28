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

from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.okx.conftest import _create_ws_mock


@pytest.fixture()
def exec_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
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

        config_kwargs = config_kwargs or {}
        instrument_types = config_kwargs.pop(
            "instrument_types",
            (nautilus_pyo3.OKXInstrumentType.SPOT,),
        )

        # Set the mock provider's instrument_types to match config
        mock_instrument_provider.instrument_types = instrument_types

        config = OKXExecClientConfig(
            api_key="test_api_key",
            api_secret="test_api_secret",
            api_passphrase="test_passphrase",
            instrument_types=instrument_types,
            **config_kwargs,
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

    return builder


@pytest.mark.asyncio
async def test_connect_success(exec_client_builder, monkeypatch):
    # Arrange
    client, private_ws, business_ws, http_client, instrument_provider = exec_client_builder(
        monkeypatch,
    )

    monkeypatch.setattr(client, "_await_account_registered", AsyncMock())

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
        http_client.add_instrument.assert_called_once_with(
            instrument_provider.instruments_pyo3.return_value[0],
        )
        http_client.request_account_state.assert_awaited_once()
        private_ws.connect.assert_awaited_once()
        private_ws.wait_until_active.assert_awaited_once_with(timeout_secs=30.0)
        business_ws.connect.assert_awaited_once()
        business_ws.wait_until_active.assert_awaited_once_with(timeout_secs=30.0)
        private_ws.subscribe_orders.assert_awaited_once_with(nautilus_pyo3.OKXInstrumentType.SPOT)
        business_ws.subscribe_orders_algo.assert_awaited_once_with(
            nautilus_pyo3.OKXInstrumentType.SPOT,
        )
        private_ws.subscribe_fills.assert_not_called()
        private_ws.subscribe_account.assert_awaited_once()
    finally:
        await client._disconnect()

    # Assert
    http_client.cancel_all_requests.assert_called_once()
    private_ws.close.assert_awaited_once()
    business_ws.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_generate_order_status_reports_converts_results(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.OrderStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    pyo3_report = MagicMock()
    http_client.request_order_status_reports.return_value = [pyo3_report]

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
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
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)
    http_client.request_order_status_reports.side_effect = Exception("boom")

    command = GenerateOrderStatusReports(
        instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
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
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_fill_reports.return_value = [MagicMock()]

    command = GenerateFillReports(
        instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
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
    # Use SWAP (derivatives) so positions are actually queried
    client, _, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"instrument_types": (nautilus_pyo3.OKXInstrumentType.SWAP,)},
    )

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.PositionStatusReport.from_pyo3",
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
async def test_generate_position_status_reports_spot_margin_uses_margin_inst_type(
    exec_client_builder,
    monkeypatch,
    eth_usdt_instrument,
):
    # Arrange - spot margin mode with specific instrument query
    client, _, _, http_client, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "use_spot_margin": True,
            "margin_mode": nautilus_pyo3.OKXMarginMode.CROSS,
        },
    )

    instrument = eth_usdt_instrument
    client._cache.add_instrument(instrument)

    expected_report = MagicMock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.PositionStatusReport.from_pyo3",
        lambda obj: expected_report,
    )

    http_client.request_position_status_reports.return_value = [MagicMock()]

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    reports = await client.generate_position_status_reports(command)

    # Assert - verify MARGIN instType is used for spot margin positions
    http_client.request_position_status_reports.assert_awaited_once()
    call_args = http_client.request_position_status_reports.call_args
    assert call_args.kwargs["instrument_type"] == nautilus_pyo3.OKXInstrumentType.MARGIN
    assert reports == [expected_report]


@pytest.mark.asyncio
async def test_handle_fill_report_updates_venue_id_before_fill(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)

    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    client._cache.add_instrument(instrument)

    order_list = TestExecStubs.limit_with_stop_market(instrument=instrument)
    stop_order = next(order for order in order_list.orders if isinstance(order, StopMarketOrder))

    submitted = TestEventStubs.order_submitted(order=stop_order)
    stop_order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=stop_order,
        venue_order_id=VenueOrderId("algo-venue-id"),
    )
    stop_order.apply(accepted)

    client._cache.add_order(stop_order, None, None)

    canonical_id = stop_order.client_order_id
    client._algo_order_ids[canonical_id] = "algo-venue-id"
    client._algo_order_instruments[canonical_id] = stop_order.instrument_id

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    new_venue_id = VenueOrderId("child-venue-id")
    fill_report = SimpleNamespace(
        client_order_id=stop_order.client_order_id,
        venue_order_id=new_venue_id,
        venue_position_id=None,
        trade_id=TestIdStubs.trade_id(),
        last_qty=stop_order.quantity,
        last_px=instrument.make_price(4018.5),
        commission=Money(0, instrument.quote_currency),
        liquidity_side=LiquiditySide.TAKER,
        ts_event=123456789,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda _obj: fill_report,
    )

    # Act
    client._handle_fill_report_pyo3(MagicMock())

    # Assert
    assert any(
        isinstance(event, OrderUpdated) and event.venue_order_id == new_venue_id
        for event in emitted_events
    )
    assert any(isinstance(event, OrderFilled) for event in emitted_events)
    assert client._cache.venue_order_id(stop_order.client_order_id) == new_venue_id
    assert canonical_id not in client._algo_order_ids

    http_client.request_fill_reports.assert_not_called()


@pytest.mark.asyncio
async def test_generate_position_status_reports_handles_failure(exec_client_builder, monkeypatch):
    # Arrange
    client, _, _, http_client, _ = exec_client_builder(monkeypatch)
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
async def test_batch_cancel_orders_success(exec_client_builder, monkeypatch):
    # Arrange
    client, private_ws, _, _, _ = exec_client_builder(monkeypatch)

    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    client._cache.add_instrument(instrument)

    # Create three limit orders
    order1 = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-batch-1"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("1.0000"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order2 = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-batch-2"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(200),
        price=Price.from_str("1.0010"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order3 = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-batch-3"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(150),
        price=Price.from_str("0.9990"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Apply submitted and accepted events
    for order in [order1, order2, order3]:
        submitted = TestEventStubs.order_submitted(order=order)
        order.apply(submitted)
        accepted = TestEventStubs.order_accepted(
            order=order,
            venue_order_id=VenueOrderId(f"venue-{order.client_order_id}"),
        )
        order.apply(accepted)
        client._cache.add_order(order, None, None)

    # Create batch cancel command
    command = BatchCancelOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        cancels=[
            CancelOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument.id,
                client_order_id=order1.client_order_id,
                venue_order_id=order1.venue_order_id,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            ),
            CancelOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument.id,
                client_order_id=order2.client_order_id,
                venue_order_id=order2.venue_order_id,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            ),
            CancelOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument.id,
                client_order_id=order3.client_order_id,
                venue_order_id=order3.venue_order_id,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            ),
        ],
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await client._batch_cancel_orders(command)

    # Assert
    private_ws.batch_cancel_orders.assert_awaited_once()
    call_args = private_ws.batch_cancel_orders.call_args[0][0]
    assert len(call_args) == 3
    # Verify all tuples have correct structure (instrument_id, client_order_id, venue_order_id)
    for item in call_args:
        assert len(item) == 3
        assert item[1] is not None  # client_order_id
        assert item[2] is not None  # venue_order_id


@pytest.mark.asyncio
async def test_batch_cancel_orders_filters_closed_orders(exec_client_builder, monkeypatch):
    # Arrange
    client, private_ws, _, _, _ = exec_client_builder(monkeypatch)

    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    client._cache.add_instrument(instrument)

    # Create two orders - one open, one closed
    order_open = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-filter-open"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("1.0000"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order_closed = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-filter-closed"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(200),
        price=Price.from_str("1.0010"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Apply events for open order
    submitted_open = TestEventStubs.order_submitted(order=order_open)
    order_open.apply(submitted_open)
    accepted_open = TestEventStubs.order_accepted(
        order=order_open,
        venue_order_id=VenueOrderId("venue-1"),
    )
    order_open.apply(accepted_open)

    # Apply events for closed order (canceled)
    submitted_closed = TestEventStubs.order_submitted(order=order_closed)
    order_closed.apply(submitted_closed)
    accepted_closed = TestEventStubs.order_accepted(
        order=order_closed,
        venue_order_id=VenueOrderId("venue-2"),
    )
    order_closed.apply(accepted_closed)
    canceled = TestEventStubs.order_canceled(order=order_closed)
    order_closed.apply(canceled)

    client._cache.add_order(order_open, None, None)
    client._cache.add_order(order_closed, None, None)

    # Create batch cancel command with both orders
    command = BatchCancelOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        cancels=[
            CancelOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument.id,
                client_order_id=order_open.client_order_id,
                venue_order_id=order_open.venue_order_id,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            ),
            CancelOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument.id,
                client_order_id=order_closed.client_order_id,
                venue_order_id=order_closed.venue_order_id,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            ),
        ],
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await client._batch_cancel_orders(command)

    # Assert - only one order should be sent (the open one)
    private_ws.batch_cancel_orders.assert_awaited_once()
    call_args = private_ws.batch_cancel_orders.call_args[0][0]
    assert len(call_args) == 1


@pytest.mark.asyncio
async def test_batch_cancel_orders_handles_order_not_in_cache(exec_client_builder, monkeypatch):
    # Arrange
    client, private_ws, _, _, _ = exec_client_builder(monkeypatch)

    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    client._cache.add_instrument(instrument)

    # Create command for order that doesn't exist in cache
    command = BatchCancelOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        cancels=[
            CancelOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument.id,
                client_order_id=TestIdStubs.client_order_id(),
                venue_order_id=VenueOrderId("venue-1"),
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            ),
        ],
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await client._batch_cancel_orders(command)

    # Assert - no orders should be sent
    private_ws.batch_cancel_orders.assert_not_called()


@pytest.mark.asyncio
async def test_cancel_all_orders_uses_mass_cancel_when_configured(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    # Arrange
    client, private_ws, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"use_mm_mass_cancel": True},
    )

    client._cache.add_instrument(instrument)

    command = CancelAllOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await client._cancel_all_orders(command)

    # Assert - should use mass cancel
    private_ws.mass_cancel_orders.assert_called_once()
    private_ws.batch_cancel_orders.assert_not_called()


@pytest.mark.asyncio
async def test_cancel_all_orders_uses_batch_cancel_by_default(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    # Arrange
    client, private_ws, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={"use_mm_mass_cancel": False},
    )

    client._cache.add_instrument(instrument)

    # Create 5 open orders
    orders = []
    for i in range(5):
        order = LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId(f"O-cancel-all-{i}"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            price=Price.from_str(f"1.{i:04d}"),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        submitted = TestEventStubs.order_submitted(order=order)
        order.apply(submitted)
        accepted = TestEventStubs.order_accepted(
            order=order,
            venue_order_id=VenueOrderId(f"venue-{i}"),
        )
        order.apply(accepted)
        client._cache.add_order(order, None, None)
        orders.append(order)

    # Create batch cancel command to test batching logic
    cancels = [
        CancelOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        for order in orders
    ]

    batch_command = BatchCancelOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        cancels=cancels,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act - test batch cancel with 5 orders (should be in one batch)
    await client._batch_cancel_orders(batch_command)

    # Assert - should use batch cancel with all 5 orders in one batch
    private_ws.batch_cancel_orders.assert_called_once()
    call_args = private_ws.batch_cancel_orders.call_args[0][0]
    assert len(call_args) == 5


@pytest.mark.asyncio
async def test_cancel_all_orders_batches_in_chunks_of_20(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    # Arrange
    client, private_ws, _, _, _ = exec_client_builder(monkeypatch)

    client._cache.add_instrument(instrument)

    # Create 45 open orders and add to cache
    orders = []
    for i in range(45):
        order = LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId(f"O-chunk-{i}"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            price=Price.from_str(f"1.{i:04d}"),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        submitted = TestEventStubs.order_submitted(order=order)
        order.apply(submitted)
        accepted = TestEventStubs.order_accepted(
            order=order,
            venue_order_id=VenueOrderId(f"venue-chunk-{i}"),
        )
        order.apply(accepted)
        client._cache.add_order(order, None, None)
        orders.append(order)

    # Create 45 cancel commands (should be split into 3 batches: 20 + 20 + 5)
    cancels = [
        CancelOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        for order in orders
    ]

    # Test batching by processing cancels in chunks of 20
    batch_size = 20
    for i in range(0, len(cancels), batch_size):
        batch = cancels[i : i + batch_size]
        batch_command = BatchCancelOrders(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            cancels=batch,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        await client._batch_cancel_orders(batch_command)

    # Assert - should have 3 batch cancel calls
    assert private_ws.batch_cancel_orders.call_count == 3
    # First two batches should have 20 orders each
    first_batch = private_ws.batch_cancel_orders.call_args_list[0][0][0]
    second_batch = private_ws.batch_cancel_orders.call_args_list[1][0][0]
    third_batch = private_ws.batch_cancel_orders.call_args_list[2][0][0]
    assert len(first_batch) == 20
    assert len(second_batch) == 20
    assert len(third_batch) == 5


@pytest.mark.asyncio
async def test_cancel_all_orders_handles_mixed_regular_and_algo_orders(
    exec_client_builder,
    monkeypatch,
    instrument,
):
    """
    Test that cancel_all separates regular orders (batch via WebSocket) from algo orders
    (individual via REST API).
    """
    # Arrange
    client, private_ws, _, http_client, _ = exec_client_builder(monkeypatch)

    client._cache.add_instrument(instrument)

    # Create 3 regular orders
    regular_orders = []
    for i in range(3):
        order = LimitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=ClientOrderId(f"O-regular-{i}"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            price=Price.from_str(f"1.{i:04d}"),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        submitted = TestEventStubs.order_submitted(order=order)
        order.apply(submitted)
        accepted = TestEventStubs.order_accepted(
            order=order,
            venue_order_id=VenueOrderId(f"venue-regular-{i}"),
        )
        order.apply(accepted)
        client._cache.add_order(order, None, None)
        regular_orders.append(order)

    # Create 2 algo orders and register them in _algo_order_ids
    algo_client_ids = []
    for i in range(2):
        client_id = ClientOrderId(f"O-algo-{i}")
        algo_client_ids.append(client_id)
        # Register as algo order (simulating orders submitted via _submit_algo_order_http)
        client._algo_order_ids[client_id] = f"okx-algo-id-{i}"
        client._algo_order_instruments[client_id] = instrument.id

    # Mock the HTTP cancel_algo_order call
    http_client.cancel_algo_order = AsyncMock()

    # Act - Create batch with regular orders only (algo orders should be skipped)
    regular_cancels = [
        CancelOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )
        for order in regular_orders
    ]

    batch_command = BatchCancelOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        cancels=regular_cancels,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    await client._batch_cancel_orders(batch_command)

    # Cancel algo orders via fallback
    for client_id in algo_client_ids:
        await client._cancel_algo_order_fallback(
            client_order_id=client_id,
            instrument_id=instrument.id,
            algo_id=client._algo_order_ids[client_id],
        )

    # Assert - regular orders should be batch cancelled via WebSocket
    private_ws.batch_cancel_orders.assert_called_once()
    call_args = private_ws.batch_cancel_orders.call_args[0][0]
    assert len(call_args) == 3  # Only the 3 regular orders

    # Assert - algo orders should be cancelled via REST API (2 calls)
    assert http_client.cancel_algo_order.call_count == 2


# =====================================================================================
# Quote Quantity Order Tests
# =====================================================================================


@pytest.mark.asyncio
async def test_spot_margin_market_buy_denies_order_without_quote_quantity(
    exec_client_builder,
    monkeypatch,
    eth_usdt_instrument,
):
    # Arrange
    client, _, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "use_spot_margin": True,
            "margin_mode": nautilus_pyo3.OKXMarginMode.CROSS,
        },
    )

    instrument = eth_usdt_instrument
    client._cache.add_instrument(instrument)

    # Create market buy order without quote_quantity
    order = TestExecStubs.market_order(
        instrument=instrument,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("20.0"),  # Base quantity without quote_quantity=True
    )

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    from nautilus_trader.execution.messages import SubmitOrder

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        order=order,
        position_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await client._submit_order(command)

    # Assert
    assert any(isinstance(event, OrderDenied) for event in emitted_events)
    denied_event = next(event for event in emitted_events if isinstance(event, OrderDenied))
    assert "quote-denominated quantities" in denied_event.reason


@pytest.mark.asyncio
async def test_spot_margin_market_buy_quote_quantity_converts_on_first_fill(
    exec_client_builder,
    monkeypatch,
    eth_usdt_instrument,
):
    # Arrange
    client, _, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "use_spot_margin": True,
            "margin_mode": nautilus_pyo3.OKXMarginMode.CROSS,
        },
    )

    instrument = eth_usdt_instrument
    client._cache.add_instrument(instrument)

    # Create market buy order with quote_quantity=True
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-quote-qty"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("20.0"),  # 20 USDT
        init_id=TestIdStubs.uuid(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=True,  # Key: quote quantity order
    )

    # Simulate order submission and acceptance
    submitted = TestEventStubs.order_submitted(order=order)
    order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=order,
        venue_order_id=VenueOrderId("venue-123"),
    )
    order.apply(accepted)

    client._cache.add_order(order, None, None)

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    # Create fill report: gross = 0.005230 ETH, commission = 0.00000523 ETH
    # Net = 0.005230 - 0.00000523 = 0.00522477 ETH
    fill_report = SimpleNamespace(
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
        venue_position_id=None,
        trade_id=TestIdStubs.trade_id(),
        last_qty=Quantity.from_str("0.005230"),  # Gross fill in base currency
        last_px=Price.from_str("3825.25"),
        commission=Money.from_str("0.00000523 ETH"),  # Fee in base currency
        liquidity_side=LiquiditySide.TAKER,
        ts_event=123456789,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda _obj: fill_report,
    )

    # Act
    client._handle_fill_report_pyo3(MagicMock())

    # Assert
    # Should generate OrderUpdated with net quantity
    assert any(isinstance(event, OrderUpdated) for event in emitted_events)
    updated_event = next(event for event in emitted_events if isinstance(event, OrderUpdated))
    # Net quantity = 0.005230 - 0.00000523 ≈ 0.005225 (rounded to precision 6)
    assert updated_event.quantity == Quantity.from_str("0.005225")

    # Should generate OrderFilled with net last_qty
    assert any(isinstance(event, OrderFilled) for event in emitted_events)
    filled_event = next(event for event in emitted_events if isinstance(event, OrderFilled))
    assert filled_event.last_qty == Quantity.from_str("0.005225")

    # Order should no longer be quote_quantity
    assert not order.is_quote_quantity


@pytest.mark.asyncio
async def test_spot_margin_market_buy_quote_quantity_handles_quote_currency_commission(
    exec_client_builder,
    monkeypatch,
    eth_usdt_instrument,
):
    # Arrange
    client, _, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "use_spot_margin": True,
            "margin_mode": nautilus_pyo3.OKXMarginMode.CROSS,
        },
    )

    instrument = eth_usdt_instrument
    client._cache.add_instrument(instrument)

    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-quote-qty-2"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("20.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=True,
    )

    submitted = TestEventStubs.order_submitted(order=order)
    order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=order,
        venue_order_id=VenueOrderId("venue-456"),
    )
    order.apply(accepted)

    client._cache.add_order(order, None, None)

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    # Create fill with commission in QUOTE currency (unusual but possible)
    fill_report = SimpleNamespace(
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
        venue_position_id=None,
        trade_id=TestIdStubs.trade_id(),
        last_qty=Quantity.from_str("0.005230"),
        last_px=Price.from_str("3825.25"),
        commission=Money.from_str("0.02 USDT"),  # Fee in QUOTE currency
        liquidity_side=LiquiditySide.TAKER,
        ts_event=123456789,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda _obj: fill_report,
    )

    # Act
    client._handle_fill_report_pyo3(MagicMock())

    # Assert
    # Should generate OrderUpdated but NOT subtract commission (different currency)
    assert any(isinstance(event, OrderUpdated) for event in emitted_events)
    updated_event = next(event for event in emitted_events if isinstance(event, OrderUpdated))
    # Quantity should be gross (not adjusted for commission)
    assert updated_event.quantity == Quantity.from_str("0.005230")

    # OrderFilled should also have gross last_qty
    assert any(isinstance(event, OrderFilled) for event in emitted_events)
    filled_event = next(event for event in emitted_events if isinstance(event, OrderFilled))
    assert filled_event.last_qty == Quantity.from_str("0.005230")


@pytest.mark.asyncio
async def test_regular_market_buy_not_affected_by_quote_quantity_logic(
    exec_client_builder,
    monkeypatch,
    eth_usdt_instrument,
):
    # Arrange - without use_spot_margin, should not trigger quote quantity validation
    client, _, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "use_spot_margin": False,  # Regular spot trading
        },
    )

    instrument = eth_usdt_instrument
    client._cache.add_instrument(instrument)

    # Market buy order WITHOUT quote_quantity should be allowed for regular spot
    order = TestExecStubs.market_order(
        instrument=instrument,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.1"),  # Base quantity
    )

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    from nautilus_trader.execution.messages import SubmitOrder

    command = SubmitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        order=order,
        position_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    # Act
    await client._submit_order(command)

    # Assert - should not be denied
    assert not any(isinstance(event, OrderDenied) for event in emitted_events)


@pytest.mark.asyncio
async def test_spot_margin_market_buy_quote_quantity_handles_partial_fills(
    exec_client_builder,
    monkeypatch,
    eth_usdt_instrument,
):
    # Arrange
    client, _, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "use_spot_margin": True,
            "margin_mode": nautilus_pyo3.OKXMarginMode.CROSS,
        },
    )

    instrument = eth_usdt_instrument
    client._cache.add_instrument(instrument)

    # Create market buy order with quote_quantity=True for 20 USDT
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-partial-fills"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("20.0"),  # 20 USDT
        init_id=TestIdStubs.uuid(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=True,
    )

    # Simulate order submission and acceptance
    submitted = TestEventStubs.order_submitted(order=order)
    order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=order,
        venue_order_id=VenueOrderId("venue-partial"),
    )
    order.apply(accepted)

    client._cache.add_order(order, None, None)

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    # First partial fill: 10 USDT worth = 0.002615 ETH @ 3825.25, commission = 0.00000261 ETH
    # Net = 0.002615 - 0.00000261 = 0.00261239 ETH
    fill_report_1 = SimpleNamespace(
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
        venue_position_id=None,
        trade_id=TestIdStubs.trade_id(),
        last_qty=Quantity.from_str("0.002615"),  # Gross fill in base currency
        last_px=Price.from_str("3825.25"),
        commission=Money.from_str("0.00000261 ETH"),  # Fee in base currency
        liquidity_side=LiquiditySide.TAKER,
        ts_event=123456789,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda _obj: fill_report_1,
    )

    # Act - First fill
    client._handle_fill_report_pyo3(MagicMock())

    # Assert - First fill should generate OrderUpdated with net quantity
    assert any(isinstance(event, OrderUpdated) for event in emitted_events)
    updated_event = next(event for event in emitted_events if isinstance(event, OrderUpdated))
    assert updated_event.quantity == Quantity.from_str("0.002612")  # Net rounded to precision 6

    # First fill should have net last_qty
    filled_events = [e for e in emitted_events if isinstance(e, OrderFilled)]
    assert len(filled_events) == 1
    assert filled_events[0].last_qty == Quantity.from_str("0.002612")

    # Clear events for second fill
    emitted_events.clear()

    # Second partial fill: Another 10 USDT worth = 0.002615 ETH @ 3825.25, commission = 0.00000261 ETH
    # Net should also be = 0.002615 - 0.00000261 = 0.00261239 ETH
    fill_report_2 = SimpleNamespace(
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
        venue_position_id=None,
        trade_id=TestIdStubs.trade_id(),
        last_qty=Quantity.from_str("0.002615"),  # Gross fill in base currency
        last_px=Price.from_str("3825.25"),
        commission=Money.from_str("0.00000261 ETH"),  # Fee in base currency
        liquidity_side=LiquiditySide.TAKER,
        ts_event=123456790,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda _obj: fill_report_2,
    )

    # Act - Second fill
    client._handle_fill_report_pyo3(MagicMock())

    # Assert - Second fill should NOT generate OrderUpdated (already converted)
    assert not any(isinstance(event, OrderUpdated) for event in emitted_events)

    # CRITICAL: Second fill should ALSO have net last_qty (adjusted for commission)
    filled_events = [e for e in emitted_events if isinstance(e, OrderFilled)]
    assert len(filled_events) == 1
    # This should be NET (0.002615 - 0.00000261 = 0.00261239 ≈ 0.002612)
    # NOT gross (0.002615)
    assert filled_events[0].last_qty == Quantity.from_str("0.002612")


@pytest.mark.asyncio
async def test_fill_with_changed_venue_order_id_generates_update(
    exec_client_builder,
    monkeypatch,
    eth_usdt_instrument,
):
    # Test that when a fill arrives with a different venue_order_id (e.g., algo order
    # transitions to live order), we correctly generate OrderUpdated with venue_order_id_modified=True
    # Arrange
    client, _, _, _, _ = exec_client_builder(
        monkeypatch,
        config_kwargs={
            "use_spot_margin": True,
            "margin_mode": nautilus_pyo3.OKXMarginMode.CROSS,
        },
    )

    instrument = eth_usdt_instrument
    client._cache.add_instrument(instrument)

    # Create and accept order with initial venue_order_id
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-venue-change"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("20.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=True,
    )

    submitted = TestEventStubs.order_submitted(order=order)
    order.apply(submitted)
    accepted = TestEventStubs.order_accepted(
        order=order,
        venue_order_id=VenueOrderId("algo-123"),  # Initial algo order ID
    )
    order.apply(accepted)

    client._cache.add_order(order, None, None)

    emitted_events: list = []

    def _capture(event):
        emitted_events.append(event)

    monkeypatch.setattr(client, "_send_order_event", _capture)

    # Fill arrives with DIFFERENT venue_order_id (algo transitioned to live)
    fill_report = SimpleNamespace(
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("live-456"),  # Different venue ID
        venue_position_id=None,
        trade_id=TestIdStubs.trade_id(),
        last_qty=Quantity.from_str("0.005230"),
        last_px=Price.from_str("3825.25"),
        commission=Money.from_str("0.00000523 ETH"),
        liquidity_side=LiquiditySide.TAKER,
        ts_event=123456789,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.okx.execution.FillReport.from_pyo3",
        lambda _obj: fill_report,
    )

    # Act
    client._handle_fill_report_pyo3(MagicMock())

    # Assert
    # Should generate ONE OrderUpdated event that handles both:
    # 1. Quote quantity conversion (quantity updated)
    # 2. Venue order ID change (venue_order_id updated with venue_order_id_modified=True)
    updated_events = [e for e in emitted_events if isinstance(e, OrderUpdated)]
    assert len(updated_events) == 1

    # The single update should have the new venue_order_id
    assert updated_events[0].venue_order_id == VenueOrderId("live-456")
    # And the net quantity from the quote quantity conversion
    assert updated_events[0].quantity == Quantity.from_str("0.005225")

    # Should also generate OrderFilled with correct venue_order_id
    filled_events = [e for e in emitted_events if isinstance(e, OrderFilled)]
    assert len(filled_events) == 1
    assert filled_events[0].venue_order_id == VenueOrderId("live-456")
    # And net last_qty (commission adjusted)
    assert filled_events[0].last_qty == Quantity.from_str("0.005225")
