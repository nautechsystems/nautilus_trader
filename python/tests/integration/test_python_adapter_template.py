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
"""
Verify Python adapter configuration and live engine integration.
"""

from __future__ import annotations

import asyncio
import inspect
from decimal import Decimal
from typing import override

import pytest
from unit.adapters.example_modules import load_example_module

from nautilus_trader.common import Environment
from nautilus_trader.config import DataClientConfig
from nautilus_trader.config import ExecutionClientConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveNodeConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live.clients import ExecutionClientFactory
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import ClientId
from nautilus_trader.model import ExecutionMassStatus
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import OrderStatusReport
from nautilus_trader.model import OrderType
from nautilus_trader.model import PositionSide
from nautilus_trader.model import PositionStatusReport
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.model import VenueOrderId
from nautilus_trader.trading import Strategy
from tests.unit.model.factories import make_fill_report
from tests.unit.model.factories import make_order_status_report
from tests.unit.model.factories import make_position_status_report


constants = load_example_module("_template", "constants")
data = load_example_module("_template", "data")
execution = load_example_module("_template", "execution")
factories = load_example_module("_template", "factories")
providers = load_example_module("_template", "providers")
INSTRUMENT_ID = constants.INSTRUMENT_ID
VENUE = constants.VENUE
TemplateDataClient = data.TemplateDataClient
TemplateExecutionClient = execution.TemplateExecutionClient
TemplateDataClientFactory = factories.TemplateDataClientFactory
TemplateExecutionClientFactory = factories.TemplateExecutionClientFactory
TemplateInstrumentProvider = providers.TemplateInstrumentProvider


@pytest.mark.parametrize(
    "client_type",
    [TemplateDataClient, TemplateExecutionClient],
)
def test_template_declares_all_client_hooks(client_type) -> None:
    """
    Keep every adapter hook visible with compatible parameter names.
    """
    base_type = client_type.__bases__[0]
    hooks = {
        name: method
        for name, method in inspect.getmembers(base_type)
        if inspect.iscoroutinefunction(method)
        or name
        in {
            "_handles_order_venue",
            "_provides_bulk_position_coverage",
            "_calculate_commission",
        }
    }

    assert hooks.keys() <= client_type.__dict__.keys()
    for name, method in hooks.items():
        assert list(inspect.signature(client_type.__dict__[name]).parameters) == list(
            inspect.signature(method).parameters,
        ), name


def test_template_declares_instrument_loading_hooks() -> None:
    """
    Expose all venue instrument-loading entry points in the template.
    """
    assert {"load_all_async", "load_ids_async", "load_async"} <= (
        TemplateInstrumentProvider.__dict__.keys()
    )


@pytest.mark.parametrize(
    ("client_type", "method"),
    [
        (TemplateDataClient, "_subscribe_trades"),
        (TemplateDataClient, "_unsubscribe_trades"),
        (TemplateDataClient, "_request_quotes"),
        (TemplateExecutionClient, "_cancel_order"),
        (TemplateExecutionClient, "_generate_order_status_report"),
    ],
)
def test_template_unsupported_hooks_report_consistent_error(client_type, method) -> None:
    """
    Unsupported operations fail explicitly rather than reporting success.
    """
    with pytest.raises(NotImplementedError) as exc_info:
        asyncio.run(getattr(client_type, method)(object(), object()))

    assert str(exc_info.value) == "This operation is not implemented by the template adapter"


@pytest.mark.parametrize("launch", ["owned", "hosted", "uvloop"])
def test_deterministic_template_runs_through_live_engines(launch) -> None:
    """
    Deterministic template runs through live engines.
    """
    runner = pytest.importorskip("uvloop").run if launch == "uvloop" else asyncio.run
    node, strategy = build_node()
    if launch == "owned":
        node.run()
    else:
        runner(run_hosted(node))

    fill = strategy.fill
    assert fill.trader_id == TraderId("TEMPLATE-001")
    assert fill.account_id == AccountId("TEMPLATE-001")
    assert fill.strategy_id == strategy.strategy_id
    assert fill.instrument_id == INSTRUMENT_ID
    assert fill.client_order_id == strategy.order.client_order_id
    assert fill.order_side == OrderSide.BUY
    assert fill.last_qty == Quantity.from_str("1000")
    assert fill.last_px == Price.from_str("1.12349")
    assert fill.commission == Money.from_str("0.03 USD")
    assert fill.liquidity_side == LiquiditySide.TAKER
    assert node.cache.order(strategy.order.client_order_id).status == OrderStatus.FILLED
    assert node.cache.quote(INSTRUMENT_ID).bid_price == Price.from_str("1.12345")


@pytest.mark.parametrize(
    "kind",
    ["order", "fill", "position", "mass_order", "mass_fill", "mass_position", "mass_venue"],
)
def test_execution_output_rejects_foreign_report_identity(kind) -> None:
    """
    Execution output rejects foreign report identity.
    """
    errors = []

    class Client(TemplateExecutionClient):
        async def _connect(self):
            await super()._connect()

            if kind.endswith("order"):
                report = make_order_status_report(INSTRUMENT_ID, True)
            elif kind.endswith("fill"):
                report = make_fill_report(INSTRUMENT_ID)
            elif kind.endswith("position"):
                report = make_position_status_report(INSTRUMENT_ID)
            if kind.startswith("mass_"):
                mass = ExecutionMassStatus(
                    self.client_id,
                    self.account_id,
                    Venue("FOREIGN") if kind == "mass_venue" else VENUE,
                    41,
                )

                if kind != "mass_venue":
                    getattr(mass, f"add_{kind.removeprefix('mass_')}_reports")([report])
                report = mass
            try:
                self._handle_report(report)
            except TypeError as e:
                errors.append(str(e))

    class Factory(ExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return Client(
                **kwargs,
                venue=VENUE,
                account_id=AccountId("TEMPLATE-001"),
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )

    node, strategy = build_node(exec_factory=Factory)
    asyncio.run(run_hosted(node))

    assert errors == ["Execution output identity does not match its owner"]
    assert strategy.fill.last_qty == Quantity.from_str("1000")


@pytest.mark.parametrize("shutdown_cancel", [False, True])
def test_order_list_batch_fields_and_shutdown_cancellation(shutdown_cancel) -> None:  # noqa: C901 - Keep the real-engine lifecycle in one scenario.
    """
    Preserve batch fields and accept cancellations during the stop grace period.
    """
    submitted = []
    modified = []
    canceled = []
    queried = []
    client_id = ClientId("TEMPLATE")
    params = {"batch": "ordered", "sequence": 71}

    class Client(TemplateExecutionClient):
        async def _submit_order_list(self, command):
            submitted.append(command)
            for initialized in command.order_inits:
                order = self.cache.order(initialized.client_order_id)
                self.generate_order_submitted(order)
                self.generate_order_accepted(
                    order,
                    VenueOrderId(str(order.client_order_id)),
                    self.clock.timestamp_ns(),
                )

        async def _modify_order(self, command):
            modified.append(command)
            order = self.cache.order(command.client_order_id)
            self.generate_order_updated(
                order,
                order.venue_order_id,
                command.quantity,
                command.price,
                command.trigger_price,
                None,
                self.clock.timestamp_ns(),
            )

        async def _cancel_order(self, command):
            canceled.append(command)
            order = self.cache.order(command.client_order_id)
            self.generate_order_canceled(order, order.venue_order_id, self.clock.timestamp_ns())

        async def _query_account(self, command):
            queried.append(command)

        async def _query_order(self, command):
            queried.append(command)

    class Factory(ExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> Client:
            return Client(
                **kwargs,
                venue=VENUE,
                account_id=AccountId("TEMPLATE-001"),
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )

    class Consumer(Strategy):
        def on_start(self):
            self.orders = [
                self.order_factory.limit(
                    INSTRUMENT_ID,
                    OrderSide.BUY,
                    Quantity.from_str(qty),
                    Price.from_str(px),
                )
                for qty, px in [("7", "1.00001"), ("11", "1.00003")]
            ]
            self.accepted = 0
            self.updated = 0
            self.canceled = 0
            self.submit_order_list(self.orders, client_id=client_id, params=params)

        def on_order_accepted(self, event):  # noqa: ARG002 - Retain the strategy callback signature.
            self.accepted += 1
            if self.accepted != 2:
                return
            if shutdown_cancel:
                self.shutdown_system("Cancel during grace period")
            else:
                self.modify_orders(
                    [
                        (
                            self.orders[0].client_order_id,
                            Quantity.from_str("31"),
                            Price.from_str("1.01007"),
                            None,
                        ),
                        (
                            self.orders[1].client_order_id,
                            Quantity.from_str("47"),
                            Price.from_str("1.02009"),
                            None,
                        ),
                    ],
                    client_id=client_id,
                    params=params,
                )

        def on_order_updated(self, event):  # noqa: ARG002 - Retain the strategy callback signature.
            self.updated += 1
            if self.updated == 2:
                self.query_account(AccountId("TEMPLATE-001"), client_id=client_id, params=params)
                self.query_order(self.orders[0], client_id=client_id, params=params)
                self.cancel_orders(
                    [o.client_order_id for o in self.orders],
                    client_id=client_id,
                    params=params,
                )

        def on_order_canceled(self, event):  # noqa: ARG002 - Retain the strategy callback signature.
            self.canceled += 1
            if self.canceled == 2:
                self.shutdown_system("Batch canceled")

        def on_stop(self):
            if shutdown_cancel:
                self.cancel_orders(
                    [o.client_order_id for o in self.orders],
                    client_id=client_id,
                    params=params,
                )

    node = (
        LiveNode.builder("BATCH", TraderId("TEMPLATE-001"), Environment.SANDBOX)
        .with_delay_post_stop_secs(1 if shutdown_cancel else 0)
        .with_timeout_connection(2)
        .with_timeout_disconnection_secs(1)
        .add_data_client(
            "TEMPLATE",
            TemplateDataClientFactory,
            DataClientConfig(instrument_provider=InstrumentProviderConfig(load_all=True)),
        )
        .add_exec_client("TEMPLATE", Factory, ExecutionClientConfig())
        .build()
    )
    consumer = Consumer()
    node.add_strategy(consumer)
    asyncio.run(run_hosted(node))

    ids = [order.client_order_id for order in consumer.orders]
    assert len(submitted) == 1
    command = submitted[0]
    assert command.trader_id == TraderId("TEMPLATE-001")
    assert command.client_id == client_id
    assert command.strategy_id == consumer.strategy_id
    assert command.instrument_id == INSTRUMENT_ID
    assert command.order_list.client_order_ids() == ids
    assert [order.client_order_id for order in command.order_inits] == ids
    assert [order.quantity for order in command.order_inits] == [
        Quantity.from_str("7"),
        Quantity.from_str("11"),
    ]
    assert command.params == params
    assert command.position_id is None
    assert command.exec_algorithm_id is None
    assert [command.client_order_id for command in canceled] == ids
    assert [command.venue_order_id for command in canceled] == [
        VenueOrderId(str(value)) for value in ids
    ]
    assert [command.params for command in canceled] == [params, params]
    assert [node.cache.order(value).status for value in ids] == [
        OrderStatus.CANCELED,
        OrderStatus.CANCELED,
    ]

    if shutdown_cancel:
        assert modified == []
        assert queried == []
    else:
        assert [command.client_order_id for command in modified] == ids
        assert [command.quantity for command in modified] == [
            Quantity.from_str("31"),
            Quantity.from_str("47"),
        ]
        assert [command.price for command in modified] == [
            Price.from_str("1.01007"),
            Price.from_str("1.02009"),
        ]
        assert [command.params for command in modified] == [params, params]
        assert [command.params for command in queried] == [params, params]
        assert queried[0].account_id == AccountId("TEMPLATE-001")
        assert queried[1].client_order_id == ids[0]


@pytest.mark.parametrize("source", ["orders", "fills", "positions", "mass"])
def test_reconciliation_rejects_foreign_returned_reports(source) -> None:
    """
    Reconciliation rejects foreign returned reports.
    """
    clients = []

    class Client(TemplateExecutionClient):
        async def _generate_mass_status(self, lookback_mins):
            if source != "mass":
                return await super()._generate_mass_status(lookback_mins)
            report = ExecutionMassStatus(self.client_id, self.account_id, VENUE, 47)
            report.add_position_reports([make_position_status_report(INSTRUMENT_ID)])
            return report

        async def _generate_order_status_reports(self, _command):
            return [make_order_status_report(INSTRUMENT_ID, True)] if source == "orders" else []

        async def _generate_fill_reports(self, _command):
            return [make_fill_report(INSTRUMENT_ID)] if source == "fills" else []

        async def _generate_position_status_reports(self, _command):
            return [make_position_status_report(INSTRUMENT_ID)] if source == "positions" else []

    class Factory(ExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            client = Client(
                **kwargs,
                venue=VENUE,
                account_id=AccountId("TEMPLATE-001"),
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )
            clients.append(client)
            return client

    node, strategy = build_node(exec_factory=Factory)
    with pytest.raises(RuntimeError, match="Failed to get mass status from TEMPLATE"):
        asyncio.run(run_hosted(node))

    assert strategy.fill is None
    assert clients[0]._runtime.complete is True
    with pytest.raises(RuntimeError, match="disposed"):
        clients[0].cache.instrument(INSTRUMENT_ID)


def test_reconciliation_calls_external_order_and_commission_hooks() -> None:
    """
    Reconciliation calls external order and commission hooks.
    """
    commissions = []
    registrations = []
    instruments = []
    external_id = VenueOrderId("EXTERNAL-101")

    class Client(TemplateExecutionClient):
        async def _on_instrument(self, instrument):
            instruments.append((instrument, self.cache.instrument(instrument.id)))

        async def _generate_order_status_reports(self, _command):
            now = self.clock.timestamp_ns()
            return [
                OrderStatusReport(
                    self.account_id,
                    INSTRUMENT_ID,
                    external_id,
                    OrderSide.BUY,
                    OrderType.MARKET,
                    TimeInForce.GTC,
                    OrderStatus.FILLED,
                    Quantity.from_str("3"),
                    Quantity.from_str("3"),
                    now - 3,
                    now - 2,
                    now,
                    avg_px=Decimal("1.10000"),
                ),
            ]

        async def _generate_position_status_reports(self, _command):
            now = self.clock.timestamp_ns()
            return [
                PositionStatusReport(
                    self.account_id,
                    INSTRUMENT_ID,
                    PositionSide.LONG,
                    Quantity.from_str("3"),
                    now - 1,
                    now,
                ),
            ]

        def _calculate_commission(self, instrument, last_qty, last_px, liquidity_side):
            commissions.append((instrument, last_qty, last_px, liquidity_side))
            return Money.from_str("0.07 USD")

        async def _register_external_order(
            self,
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
            ts_init,
        ):
            registrations.append(
                (client_order_id, venue_order_id, instrument_id, strategy_id, ts_init),
            )

    class Factory(ExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return Client(
                **kwargs,
                venue=VENUE,
                account_id=AccountId("TEMPLATE-001"),
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )

    node, strategy = build_node(exec_factory=Factory)
    asyncio.run(run_hosted(node))

    instrument = node.cache.instrument(INSTRUMENT_ID)
    external_order = node.cache.order(node.cache.client_order_id(external_id))
    assert instruments == [(instrument, instrument)]
    assert commissions == [
        (instrument, Quantity.from_str("3"), Price.from_str("1.10000"), LiquiditySide.TAKER),
    ]
    assert registrations == [
        (
            external_order.client_order_id,
            external_id,
            INSTRUMENT_ID,
            StrategyId("EXTERNAL"),
            external_order.last_event.ts_init,
        ),
    ]
    assert external_order.status == OrderStatus.FILLED
    assert external_order.last_event.commission == Money.from_str("0.07 USD")
    assert strategy.fill.last_qty == Quantity.from_str("1000")


@pytest.mark.parametrize(
    ("kind", "expected_status"),
    [
        ("denied", OrderStatus.DENIED),
        ("rejected", OrderStatus.REJECTED),
        ("modify_rejected", OrderStatus.ACCEPTED),
        ("cancel_rejected", OrderStatus.ACCEPTED),
        ("triggered", OrderStatus.TRIGGERED),
        ("expired", OrderStatus.EXPIRED),
    ],
)
def test_execution_generated_event_reaches_core_and_strategy(kind, expected_status) -> None:  # noqa: C901 - Keep each real-engine order transition and its assertions together.
    """
    Generated events preserve their fields and apply the corresponding core transition.
    """
    emitted = []
    received = []
    venue_order_id = VenueOrderId("VENUE-173")
    reason = "Distinct venue rejection reason"
    event_ns = 1704164646000000037

    class Client(TemplateExecutionClient):
        async def _submit_order(self, command):
            order = command.order
            if kind == "denied":
                self.generate_order_denied(order, reason)
                return
            self.generate_order_submitted(order)
            if kind == "rejected":
                self.generate_order_rejected(order, reason, event_ns, due_post_only=True)
            else:
                self.generate_order_accepted(order, venue_order_id, event_ns - 11)
                if not kind.endswith("rejected"):
                    getattr(self, f"generate_order_{kind}")(order, venue_order_id, event_ns)

        async def _modify_order(self, command):
            self.generate_order_modify_rejected(
                self.cache.order(command.client_order_id),
                venue_order_id,
                reason,
                event_ns,
            )

        async def _cancel_order(self, command):
            self.generate_order_cancel_rejected(
                self.cache.order(command.client_order_id),
                venue_order_id,
                reason,
                event_ns,
            )

    class Factory(ExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            client = Client(
                **kwargs,
                venue=VENUE,
                account_id=AccountId("TEMPLATE-001"),
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )
            emitted.append(client)
            return client

    class Consumer(Strategy):
        def on_start(self):
            self.order = self.order_factory.stop_limit(
                INSTRUMENT_ID,
                OrderSide.BUY,
                Quantity.from_str("17"),
                Price.from_str("1.51009"),
                Price.from_str("1.50007"),
            )
            self.submit_order(self.order)

        def on_order_accepted(self, event):  # noqa: ARG002 - Preserve the strategy callback signature.
            if kind == "modify_rejected":
                self.modify_order(self.order.client_order_id, quantity=Quantity.from_str("23"))
            elif kind == "cancel_rejected":
                self.cancel_order(self.order.client_order_id)

    def record(self, event):
        received.append(event)
        self.shutdown_system("Generated event received")

    setattr(Consumer, f"on_order_{kind}", record)
    node = (
        LiveNode.builder("EVENTS", TraderId("TEMPLATE-001"), Environment.SANDBOX)
        .with_delay_post_stop_secs(0)
        .with_timeout_connection(2)
        .with_timeout_disconnection_secs(1)
        .add_data_client(
            "TEMPLATE",
            TemplateDataClientFactory,
            DataClientConfig(instrument_provider=InstrumentProviderConfig(load_all=True)),
        )
        .add_exec_client("TEMPLATE", Factory, ExecutionClientConfig())
        .build()
    )
    consumer = Consumer()
    node.add_strategy(consumer)
    asyncio.run(run_hosted(node))

    assert len(received) == 1
    event = received[0]
    assert event.trader_id == TraderId("TEMPLATE-001")
    assert event.strategy_id == consumer.strategy_id
    assert event.instrument_id == INSTRUMENT_ID
    assert event.client_order_id == consumer.order.client_order_id
    if kind != "denied":
        assert event.reconciliation is False
    if kind == "denied":
        assert event.ts_event == event.ts_init
    else:
        assert event.account_id == AccountId("TEMPLATE-001")
        assert event.ts_event == event_ns
    if kind in ("denied", "rejected", "modify_rejected", "cancel_rejected"):
        assert event.reason == reason
    if kind == "rejected":
        assert event.due_post_only is True
    if kind not in ("denied", "rejected"):
        assert event.venue_order_id == venue_order_id
    assert node.cache.order(consumer.order.client_order_id).status == expected_status
    assert emitted[0].is_connected is False


def test_adapter_cache_queries_follow_core_order_and_position_states(monkeypatch) -> None:  # noqa: C901 - Keep cache snapshots and transition assertions in one scenario.
    """
    Return filtered, owned cache snapshots at each order transition.
    """
    from nautilus_trader.model import ClientOrderId
    from nautilus_trader.model import InstrumentId
    from nautilus_trader.model import OrderListId
    from nautilus_trader.model import PositionId

    clients = []
    snapshots = []
    original_fill = TemplateStrategy.on_order_filled

    class Factory(TemplateExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            client = TemplateExecutionClientFactory.create(**kwargs)
            clients.append(client)
            return client

    def capture(self, event):
        cache = clients[0].cache
        order_id = event.client_order_id
        filters = {
            "venue": VENUE,
            "instrument_id": INSTRUMENT_ID,
            "strategy_id": self.strategy_id,
            "account_id": AccountId("TEMPLATE-001"),
        }
        mismatches = {
            "venue": Venue("OTHER"),
            "instrument_id": InstrumentId.from_str("GBP/USD.TEMPLATE"),
            "strategy_id": StrategyId("OTHER-001"),
            "account_id": AccountId("OTHER-001"),
            "side": OrderSide.SELL,
        }
        collections = {}

        for method in ("orders", "orders_open", "orders_inflight"):
            query = getattr(cache, method)
            collections[method] = [o.client_order_id for o in query(**filters, side=OrderSide.BUY)]
            collections[f"{method}:unfiltered"] = [o.client_order_id for o in query()]
            for field, value in mismatches.items():
                collections[f"{method}:{field}"] = query(**{**filters, field: value})
        positions = cache.positions_open(**filters, side=PositionSide.LONG)
        position_misses = {}
        for field, value in {**mismatches, "side": PositionSide.SHORT}.items():
            position_misses[field] = cache.positions_open(**{**filters, field: value})
        snapshots.append(
            {
                "type": type(event).__name__,
                "order": cache.order(order_id),
                "collections": collections,
                "count": cache.orders_open_count(**filters, side=OrderSide.BUY),
                "count_wrong_side": cache.orders_open_count(**filters, side=OrderSide.SELL),
                "open_ids": cache.client_order_ids_open(**filters),
                "wrong_open_ids": cache.client_order_ids_open(venue=Venue("OTHER")),
                "positions": positions,
                "position_misses": position_misses,
                "position_id": cache.position_id(order_id),
                "position": cache.position(positions[0].id) if positions else None,
                "venue_order_id": cache.venue_order_id(order_id),
                "client_order_id": cache.client_order_id(event.venue_order_id)
                if hasattr(event, "venue_order_id")
                else None,
                "strategy_id": cache.strategy_id_for_order(order_id),
                "account": cache.account(AccountId("TEMPLATE-001")),
                "instrument_ids": cache.instrument_ids(VENUE),
                "instruments": cache.instruments(VENUE),
                "foreign_instruments": cache.instruments(Venue("OTHER")),
                "foreign_ids": cache.instrument_ids(Venue("OTHER")),
                "missing": [
                    cache.get("absent"),
                    cache.order(ClientOrderId("absent")),
                    cache.account(AccountId("OTHER-001")),
                    cache.position(PositionId("absent")),
                    cache.order_list(OrderListId("absent")),
                    cache.order_book(INSTRUMENT_ID),
                    cache.quote(INSTRUMENT_ID, 1),
                    cache.instrument(InstrumentId.from_str("GBP/USD.TEMPLATE")),
                    cache.client_order_id(VenueOrderId("absent")),
                    cache.venue_order_id(ClientOrderId("absent")),
                    cache.position_id(ClientOrderId("absent")),
                    cache.strategy_id_for_order(ClientOrderId("absent")),
                ],
            },
        )

        if type(event).__name__ == "OrderFilled":
            original_fill(self, event)

    for event in ("submitted", "accepted", "filled"):
        monkeypatch.setattr(TemplateStrategy, f"on_order_{event}", capture)
    node, strategy = build_node(exec_factory=Factory)
    asyncio.run(run_hosted(node))

    assert [snapshot["type"] for snapshot in snapshots] == [
        "OrderSubmitted",
        "OrderAccepted",
        "OrderFilled",
    ]

    for snapshot, status in zip(
        snapshots,
        [OrderStatus.SUBMITTED, OrderStatus.ACCEPTED, OrderStatus.FILLED],
        strict=True,
    ):
        assert snapshot["order"].status == status
        assert snapshot["order"].client_order_id == strategy.order.client_order_id
        assert snapshot["collections"]["orders"] == [strategy.order.client_order_id]
        assert snapshot["collections"]["orders:unfiltered"] == [strategy.order.client_order_id]

        for method, expected in (
            ("orders_open", status == OrderStatus.ACCEPTED),
            ("orders_inflight", status == OrderStatus.SUBMITTED),
        ):
            ids = [strategy.order.client_order_id] if expected else []
            assert snapshot["collections"][method] == ids
            assert snapshot["collections"][f"{method}:unfiltered"] == ids
        assert all(
            value == []
            for key, value in snapshot["collections"].items()
            if ":" in key and not key.endswith(":unfiltered")
        )
        assert snapshot["count"] == (1 if status == OrderStatus.ACCEPTED else 0)
        assert snapshot["count_wrong_side"] == 0
        assert snapshot["open_ids"] == (
            [strategy.order.client_order_id] if status == OrderStatus.ACCEPTED else []
        )
        assert snapshot["wrong_open_ids"] == []
        assert snapshot["strategy_id"] == strategy.strategy_id
        assert snapshot["account"].id == AccountId("TEMPLATE-001")
        assert snapshot["instrument_ids"] == [INSTRUMENT_ID]
        assert [instrument.id for instrument in snapshot["instruments"]] == [INSTRUMENT_ID]
        assert snapshot["foreign_instruments"] == []
        assert snapshot["foreign_ids"] == []
        assert snapshot["missing"] == [None] * 12
        assert snapshot["position_misses"] == {
            field: [] for field in ("venue", "instrument_id", "strategy_id", "account_id", "side")
        }

        if status == OrderStatus.FILLED:
            assert [position.id for position in snapshot["positions"]] == [
                strategy.fill.position_id,
            ]
            assert snapshot["position"].quantity == Quantity.from_str("1000")
            assert snapshot["position_id"] == strategy.fill.position_id
        else:
            assert snapshot["positions"] == []
            assert snapshot["position"] is None
        assert snapshot["venue_order_id"] == (
            None if status == OrderStatus.SUBMITTED else strategy.fill.venue_order_id
        )
        assert snapshot["client_order_id"] == (
            None if status == OrderStatus.SUBMITTED else strategy.order.client_order_id
        )


@pytest.mark.parametrize(
    ("kind", "message"),
    [
        ("trader", "Execution client trader identity does not match its node"),
        ("tolerance", "Position reconciliation tolerance must be nonnegative"),
        ("venue", "Execution clients require a venue"),
    ],
)
def test_execution_factory_rejects_invalid_identity_and_tolerance(kind, message) -> None:
    """
    Invalid execution identities fail before an adapter can enter the core.
    """
    clients = []

    class InvalidFactory(TemplateExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            client = TemplateExecutionClientFactory.create(**kwargs)
            if kind == "trader":
                client.trader_id = TraderId("OTHER-001")
            elif kind == "tolerance":
                client.position_reconciliation_tolerance = Decimal("-0.00000001")
            else:
                client.venue = None
            clients.append(client)
            return client

    with pytest.raises(RuntimeError, match=message):
        build_node(exec_factory=InvalidFactory)

    assert len(clients) == 1
    with pytest.raises(RuntimeError, match="disposed"):
        clients[0].cache.instrument(INSTRUMENT_ID)


@pytest.mark.parametrize(
    "kind",
    ["unknown", "fills_without_order", "foreign_associated_fill", "foreign_event"],
)
def test_execution_output_rejects_invalid_payload_before_core_dispatch(kind) -> None:
    """
    Invalid report combinations and foreign events never reach core processing.
    """
    from nautilus_trader.core import UUID4
    from nautilus_trader.model import ClientOrderId
    from nautilus_trader.model import OrderSubmitted

    rejected = []

    class Client(TemplateExecutionClient):
        async def _connect(self):
            await super()._connect()

            if kind == "unknown":
                with pytest.raises(TypeError, match="Expected a Nautilus execution report"):
                    self._handle_report(object())
            elif kind == "fills_without_order":
                with pytest.raises(
                    TypeError,
                    match="Associated fills require an OrderStatusReport",
                ):
                    self._handle_report(make_position_status_report(INSTRUMENT_ID), [])
            elif kind == "foreign_associated_fill":
                report = make_order_status_report(INSTRUMENT_ID, False)
                # Preserve valid outer identity so the associated fill determines rejection.
                values = report.to_dict()
                values["account_id"] = str(self.account_id)
                report = OrderStatusReport.from_dict(values)
                with pytest.raises(
                    TypeError,
                    match="Execution output identity does not match its owner",
                ):
                    self._handle_report(report, [make_fill_report(INSTRUMENT_ID)])
            else:
                event = OrderSubmitted(
                    TraderId("OTHER-001"),
                    StrategyId("OTHER-002"),
                    INSTRUMENT_ID,
                    ClientOrderId("OTHER-3"),
                    self.account_id,
                    UUID4(),
                    149,
                    151,
                )
                with pytest.raises(
                    TypeError,
                    match="Execution output identity does not match its owner",
                ):
                    self._handle_event(event)
            rejected.append(kind)

    class Factory(TemplateExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return Client(
                **kwargs,
                venue=VENUE,
                account_id=AccountId("TEMPLATE-001"),
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )

    node, strategy = build_node(exec_factory=Factory)
    asyncio.run(run_hosted(node))

    assert rejected == [kind]
    assert strategy.fill.last_qty == Quantity.from_str("1000")
    assert node.cache.order(strategy.order.client_order_id).status == OrderStatus.FILLED


@pytest.mark.parametrize("output", ["generated", "event", "batch"])
def test_cancel_all_orders_preserves_filter_and_cancels_core_order(output) -> None:  # noqa: C901 - Keep generated, single, and batched output transitions together.
    """
    Bulk cancellation forwards the strategy, side, and params to the custom client.
    """
    from nautilus_trader.core import UUID4
    from nautilus_trader.model import OrderAccepted
    from nautilus_trader.model import OrderCanceled
    from nautilus_trader.model import OrderSubmitted

    commands = []
    received = []
    params = {"reason": "session_end", "sequence": 179}

    class Client(TemplateExecutionClient):
        async def _submit_order(self, command):
            if output == "generated":
                self.generate_order_submitted(command.order)
                self.generate_order_accepted(command.order, VenueOrderId("CANCEL-181"), 191)
            else:
                submitted = OrderSubmitted(
                    self.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    self.account_id,
                    UUID4(),
                    181,
                    187,
                )
                accepted = OrderAccepted(
                    self.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    VenueOrderId("CANCEL-181"),
                    self.account_id,
                    UUID4(),
                    191,
                    192,
                    False,
                )

                if output == "event":
                    self._handle_event(submitted)
                    self._handle_event(accepted)
                else:
                    self._handle_order_submitted_batch([submitted])
                    self._handle_order_accepted_batch([accepted])

        async def _cancel_all_orders(self, command):
            commands.append(command)
            for order in self.cache.orders_open(
                instrument_id=command.instrument_id,
                side=command.order_side,
            ):
                if output == "generated":
                    self.generate_order_canceled(order, order.venue_order_id, 193)
                else:
                    canceled = OrderCanceled(
                        self.trader_id,
                        order.strategy_id,
                        order.instrument_id,
                        order.client_order_id,
                        UUID4(),
                        193,
                        197,
                        False,
                        order.venue_order_id,
                        self.account_id,
                        "Venue cancellation",
                    )

                    if output == "event":
                        self._handle_event(canceled)
                    else:
                        self._handle_order_canceled_batch([canceled])

    class Factory(TemplateExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return Client(
                **kwargs,
                venue=VENUE,
                account_id=AccountId("TEMPLATE-001"),
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )

    class Consumer(Strategy):
        def on_start(self):
            self.order = self.order_factory.limit(
                INSTRUMENT_ID,
                OrderSide.BUY,
                Quantity.from_str("17"),
                Price.from_str("1.10003"),
            )
            self.submit_order(self.order)

        def on_order_accepted(self, event):  # noqa: ARG002 - Preserve the strategy callback signature.
            self.cancel_all_orders(
                INSTRUMENT_ID,
                order_side=OrderSide.BUY,
                client_id=ClientId("TEMPLATE"),
                strategy_only=False,
                params=params,
            )

        def on_order_canceled(self, event):
            received.append(event)
            self.shutdown_system("Cancel all completed")

    node = (
        LiveNode.builder("CANCEL-ALL", TraderId("TEMPLATE-001"), Environment.SANDBOX)
        .with_delay_post_stop_secs(0)
        .with_timeout_connection(2)
        .with_timeout_disconnection_secs(1)
        .add_data_client(
            "TEMPLATE",
            TemplateDataClientFactory,
            DataClientConfig(instrument_provider=InstrumentProviderConfig(load_all=True)),
        )
        .add_exec_client("TEMPLATE", Factory, ExecutionClientConfig())
        .build()
    )
    consumer = Consumer()
    node.add_strategy(consumer)
    asyncio.run(run_hosted(node))

    assert len(commands) == 1
    command = commands[0]
    assert command.trader_id == TraderId("TEMPLATE-001")
    assert command.strategy_id == consumer.strategy_id
    assert command.client_id == ClientId("TEMPLATE")
    assert command.instrument_id == INSTRUMENT_ID
    assert command.order_side == OrderSide.BUY
    assert command.params == params
    assert len(received) == 1
    assert received[0].client_order_id == consumer.order.client_order_id
    assert received[0].venue_order_id == VenueOrderId("CANCEL-181")
    assert received[0].ts_event == 193
    assert node.cache.order(consumer.order.client_order_id).status == OrderStatus.CANCELED

    if output != "generated":
        assert received[0].ts_init == 197
        assert received[0].reason == "Venue cancellation"
        assert received[0].account_id == AccountId("TEMPLATE-001")
        assert received[0].trader_id == TraderId("TEMPLATE-001")
        assert received[0].strategy_id == consumer.strategy_id
        assert received[0].instrument_id == INSTRUMENT_ID
        assert received[0].reconciliation is False


def build_node(
    data_factory=TemplateDataClientFactory,
    exec_factory=TemplateExecutionClientFactory,
) -> tuple[LiveNode, TemplateStrategy]:
    """
    Build the configured node and register its deterministic strategy.
    """
    node = LiveNode.build(
        "PYTHON-TEMPLATE",
        LiveNodeConfig(
            environment=Environment.SANDBOX,
            trader_id=TraderId("TEMPLATE-001"),
            timeout_connection_secs=2,
            timeout_reconciliation_secs=2,
            timeout_portfolio_secs=2,
            timeout_disconnection_secs=1,
            delay_post_stop_secs=0,
            data_clients={
                "TEMPLATE": DataClientConfig(
                    instrument_provider=InstrumentProviderConfig(load_all=True),
                ),
            },
            exec_clients={"TEMPLATE": ExecutionClientConfig()},
        ),
        data_factories={"TEMPLATE": data_factory},
        exec_factories={"TEMPLATE": exec_factory},
    )
    strategy = TemplateStrategy()
    node.add_strategy(strategy)
    return node, strategy


async def run_hosted(node) -> None:
    """
    Await coordinated node shutdown within the example timeout.
    """
    async with asyncio.timeout(10):
        await node.run_async()


class TemplateStrategy(Strategy):
    """
    Submit a market order on the first quote and stop after its fill.
    """

    def __init__(self) -> None:
        """
        Retain the component inputs without starting asynchronous work.
        """
        super().__init__()
        self.order = None
        self.fill = None

    def on_start(self) -> None:
        """
        Subscribe to the template quote stream.
        """
        self.subscribe_quotes(INSTRUMENT_ID)

    @override
    def on_quote(self, quote: QuoteTick) -> None:
        """
        Submit the market order once a quote reaches the strategy.
        """
        if self.order is None:
            self.order = self.order_factory.market(
                INSTRUMENT_ID,
                OrderSide.BUY,
                Quantity.from_str("1000"),
            )
            self.submit_order(self.order)

    def on_order_filled(self, event) -> None:
        """
        Retain the fill and request coordinated shutdown.
        """
        self.fill = event
        self.shutdown_system("Template order filled")
