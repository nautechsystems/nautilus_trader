"""Tests for the Rithmic execution client."""

import asyncio
from decimal import Decimal

import pytest

pytest.importorskip("nautilus_trader")

from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import (
    BatchCancelOrders,
    CancelAllOrders,
    GenerateFillReports,
    GenerateOrderStatusReports,
    GeneratePositionStatusReports,
)
from nautilus_trader.model.enums import (
    ContingencyType,
    OrderSide,
    OrderStatus,
    OrderType,
    PositionSide,
    TimeInForce,
)
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientId,
    ClientOrderId,
    InstrumentId,
    VenueOrderId,
)
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs

from nautilus_trader.adapters.rithmic.config import RithmicEnvironment, RithmicExecClientConfig
from nautilus_trader.adapters.rithmic.execution import RITHMIC_VENUE, RithmicLiveExecutionClient


def _make_client(
    loop: asyncio.AbstractEventLoop,
    native_bracket_state_path: str | None = None,
) -> RithmicLiveExecutionClient:
    clock = TestComponentStubs.clock()
    msgbus = MessageBus(
        trader_id=TestIdStubs.trader_id(),
        clock=clock,
    )
    cache = TestComponentStubs.cache()

    return RithmicLiveExecutionClient(  # type: ignore[abstract]
        loop=loop,
        client_id=ClientId("RITHMIC"),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=RithmicExecClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="u",
            password="p",
            system_name="s",
            account_id="A1",
            native_bracket_state_path=native_bracket_state_path,
        ),
    )


class _ExecutionPayload:
    def __init__(self, **kwargs) -> None:
        self.__dict__.update(kwargs)


class _ExecutionEvent:
    def __init__(self, kind: str, payload=None) -> None:
        self._kind = kind
        self._payload = payload

    def is_error(self):
        return self._kind == "error"

    def as_error(self):
        return self._payload

    def is_rejected(self):
        return self._kind == "rejected"

    def as_rejected(self):
        return self._payload

    def is_submitted(self):
        return self._kind == "submitted"

    def as_submitted(self):
        return self._payload

    def is_accepted(self):
        return self._kind == "accepted"

    def as_accepted(self):
        return self._payload

    def is_filled(self):
        return self._kind == "filled"

    def as_filled(self):
        return self._payload

    def is_cancelled(self):
        return self._kind == "cancelled"

    def as_cancelled(self):
        return self._payload

    def is_modified(self):
        return self._kind == "modified"

    def as_modified(self):
        return self._payload


class TestRithmicLiveExecutionClient:
    def test_venue(self):
        assert RITHMIC_VENUE.value == "RITHMIC"

    def test_submit_modify_cancel_stubs(self):
        calls = {}

        class DummyRust:
            async def submit_order(self, **kwargs):
                calls["submit"] = kwargs

            async def modify_order(self, **kwargs):
                calls["modify"] = kwargs

            async def cancel_order(self, venue_order_id):
                calls["cancel"] = venue_order_id

            async def batch_cancel_orders(self, ids):
                calls["batch"] = ids

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()

            order = TestExecStubs.limit_order(
                client_order_id=ClientOrderId("C1"),
            )
            submit = TestCommandStubs.submit_order_command(order)
            loop.run_until_complete(client._submit_order(submit))

            accepted = TestExecStubs.make_accepted_order(
                order=order,
                venue_order_id=VenueOrderId("V1"),
            )
            modify = TestCommandStubs.modify_order_command(
                order=accepted,
                quantity=Quantity.from_int(2),
                price=Price.from_str("54.0"),
            )
            loop.run_until_complete(client._modify_order(modify))

            cancel = TestCommandStubs.cancel_order_command(order=accepted)
            loop.run_until_complete(client._cancel_order(cancel))

            batch_cancel = BatchCancelOrders(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=accepted.instrument_id,
                cancels=[cancel],
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            )
            loop.run_until_complete(client._batch_cancel_orders(batch_cancel))
        finally:
            loop.close()

        assert calls["submit"]["client_order_id"] == "C1"
        assert calls["modify"]["venue_order_id"] == "V1"
        assert calls["modify"]["new_qty"] == 2
        assert calls["modify"]["new_price"] == 54.0
        assert calls["cancel"] == "V1"
        assert calls["batch"] == ["V1"]

    def test_cancel_all_orders_filters_by_instrument_and_side(self):
        calls = {}

        class DummyRust:
            def open_orders(self, **kwargs):
                calls["open_orders"] = kwargs
                return [
                    {
                        "client_order_id": "C1",
                        "symbol": "MNQH6",
                        "exchange": "CME",
                        "venue_order_id": "V1",
                        "side": "BUY",
                    },
                    {
                        "client_order_id": "C2",
                        "symbol": "MNQH6",
                        "exchange": "CME",
                        "venue_order_id": "V2",
                        "side": "BUY",
                    },
                ]

            async def cancel_orders(self, **kwargs):
                calls["cancel_orders"] = kwargs
                return 2

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()

            target_instrument_id = InstrumentId.from_str("MNQH6.RITHMIC")

            loop.run_until_complete(
                client._cancel_all_orders(
                    CancelAllOrders(
                        trader_id=TestIdStubs.trader_id(),
                        strategy_id=TestIdStubs.strategy_id(),
                        instrument_id=target_instrument_id,
                        order_side=OrderSide.BUY,
                        command_id=TestIdStubs.uuid(),
                        ts_init=0,
                    ),
                ),
            )
        finally:
            loop.close()

        assert calls["open_orders"] == {
            "symbol": "MNQH6",
            "exchange": "CME",
            "side": client._to_rithmic_side(OrderSide.BUY),
        }
        assert calls["cancel_orders"] == {
            "symbol": "MNQH6",
            "exchange": "CME",
            "side": client._to_rithmic_side(OrderSide.BUY),
        }

    @pytest.mark.parametrize("exit_path", ["close_position", "market_exit", "manage_stop"])
    def test_submit_order_supports_standard_strategy_exit_market_orders(self, exit_path):
        calls = {}

        class DummyRust:
            async def submit_order(self, **kwargs):
                calls["submit"] = kwargs

        class DummyOrder:
            account_id = AccountId("RITHMIC-A1")
            instrument_id = InstrumentId.from_str("MNQM6.RITHMIC")
            client_order_id = ClientOrderId(f"{exit_path.upper()}-1")
            side = OrderSide.SELL
            order_type = OrderType.MARKET
            quantity = Quantity.from_int(1)
            price = None
            trigger_price = None
            trailing_offset = None
            time_in_force = TimeInForce.GTC
            order_list_id = None
            linked_order_ids = None
            parent_order_id = None
            contingency_type = ContingencyType.NO_CONTINGENCY
            expire_time = None
            display_qty = None
            post_only = False
            reduce_only = True
            ts_init = 0
            venue_order_id = None

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()

            loop.run_until_complete(client._submit_order(_ExecutionPayload(order=DummyOrder())))
        finally:
            loop.close()

        submit = calls["submit"]
        state = client._orders[DummyOrder.client_order_id.value]
        assert submit["symbol"] == "MNQM6"
        assert submit["exchange"] == "CME"
        assert submit["side"] == client._to_rithmic_side(OrderSide.SELL)
        assert submit["order_type"] == client._to_rithmic_order_type(OrderType.MARKET)
        assert submit["time_in_force"] == client._to_rithmic_tif(TimeInForce.GTC)
        assert submit["quantity"] == 1
        assert state["order_type"] == OrderType.MARKET
        assert state["time_in_force"] == TimeInForce.GTC
        assert state["reduce_only"] is True

    def test_submit_order_rejects_mismatched_rithmic_account_id(self):
        class DummyRust:
            async def submit_order(self, **kwargs):
                raise AssertionError("submit_order should not be called for mismatched account routing")

        class DummyOrder:
            account_id = AccountId("RITHMIC-A2")
            instrument_id = InstrumentId.from_str("MNQH6.RITHMIC")
            client_order_id = ClientOrderId("C1")
            side = OrderSide.BUY
            order_type = OrderType.LIMIT
            quantity = Quantity.from_int(1)
            price = Price.from_str("21000")
            trigger_price = None
            trailing_offset = None
            time_in_force = TimeInForce.DAY
            order_list_id = None
            linked_order_ids = None
            parent_order_id = None
            contingency_type = ContingencyType.NO_CONTINGENCY
            expire_time = None
            display_qty = None
            post_only = False
            reduce_only = False
            ts_init = 0
            venue_order_id = None

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()
            with pytest.raises(ValueError, match="one client and one connection per account"):
                loop.run_until_complete(client._submit_order(_ExecutionPayload(order=DummyOrder())))
        finally:
            loop.close()

    def test_submit_order_list_uses_native_bracket_semantics(self):
        calls = {}

        class DummyRust:
            async def submit_bracket_order(self, **kwargs):
                calls["bracket"] = kwargs

            def get_order(self, client_order_id):
                if client_order_id == calls["bracket"]["client_order_id"]:
                    return {
                        "symbol": calls["bracket"]["symbol"],
                        "exchange": calls["bracket"]["exchange"],
                        "venue_order_id": "PB1",
                    }
                return None

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()

            instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
            client._cache.add_instrument(instrument)
            factory = OrderFactory(
                TestIdStubs.trader_id(),
                TestIdStubs.strategy_id(),
                client._clock,
                client._cache,
            )
            order_list = factory.bracket(
                instrument_id=instrument.id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
                entry_order_type=OrderType.LIMIT,
                entry_price=Price.from_str("1.00000"),
                tp_price=Price.from_str("1.01000"),
                sl_trigger_price=Price.from_str("0.99000"),
            )

            loop.run_until_complete(client._submit_order_list(_ExecutionPayload(order_list=order_list)))
        finally:
            loop.close()

        parent_order, stop_order, target_order = order_list.orders
        assert calls["bracket"]["client_order_id"] == parent_order.client_order_id.value
        assert calls["bracket"]["profit_ticks"] == 1000
        assert calls["bracket"]["stop_ticks"] == 1000
        assert client._orders[parent_order.client_order_id.value]["venue_order_id"] == VenueOrderId("PB1")
        assert client._orders[stop_order.client_order_id.value]["status"] == OrderStatus.INITIALIZED
        assert client._orders[target_order.client_order_id.value]["status"] == OrderStatus.INITIALIZED

    def test_submit_order_list_uses_native_oco_semantics(self):
        calls = {}

        class DummyRust:
            async def submit_oco_order(self, **kwargs):
                calls["oco"] = kwargs

            def get_order(self, client_order_id):
                venue_map = {
                    "C1": "V1",
                    "C2": "V2",
                }
                venue_order_id = venue_map.get(client_order_id)
                if venue_order_id is None:
                    return None
                return {
                    "symbol": "MNQZ4",
                    "exchange": "CME",
                    "venue_order_id": venue_order_id,
                }

        class DummyOrder:
            def __init__(self, **kwargs) -> None:
                self.__dict__.update(kwargs)

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()
            instrument_id = InstrumentId.from_str("MNQZ4.RITHMIC")
            first_id = ClientOrderId("C1")
            second_id = ClientOrderId("C2")
            first_order = DummyOrder(
                client_order_id=first_id,
                instrument_id=instrument_id,
                side=OrderSide.SELL,
                order_type=OrderType.LIMIT,
                quantity=Quantity.from_int(1),
                price=Price.from_str("20050.0"),
                trigger_price=None,
                time_in_force=TimeInForce.DAY,
                order_list_id=None,
                linked_order_ids=[second_id],
                parent_order_id=None,
                contingency_type=ContingencyType.OCO,
                ts_init=1,
            )
            second_order = DummyOrder(
                client_order_id=second_id,
                instrument_id=instrument_id,
                side=OrderSide.SELL,
                order_type=OrderType.STOP_MARKET,
                quantity=Quantity.from_int(1),
                price=None,
                trigger_price=Price.from_str("19950.0"),
                time_in_force=TimeInForce.DAY,
                order_list_id=None,
                linked_order_ids=[first_id],
                parent_order_id=None,
                contingency_type=ContingencyType.OCO,
                ts_init=1,
            )

            loop.run_until_complete(
                client._submit_order_list(
                    _ExecutionPayload(order_list=_ExecutionPayload(orders=[first_order, second_order]))
                )
            )
        finally:
            loop.close()

        assert calls["oco"]["leg1_client_order_id"] == "C1"
        assert calls["oco"]["leg2_client_order_id"] == "C2"
        assert client._orders["C1"]["venue_order_id"] == VenueOrderId("V1")
        assert client._orders["C2"]["venue_order_id"] == VenueOrderId("V2")

    def test_submit_order_list_rejects_unsupported_native_shapes(self):
        class DummyRust:
            pass

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()

            instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
            client._cache.add_instrument(instrument)
            factory = OrderFactory(
                TestIdStubs.trader_id(),
                TestIdStubs.strategy_id(),
                client._clock,
                client._cache,
            )
            unsupported = factory.bracket(
                instrument_id=instrument.id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
                entry_order_type=OrderType.MARKET,
                tp_price=Price.from_str("1.01000"),
                sl_trigger_price=Price.from_str("0.99000"),
            )

            with pytest.raises(ValueError, match="LIMIT entry orders"):
                loop.run_until_complete(
                    client._submit_order_list(_ExecutionPayload(order_list=unsupported))
                )
        finally:
            loop.close()

    @pytest.mark.asyncio
    async def test_pnl_events_populate_positions_and_balances(self):
        client = _make_client(asyncio.get_running_loop())

        class DummyAccountEvent:
            account_id = "A1"
            currency = "USD"
            total = 1000.0
            available = 900.0
            locked = 50.0
            unrealized_pnl = 10.0
            realized_pnl = 5.0

        class DummyPositionEvent:
            account_id = "A1"
            symbol = "ESZ4"
            exchange = "CME"
            quantity = 2.0
            avg_price = 5000.0
            unrealized_pnl = 12.5
            realized_pnl = 1.2
            ts_event = 1

        client._on_pnl_event(DummyAccountEvent())
        client._on_pnl_event(DummyPositionEvent())

        reports = await client.generate_position_status_reports(
            GeneratePositionStatusReports(
                instrument_id=None,
                start=None,
                end=None,
                command_id=UUID4(),
                ts_init=0,
            ),
        )

        assert client._balances["A1"]["currency"] == "USD"
        assert reports and len(reports) == 1

        report = reports[0]
        assert report.account_id.value == "RITHMIC-A1"
        assert report.instrument_id == InstrumentId.from_str("ESZ4.RITHMIC")
        assert report.position_side == PositionSide.LONG
        assert float(report.quantity) == 2.0
        assert report.avg_px_open == Decimal("5000.0")

    @pytest.mark.asyncio
    async def test_flatten_account_async_cancels_scoped_orders_and_flattens_positions(self):
        client = _make_client(asyncio.get_running_loop())

        target_instrument_id = InstrumentId.from_str("ESZ4.RITHMIC")
        cancelled: list[dict] = []
        submitted: list[dict] = []

        class DummyGateway:
            def __init__(self):
                self._positions = {
                    "ESZ4": _ExecutionPayload(
                        account_id="A1",
                        symbol="ESZ4",
                        exchange="CME",
                        quantity=1.0,
                        avg_price=5000.0,
                        unrealized_pnl=0.0,
                        realized_pnl=0.0,
                        ts_event=1,
                    ),
                    "NQZ4": _ExecutionPayload(
                        account_id="A1",
                        symbol="NQZ4",
                        exchange="CME",
                        quantity=2.0,
                        avg_price=21000.0,
                        unrealized_pnl=0.0,
                        realized_pnl=0.0,
                        ts_event=1,
                    ),
                }

            def positions(self, account_id=None):
                assert account_id == "A1"
                return list(self._positions.values())

        class DummyRust:
            def __init__(self):
                self._target_order_open = True

            def open_orders(self, **kwargs):
                symbol = kwargs.get("symbol")
                if symbol == "ESZ4" and self._target_order_open:
                    return [
                        {
                            "client_order_id": "OPEN-TARGET",
                            "symbol": "ESZ4",
                            "exchange": "CME",
                            "venue_order_id": "OV1",
                            "side": "BUY",
                        },
                    ]
                if symbol == "NQZ4":
                    return [
                        {
                            "client_order_id": "OPEN-OTHER",
                            "symbol": "NQZ4",
                            "exchange": "CME",
                            "venue_order_id": "OV2",
                            "side": "BUY",
                        },
                    ]
                return []

            async def cancel_orders(self, **kwargs):
                cancelled.append(kwargs)
                if kwargs.get("symbol") == "ESZ4":
                    self._target_order_open = False
                return 1

            async def submit_order(self, **kwargs):
                submitted.append(kwargs)
                asyncio.get_running_loop().call_soon(
                    self._fill_target_position,
                    kwargs["client_order_id"],
                    kwargs["symbol"],
                    kwargs["exchange"],
                    kwargs["quantity"],
                )

            def _fill_target_position(self, client_order_id, symbol, exchange, quantity):
                dummy_gateway._positions.pop(symbol, None)
                client._on_execution_event(
                    _ExecutionEvent(
                        "filled",
                        _ExecutionPayload(
                            client_order_id=client_order_id,
                            venue_order_id=f"FLAT-{client_order_id}",
                            symbol=symbol,
                            exchange=exchange,
                            side="SELL",
                            order_type="MARKET",
                            time_in_force="IOC",
                            fill_price=5000.0,
                            fill_qty=float(quantity),
                            leaves_qty=0.0,
                            commission=0.0,
                            trade_id=f"TRD-{client_order_id}",
                            currency="USD",
                            ts_event=10,
                        ),
                    ),
                )

        dummy_gateway = DummyGateway()
        client._gateway = dummy_gateway
        client._client = DummyRust()

        await client.flatten_account_async(
            instrument_ids=[target_instrument_id],
            time_in_force=TimeInForce.IOC,
            timeout_secs=2.0,
            poll_interval_secs=0.01,
        )

        assert cancelled == [{"symbol": "ESZ4", "exchange": "CME", "side": None}]
        assert len(submitted) == 1
        assert submitted[0]["symbol"] == "ESZ4"
        assert submitted[0]["exchange"] == "CME"
        assert submitted[0]["side"] == client._to_rithmic_side(OrderSide.SELL)
        assert submitted[0]["order_type"] == client._to_rithmic_order_type(OrderType.MARKET)
        assert submitted[0]["time_in_force"] == client._to_rithmic_tif(TimeInForce.IOC)
        remaining_positions = {position.symbol: position.quantity for position in dummy_gateway.positions("A1")}
        assert "ESZ4" not in remaining_positions
        assert remaining_positions == {"NQZ4": 2.0}

    @pytest.mark.asyncio
    async def test_fill_reports_preserve_event_order(self):
        client = _make_client(asyncio.get_running_loop())

        instrument_id = InstrumentId.from_str("ESZ4.RITHMIC")
        client._fills = [
            {
                "client_order_id": "C1",
                "venue_order_id": "V1",
                "price": Decimal("101.0"),
                "qty": Decimal("1.0"),
                "commission": Decimal("0"),
                "ts_event": 2,
                "ts_init": 2,
                "instrument_id": instrument_id,
                "side": OrderSide.BUY,
                "currency": "USD",
            },
            {
                "client_order_id": "C1",
                "venue_order_id": "V1",
                "price": Decimal("100.0"),
                "qty": Decimal("1.0"),
                "commission": Decimal("0"),
                "ts_event": 1,
                "ts_init": 1,
                "instrument_id": instrument_id,
                "side": OrderSide.BUY,
                "currency": "USD",
            },
        ]

        reports = await client.generate_fill_reports(
            GenerateFillReports(
                instrument_id=instrument_id,
                venue_order_id=None,
                start=None,
                end=None,
                command_id=UUID4(),
                ts_init=0,
            ),
        )

        assert [float(report.last_px) for report in reports] == [100.0, 101.0]
        assert [float(report.last_qty) for report in reports] == [1.0, 1.0]

    @pytest.mark.asyncio
    async def test_replayed_fill_recovers_context_without_cache_and_dedupes(self):
        client = _make_client(asyncio.get_running_loop())

        event = _ExecutionEvent(
            "filled",
            _ExecutionPayload(
                client_order_id="C9",
                venue_order_id="V9",
                symbol="ESZ4",
                exchange="CME",
                side="BUY",
                fill_price=5010.25,
                fill_qty=1.0,
                leaves_qty=0.0,
                commission=1.25,
                trade_id="FILL-1",
                currency="USD",
                ts_event=10,
            ),
        )

        client._on_execution_event(event)
        client._on_execution_event(event)

        assert len(client._fills) == 1
        assert client._orders["C9"]["instrument_id"] == InstrumentId.from_str("ESZ4.RITHMIC")
        assert client._orders["C9"]["order_side"] == OrderSide.BUY

        reports = await client.generate_fill_reports(
            GenerateFillReports(
                instrument_id=InstrumentId.from_str("ESZ4.RITHMIC"),
                venue_order_id=None,
                start=None,
                end=None,
                command_id=UUID4(),
                ts_init=0,
            ),
        )

        assert len(reports) == 1
        report = reports[0]
        assert report.instrument_id == InstrumentId.from_str("ESZ4.RITHMIC")
        assert report.trade_id.value == "FILL-1"
        assert report.order_side == OrderSide.BUY
        assert float(report.last_px) == 5010.25

    @pytest.mark.asyncio
    async def test_replayed_open_order_snapshot_rehydrates_status_report(self):
        client = _make_client(asyncio.get_running_loop())

        client._on_execution_event(
            _ExecutionEvent(
                "accepted",
                _ExecutionPayload(
                    client_order_id="C10",
                    venue_order_id="V10",
                    symbol="NQH5",
                    exchange="CME",
                    side="SELL",
                    order_type="LIMIT",
                    time_in_force="GTC",
                    quantity=3.0,
                    filled_qty=1.0,
                    leaves_qty=2.0,
                    price=21000.5,
                    avg_price=21000.25,
                    ts_event=100,
                ),
            ),
        )

        reports = await client.generate_order_status_reports(
            GenerateOrderStatusReports(
                instrument_id=None,
                open_only=False,
                start=None,
                end=None,
                command_id=UUID4(),
                ts_init=0,
            ),
        )

        assert len(reports) == 1
        report = reports[0]
        assert report.instrument_id == InstrumentId.from_str("NQH5.RITHMIC")
        assert report.order_side == OrderSide.SELL
        assert report.order_type == OrderType.LIMIT
        assert report.time_in_force == TimeInForce.GTC
        assert report.order_status == OrderStatus.ACCEPTED
        assert float(report.quantity) == 3.0
        assert float(report.filled_qty) == 1.0
        assert float(report.price) == 21000.5
        assert report.avg_px == Decimal("21000.25")

    def test_unrecoverable_native_bracket_parent_venue_ids_include_unknown_basket_ids(self):
        class DummyRust:
            async def show_brackets(self):
                return [{"basket_id": "PBX"}]

            async def show_bracket_stops(self):
                return [{"basket_id": "PBY"}]

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._client = DummyRust()
            unresolved = loop.run_until_complete(client._unrecoverable_native_bracket_parent_venue_ids())
            assert unresolved == ["PBX", "PBY"]
        finally:
            loop.close()

    def test_unrecoverable_native_bracket_parent_venue_ids_ignore_locally_persisted_parent_basket_ids(self, tmp_path):
        state_path = tmp_path / "native-brackets.json"

        class DummyRust:
            async def show_brackets(self):
                return [{"basket_id": "PB1"}]

            async def show_bracket_stops(self):
                return [{"basket_id": "PB1"}]

        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop, str(state_path))
            instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
            client._cache.add_instrument(instrument)
            factory = OrderFactory(
                TestIdStubs.trader_id(),
                TestIdStubs.strategy_id(),
                client._clock,
                client._cache,
            )
            order_list = factory.bracket(
                instrument_id=instrument.id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
                entry_order_type=OrderType.LIMIT,
                entry_price=Price.from_str("1.00000"),
                tp_price=Price.from_str("1.01000"),
                sl_trigger_price=Price.from_str("0.99000"),
            )
            parent_order, stop_order, target_order = order_list.orders
            client._register_native_bracket(parent_order, stop_order, target_order, "PB1")
            client._client = DummyRust()
            unresolved = loop.run_until_complete(client._unrecoverable_native_bracket_parent_venue_ids())
            assert unresolved == []
        finally:
            loop.close()

    def test_native_bracket_child_events_resolve_to_child_order_ids(self):
        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
            client._cache.add_instrument(instrument)
            factory = OrderFactory(
                TestIdStubs.trader_id(),
                TestIdStubs.strategy_id(),
                client._clock,
                client._cache,
            )
            order_list = factory.bracket(
                instrument_id=instrument.id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
                entry_order_type=OrderType.LIMIT,
                entry_price=Price.from_str("1.00000"),
                tp_price=Price.from_str("1.01000"),
                sl_trigger_price=Price.from_str("0.99000"),
            )
            parent_order, stop_order, target_order = order_list.orders
            client._seed_order_state(
                parent_order,
                status=OrderStatus.SUBMITTED,
                venue_order_id=VenueOrderId("PB1"),
            )
            client._seed_order_state(stop_order, status=OrderStatus.INITIALIZED)
            client._seed_order_state(target_order, status=OrderStatus.INITIALIZED)
            client._register_native_bracket(parent_order, stop_order, target_order, "PB1")

            client._on_execution_event(
                _ExecutionEvent(
                    "accepted",
                    _ExecutionPayload(
                        client_order_id=parent_order.client_order_id.value,
                        venue_order_id="SB1",
                        symbol="AUD/USD",
                        exchange="SIM",
                        side="SELL",
                        order_type="STOP_MARKET",
                        time_in_force="GTC",
                        quantity=1.0,
                        leaves_qty=1.0,
                        trigger_price=0.9900,
                        bracket_type="STOP_ONLY_STATIC",
                        original_basket_id="PB1",
                        linked_basket_ids=["TB1"],
                        ts_event=10,
                    ),
                ),
            )
            client._on_execution_event(
                _ExecutionEvent(
                    "filled",
                    _ExecutionPayload(
                        client_order_id=parent_order.client_order_id.value,
                        venue_order_id="TB1",
                        symbol="AUD/USD",
                        exchange="SIM",
                        side="SELL",
                        order_type="LIMIT",
                        quantity=1.0,
                        fill_price=1.0100,
                        fill_qty=1.0,
                        leaves_qty=0.0,
                        commission=0.0,
                        trade_id="FILL-TB1",
                        currency="USD",
                        bracket_type="TARGET_ONLY_STATIC",
                        original_basket_id="PB1",
                        linked_basket_ids=["SB1"],
                        ts_event=11,
                    ),
                ),
            )

            stop_state = client._orders[stop_order.client_order_id.value]
            target_state = client._orders[target_order.client_order_id.value]
            assert stop_state["venue_order_id"] == VenueOrderId("SB1")
            assert stop_state["status"] == OrderStatus.ACCEPTED
            assert target_state["status"] == OrderStatus.FILLED
            assert client._fills[0]["client_order_id"] == target_order.client_order_id.value
        finally:
            loop.close()

    def test_native_bracket_registry_persists_across_client_restart(self, tmp_path):
        state_path = tmp_path / "native-brackets.json"
        loop = asyncio.new_event_loop()
        try:
            first_client = _make_client(loop, str(state_path))
            instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
            first_client._cache.add_instrument(instrument)
            factory = OrderFactory(
                TestIdStubs.trader_id(),
                TestIdStubs.strategy_id(),
                first_client._clock,
                first_client._cache,
            )
            order_list = factory.bracket(
                instrument_id=instrument.id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
                entry_order_type=OrderType.LIMIT,
                entry_price=Price.from_str("1.00000"),
                tp_price=Price.from_str("1.01000"),
                sl_trigger_price=Price.from_str("0.99000"),
            )
            parent_order, stop_order, target_order = order_list.orders
            first_client._register_native_bracket(parent_order, stop_order, target_order, "PB1")

            assert state_path.exists()

            second_client = _make_client(loop, str(state_path))
            second_client._on_execution_event(
                _ExecutionEvent(
                    "accepted",
                    _ExecutionPayload(
                        client_order_id=parent_order.client_order_id.value,
                        venue_order_id="SB1",
                        symbol="AUD/USD",
                        exchange="SIM",
                        side="SELL",
                        order_type="STOP_MARKET",
                        time_in_force="GTC",
                        quantity=1.0,
                        leaves_qty=1.0,
                        trigger_price=0.9900,
                        bracket_type="STOP_ONLY_STATIC",
                        original_basket_id="PB1",
                        linked_basket_ids=["TB1"],
                        ts_event=10,
                    ),
                ),
            )

            stop_state = second_client._orders[stop_order.client_order_id.value]
            assert stop_state["venue_order_id"] == VenueOrderId("SB1")
            assert stop_state["parent_order_id"] == parent_order.client_order_id
            assert stop_state["linked_order_ids"] == [target_order.client_order_id]
            assert second_client._native_brackets_by_parent_venue_id["PB1"] == parent_order.client_order_id.value
        finally:
            loop.close()

    def test_native_bracket_registry_cleanup_removes_state_file(self, tmp_path):
        state_path = tmp_path / "native-brackets.json"
        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop, str(state_path))
            instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
            client._cache.add_instrument(instrument)
            factory = OrderFactory(
                TestIdStubs.trader_id(),
                TestIdStubs.strategy_id(),
                client._clock,
                client._cache,
            )
            order_list = factory.bracket(
                instrument_id=instrument.id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
                entry_order_type=OrderType.LIMIT,
                entry_price=Price.from_str("1.00000"),
                tp_price=Price.from_str("1.01000"),
                sl_trigger_price=Price.from_str("0.99000"),
            )
            parent_order, stop_order, target_order = order_list.orders
            client._seed_order_state(
                parent_order,
                status=OrderStatus.SUBMITTED,
                venue_order_id=VenueOrderId("PB1"),
            )
            client._seed_order_state(stop_order, status=OrderStatus.INITIALIZED)
            client._seed_order_state(target_order, status=OrderStatus.INITIALIZED)
            client._register_native_bracket(parent_order, stop_order, target_order, "PB1")

            assert state_path.exists()

            client._on_execution_event(
                _ExecutionEvent(
                    "cancelled",
                    _ExecutionPayload(
                        client_order_id=parent_order.client_order_id.value,
                        venue_order_id="PB1",
                        ts_event=20,
                    ),
                ),
            )

            assert client._native_brackets == {}
            assert client._native_brackets_by_parent_venue_id == {}
            assert not state_path.exists()
        finally:
            loop.close()

    def test_stale_execution_event_does_not_roll_back_order_state(self):
        loop = asyncio.new_event_loop()
        try:
            client = _make_client(loop)
            client._orders["C11"] = {
                "client_order_id": ClientOrderId("C11"),
                "instrument_id": InstrumentId.from_str("MNQM5.RITHMIC"),
                "order_side": OrderSide.BUY,
                "order_type": OrderType.LIMIT,
                "time_in_force": TimeInForce.DAY,
                "quantity": Decimal("1"),
                "filled_qty": Decimal("1"),
                "leaves_qty": Decimal("0"),
                "status": OrderStatus.FILLED,
                "ts_last": 200,
                "ts_init": 50,
                "venue_order_id": VenueOrderId("V11"),
            }

            client._on_execution_event(
                _ExecutionEvent(
                    "accepted",
                    _ExecutionPayload(
                        client_order_id="C11",
                        venue_order_id="V11",
                        symbol="MNQM5",
                        exchange="CME",
                        side="BUY",
                        order_type="LIMIT",
                        time_in_force="DAY",
                        quantity=1.0,
                        leaves_qty=1.0,
                        price=19000.0,
                        ts_event=100,
                    ),
                ),
            )

            state = client._orders["C11"]
            assert state["status"] == OrderStatus.FILLED
            assert state["ts_last"] == 200
        finally:
            loop.close()
