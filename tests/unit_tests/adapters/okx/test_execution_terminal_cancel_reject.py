from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _create_ws_mock() -> MagicMock:
    mock = MagicMock(spec=nautilus_pyo3.OKXWebSocketClient)
    mock.url = "wss://test.okx.com/realtime"
    mock.is_closed.return_value = False
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()
    mock.subscribe_orders = AsyncMock()
    mock.subscribe_orders_algo = AsyncMock()
    mock.subscribe_algo_advance = AsyncMock()
    mock.subscribe_fills = AsyncMock()
    mock.subscribe_account = AsyncMock()
    mock.cache_inst_id_codes = MagicMock()
    mock.batch_cancel_orders = AsyncMock()
    return mock


@pytest.fixture
def exec_client_builder(event_loop, monkeypatch):
    def builder():
        private_ws = _create_ws_mock()
        business_ws = _create_ws_mock()
        ws_iter = iter([private_ws, business_ws])

        monkeypatch.setattr(
            "nautilus_trader.adapters.okx.execution.nautilus_pyo3.OKXWebSocketClient.with_credentials",
            lambda *args, **kwargs: next(ws_iter),
        )

        http_client = MagicMock(spec=nautilus_pyo3.OKXHttpClient)
        http_client.api_key = "test_api_key"
        http_client.is_initialized.return_value = True
        http_client.cancel_all_requests = MagicMock()
        http_client.cache_instrument = MagicMock()
        http_client.request_fill_reports = AsyncMock(return_value=[])
        http_client.request_order_status_reports = AsyncMock(return_value=[])
        http_client.request_account_state = AsyncMock(return_value=MagicMock())

        instrument_provider = OKXInstrumentProvider(
            client=http_client,
            instrument_types=(OKXInstrumentType.SWAP,),
        )
        instrument_provider.initialize = AsyncMock()
        instrument_provider.instruments_pyo3 = lambda: [MagicMock(name="py_instrument")]
        instrument_provider.inst_id_codes = lambda: {}

        client = OKXExecutionClient(
            loop=event_loop,
            client=http_client,
            msgbus=MessageBus(trader_id=TraderId("TESTER-001"), clock=LiveClock()),
            cache=TestComponentStubs.cache(),
            clock=LiveClock(),
            instrument_provider=instrument_provider,
            config=OKXExecClientConfig(
                api_key="test_api_key",
                api_secret="test_api_secret",
                api_passphrase="test_passphrase",
                instrument_types=(OKXInstrumentType.SWAP,),
            ),
            name=ClientId(OKX_VENUE.value).value,
        )
        return client, private_ws, http_client

    return builder


def _make_pending_cancel_order() -> LimitOrder:
    instrument_id = InstrumentId(Symbol("ETH-USDT-SWAP"), OKX_VENUE)
    order = LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument_id,
        client_order_id=ClientOrderId("O-okx-51400-001"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100),
        price=Price.from_str("1000.0"),
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    order.apply(TestEventStubs.order_submitted(order=order))
    order.apply(
        TestEventStubs.order_accepted(
            order=order,
            venue_order_id=VenueOrderId("3366550319725617152"),
        ),
    )
    order.apply(TestEventStubs.order_pending_cancel(order=order))
    return order


def _make_terminal_gone_event(order: LimitOrder) -> nautilus_pyo3.OrderCancelRejected:
    return nautilus_pyo3.OrderCancelRejected(
        trader_id=nautilus_pyo3.TraderId(order.trader_id.value),
        strategy_id=nautilus_pyo3.StrategyId(order.strategy_id.value),
        instrument_id=nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value),
        client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
        venue_order_id=nautilus_pyo3.VenueOrderId(order.venue_order_id.value),
        reason="s_code=51400, s_msg=Order cancellation failed as the order has been filled, canceled or does not exist",
        event_id=nautilus_pyo3.UUID4(),
        ts_event=123456789,
        ts_init=123456789,
        reconciliation=False,
        account_id=nautilus_pyo3.AccountId(TestIdStubs.account_id().value),
    )


@pytest.mark.asyncio
async def test_batch_cancel_orders_skips_pending_cancel_regular_orders(exec_client_builder):
    client, private_ws, _ = exec_client_builder()
    order = _make_pending_cancel_order()
    client._cache.add_order(order, None, None)

    regular_orders, algo_orders = client._categorize_orders_for_batch_cancel(
        [
            MagicMock(
                client_order_id=order.client_order_id,
                instrument_id=order.instrument_id,
                venue_order_id=order.venue_order_id,
            ),
        ],
    )

    assert regular_orders == []
    assert algo_orders == []
    private_ws.batch_cancel_orders.assert_not_awaited()


@pytest.mark.asyncio
async def test_handle_order_cancel_rejected_reconciles_terminal_gone_to_status_report(
    exec_client_builder,
    monkeypatch,
):
    client, _, _ = exec_client_builder()
    order = _make_pending_cancel_order()
    client._cache.add_order(order, None, None)
    client._cache.add_instrument(TestInstrumentProvider.default_fx_ccy("EUR/USD"))

    canceled_report = OrderStatusReport(
        account_id=TestIdStubs.account_id(),
        instrument_id=order.instrument_id,
        venue_order_id=order.venue_order_id,
        order_side=order.side,
        order_type=order.order_type,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.CANCELED,
        quantity=order.quantity,
        filled_qty=Quantity.from_int(0),
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=123456789,
        ts_init=123456789,
        client_order_id=order.client_order_id,
        contingency_type=order.contingency_type,
        price=order.price,
        post_only=order.is_post_only,
        reduce_only=order.is_reduce_only,
    )

    monkeypatch.setattr(client, "generate_order_status_report", AsyncMock(return_value=canceled_report))
    monkeypatch.setattr(client, "generate_fill_reports", AsyncMock(return_value=[]))

    sent_events: list[object] = []
    sent_reports: list[OrderStatusReport] = []
    monkeypatch.setattr(client, "_send_order_event", lambda event: sent_events.append(event))
    monkeypatch.setattr(client, "_send_order_status_report", lambda report: sent_reports.append(report))

    client._handle_order_cancel_rejected_pyo3(_make_terminal_gone_event(order))
    await asyncio.sleep(0)

    assert len(sent_events) == 1
    assert sent_reports == [canceled_report]


@pytest.mark.asyncio
async def test_handle_order_cancel_rejected_falls_back_to_canceled_when_status_missing(
    exec_client_builder,
    monkeypatch,
):
    client, _, _ = exec_client_builder()
    order = _make_pending_cancel_order()
    client._cache.add_order(order, None, None)

    monkeypatch.setattr(client, "generate_order_status_report", AsyncMock(return_value=None))
    monkeypatch.setattr(client, "generate_fill_reports", AsyncMock(return_value=[]))

    canceled: list[ClientOrderId] = []
    monkeypatch.setattr(
        client,
        "generate_order_canceled",
        lambda **kwargs: canceled.append(kwargs["client_order_id"]),
    )

    client._handle_order_cancel_rejected_pyo3(_make_terminal_gone_event(order))
    await asyncio.sleep(0)

    assert canceled == [order.client_order_id]
