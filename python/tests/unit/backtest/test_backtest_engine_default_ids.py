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
End-to-end coverage for class-derived default IDs.

These run a backtest rather than only registering components, because an actor ID is the
message bus routing key and the actor registry key. A derived ID that registers but does
not route would pass a registration-only check.

"""

from decimal import Decimal

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import ActorId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import Venue
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from tests.providers import TestInstrumentProvider


USDT = Currency.from_str("USDT")
ETHUSDT = TestInstrumentProvider.ethusdt_binance()

# 2020-09-13T12:26:40Z, so generated client order IDs are deterministic
BASE_NS = 1_600_000_000_000_000_000
QUOTE_COUNT = 6


class RecordingActor(DataActor):
    """
    Overrides lifecycle and data handlers, and records what it observed at runtime.
    """

    def __init__(self, config: object = None) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self.started = 0
        self.stopped = 0
        self.quotes = 0
        self.actor_id_while_running = None

    def on_start(self) -> None:
        """
        On start.
        """
        self.started += 1
        self.actor_id_while_running = str(self.actor_id)
        self.subscribe_quotes(ETHUSDT.id)

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.stopped += 1

    def on_quote(self, _quote: object) -> None:
        """
        On quote.
        """
        self.quotes += 1


class SecondRecordingActor(RecordingActor):
    """
    Collect second recording actor tests.
    """


class NonForwardingActor(DataActor):
    """
    Collect non forwarding actor tests.
    """

    def __init__(self, _config: object = None) -> None:
        """
        Initialize the helper.
        """
        # Deliberately does not forward to `super().__init__()`, so the ID and the Python self
        # reference are both established by registration rather than construction
        self.started = 0
        self.quotes = 0

    def on_start(self) -> None:
        """
        On start.
        """
        self.started += 1
        self.subscribe_quotes(ETHUSDT.id)

    def on_quote(self, _quote: object) -> None:
        """
        On quote.
        """
        self.quotes += 1


class SecondNonForwardingActor(NonForwardingActor):
    """
    Collect second non forwarding actor tests.
    """


class RecordingStrategy(Strategy):
    """
    Record identities and submit one order per quote up to ``order_limit``.
    """

    order_limit = 1

    def __init__(self, config: object = None) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self.started = 0
        self.stopped = 0
        self.quotes = 0
        self.submitted = []
        self.filled = []
        self.strategy_id_while_running = None

    def on_start(self) -> None:
        """
        On start.
        """
        self.started += 1
        self.strategy_id_while_running = str(self.strategy_id)
        self.subscribe_quotes(ETHUSDT.id)

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.stopped += 1

    def on_quote(self, _quote: object) -> None:
        """
        On quote.
        """
        self.quotes += 1

        if len(self.submitted) >= type(self).order_limit:
            return

        order = self.order_factory.market(
            instrument_id=ETHUSDT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.10000"),
        )
        self.submitted.append(order.client_order_id)
        self.submit_order(order)

    def on_order_filled(self, event: object) -> None:
        """
        On order filled.
        """
        self.filled.append(event.client_order_id)


class SecondRecordingStrategy(RecordingStrategy):
    """
    Collect second recording strategy tests.
    """


class CountingStrategy(RecordingStrategy):
    """
    Collect counting strategy tests.
    """

    order_limit = 2


class TaggedStrategy(RecordingStrategy):
    """
    Collect tagged strategy tests.
    """


class ConfiguredStrategy(RecordingStrategy):
    """
    Collect configured strategy tests.
    """


def _quotes(count: int = QUOTE_COUNT) -> list[QuoteTick]:
    ticks = []

    for i in range(count):
        mid = Decimal("2000.00") + Decimal("1.00") * i
        ticks.append(
            QuoteTick(
                instrument_id=ETHUSDT.id,
                bid_price=Price.from_decimal_dp(mid - Decimal("0.05"), ETHUSDT.price_precision),
                ask_price=Price.from_decimal_dp(mid + Decimal("0.05"), ETHUSDT.price_precision),
                bid_size=Quantity.from_decimal_dp(Decimal(10), ETHUSDT.size_precision),
                ask_size=Quantity.from_decimal_dp(Decimal(10), ETHUSDT.size_precision),
                ts_event=BASE_NS + (i * 1_000_000_000),
                ts_init=BASE_NS + (i * 1_000_000_000),
            ),
        )

    return ticks


def _engine() -> BacktestEngine:
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, USDT)],
        base_currency=USDT,
    )
    engine.add_instrument(ETHUSDT)

    return engine


def test_overridden_actor_subclasses_receive_data_under_class_derived_ids() -> None:
    """
    Test overridden actor subclasses receive data under class derived ids.
    """
    engine = _engine()
    first = RecordingActor()
    second = SecondRecordingActor()

    try:
        engine.add_actor(first)
        engine.add_actor(second)
        engine.add_data(_quotes())
        engine.run()

        assert first.actor_id == ActorId("RecordingActor")
        assert second.actor_id == ActorId("SecondRecordingActor")
        assert first.actor_id_while_running == "RecordingActor"
        assert second.actor_id_while_running == "SecondRecordingActor"
        assert first.started == 1
        assert second.started == 1
        assert first.stopped == 1
        assert second.stopped == 1
        assert first.quotes == QUOTE_COUNT
        assert second.quotes == QUOTE_COUNT
    finally:
        engine.dispose()


def test_actor_subclasses_that_skip_super_init_still_derive_distinct_ids() -> None:
    """
    Test actor subclasses that skip super init still derive distinct ids.
    """
    engine = _engine()
    first = NonForwardingActor()
    second = SecondNonForwardingActor()

    try:
        engine.add_actor(first)
        engine.add_actor(second)
        engine.add_data(_quotes())
        engine.run()

        assert first.actor_id == ActorId("NonForwardingActor")
        assert second.actor_id == ActorId("SecondNonForwardingActor")
        assert first.started == 1
        assert second.started == 1
        assert first.quotes == QUOTE_COUNT
        assert second.quotes == QUOTE_COUNT
    finally:
        engine.dispose()


def test_configured_actor_id_survives_a_run() -> None:
    """
    Test configured actor id survives a run.
    """
    engine = _engine()
    actor = RecordingActor(DataActorConfig(actor_id=ActorId("MY-ACTOR-001")))

    try:
        engine.add_actor(actor)
        engine.add_data(_quotes())
        engine.run()

        assert actor.actor_id == ActorId("MY-ACTOR-001")
        assert actor.actor_id_while_running == "MY-ACTOR-001"
        assert actor.quotes == QUOTE_COUNT
    finally:
        engine.dispose()


def test_overridden_strategy_trades_under_a_class_derived_id() -> None:
    """
    Test overridden strategy trades under a class derived id.
    """
    engine = _engine()
    strategy = CountingStrategy()

    try:
        engine.add_strategy(strategy)
        engine.add_data(_quotes())
        engine.run()

        assert strategy.strategy_id == StrategyId("CountingStrategy-000")
        assert strategy.strategy_id_while_running == "CountingStrategy-000"
        assert strategy.started == 1
        assert strategy.stopped == 1
        assert strategy.quotes == QUOTE_COUNT
        assert strategy.submitted == [
            ClientOrderId("O-20200913-122640-001-000-1"),
            ClientOrderId("O-20200913-122641-001-000-2"),
        ]
        assert strategy.filled == strategy.submitted

        orders = engine.cache.orders(strategy_id=strategy.strategy_id)
        assert [order.client_order_id for order in orders] == strategy.submitted
        assert {order.strategy_id for order in orders} == {strategy.strategy_id}

        positions = engine.cache.positions(strategy_id=strategy.strategy_id)
        assert len(positions) == 1
        assert positions[0].strategy_id == strategy.strategy_id
    finally:
        engine.dispose()


def test_strategy_order_id_tag_alone_names_the_strategy_in_a_run() -> None:
    """
    Test strategy order id tag alone names the strategy in a run.
    """
    engine = _engine()
    strategy = TaggedStrategy(StrategyConfig(order_id_tag="007"))

    try:
        engine.add_strategy(strategy)
        engine.add_data(_quotes())
        engine.run()

        assert strategy.strategy_id == StrategyId("TaggedStrategy-007")
        assert strategy.strategy_id_while_running == "TaggedStrategy-007"
        assert strategy.submitted == [ClientOrderId("O-20200913-122640-001-007-1")]
        assert strategy.filled == strategy.submitted
    finally:
        engine.dispose()


def test_configured_strategy_id_survives_a_run() -> None:
    """
    Test configured strategy id survives a run.
    """
    engine = _engine()
    strategy = ConfiguredStrategy(StrategyConfig(strategy_id=StrategyId("MINE-042")))

    try:
        engine.add_strategy(strategy)
        engine.add_data(_quotes())
        engine.run()

        assert strategy.strategy_id == StrategyId("MINE-042")
        assert strategy.strategy_id_while_running == "MINE-042"
        assert strategy.submitted == [ClientOrderId("O-20200913-122640-001-042-1")]
        assert strategy.filled == strategy.submitted
    finally:
        engine.dispose()


def test_instances_of_one_strategy_class_keep_separate_order_ids_and_events() -> None:
    """
    Test instances of one strategy class keep separate order ids and events.
    """
    engine = _engine()
    first = RecordingStrategy()
    second = SecondRecordingStrategy()
    third = RecordingStrategy()

    try:
        engine.add_strategy(first)
        engine.add_strategy(second)
        engine.add_strategy(third)
        engine.add_data(_quotes())
        engine.run()

        assert first.strategy_id == StrategyId("RecordingStrategy-000")
        assert second.strategy_id == StrategyId("SecondRecordingStrategy-001")
        assert third.strategy_id == StrategyId("RecordingStrategy-002")

        # The order ID tag exists to keep client order IDs unique across strategies
        assert first.submitted == [ClientOrderId("O-20200913-122640-001-000-1")]
        assert second.submitted == [ClientOrderId("O-20200913-122640-001-001-1")]
        assert third.submitted == [ClientOrderId("O-20200913-122640-001-002-1")]

        submitted = first.submitted + second.submitted + third.submitted
        assert len(set(submitted)) == len(submitted)

        # Each strategy sees only its own fills
        assert first.filled == first.submitted
        assert second.filled == second.submitted
        assert third.filled == third.submitted

        # And the cache attributes each order to the strategy that sent it
        for strategy in (first, second, third):
            orders = engine.cache.orders(strategy_id=strategy.strategy_id)
            assert [order.client_order_id for order in orders] == strategy.submitted
    finally:
        engine.dispose()


def test_actors_and_strategies_coexist_under_derived_ids() -> None:
    """
    Test actors and strategies coexist under derived ids.
    """
    engine = _engine()
    actor = RecordingActor()
    strategy = RecordingStrategy()

    try:
        engine.add_actor(actor)
        engine.add_strategy(strategy)
        engine.add_data(_quotes())
        engine.run()

        assert actor.actor_id == ActorId("RecordingActor")
        assert strategy.strategy_id == StrategyId("RecordingStrategy-000")
        assert actor.quotes == QUOTE_COUNT
        assert strategy.quotes == QUOTE_COUNT
        assert strategy.filled == strategy.submitted
    finally:
        engine.dispose()
