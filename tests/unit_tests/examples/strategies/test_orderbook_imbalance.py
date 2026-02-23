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

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
_USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestOrderBookImbalance:
    def setup(self):
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exchange = SimulatedExchange(
            venue=SIM,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={},
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            book_type=BookType.L2_MBP,
            latency_model=LatencyModel(0),
        )

        self.instrument = _USDJPY_SIM
        self.exchange.add_instrument(self.instrument)

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.cache.add_instrument(self.instrument)
        self.exchange.register_client(self.exec_client)
        self.data_engine.register_client(self.data_client)
        self.exec_engine.register_client(self.exec_client)
        self.exchange.reset()

        self.data_engine.start()
        self.exec_engine.start()

    def _create_strategy(self, **config_overrides) -> OrderBookImbalance:
        defaults = {
            "instrument_id": self.instrument.id,
            "max_trade_size": Decimal(10000),
            "trigger_min_size": 50.0,
            "trigger_imbalance_ratio": 0.20,
            "min_seconds_between_triggers": 1.0,
        }
        defaults.update(config_overrides)
        config = OrderBookImbalanceConfig(**defaults)
        strategy = OrderBookImbalance(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        return strategy

    def _set_book(
        self,
        bid_size: float,
        ask_size: float,
        bid_price: float = 110.0,
        ask_price: float = 110.01,
    ) -> None:
        ts = self.clock.timestamp_ns()
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            bid_levels=1,
            ask_levels=1,
            ts_event=ts,
            ts_init=ts,
        )
        self.exchange.process_order_book_deltas(snapshot)
        book = self.cache.order_book(self.instrument.id)
        if book is not None:
            book.apply_deltas(snapshot)

    def _process_book(
        self,
        bid_size: float,
        ask_size: float,
        bid_price: float = 110.0,
        ask_price: float = 110.01,
    ) -> None:
        ts = self.clock.timestamp_ns()
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            bid_levels=1,
            ask_levels=1,
            ts_event=ts,
            ts_init=ts,
        )
        self.exchange.process_order_book_deltas(snapshot)
        self.data_engine.process(snapshot)
        self.exchange.process(ts)

    def test_init_with_default_config(self):
        strategy = self._create_strategy()

        assert strategy.config.trigger_min_size == 50.0
        assert strategy.config.trigger_imbalance_ratio == 0.20
        assert strategy.config.min_seconds_between_triggers == 1.0
        assert strategy.config.book_type == "L2_MBP"
        assert strategy.config.use_quote_ticks is False
        assert strategy.config.dry_run is False

    def test_start_loads_instrument(self):
        strategy = self._create_strategy()
        strategy.start()

        assert strategy.instrument is not None
        assert strategy.instrument.id == self.instrument.id

    def test_trigger_buy_on_bid_imbalance(self):
        strategy = self._create_strategy()
        strategy.start()
        self._process_book(bid_size=200, ask_size=10)

        orders = self.cache.orders()
        assert len(orders) == 1
        assert orders[0].side == OrderSide.BUY
        assert orders[0].price == self.instrument.make_price(110.01)
        assert orders[0].quantity == self.instrument.make_qty(10)
        assert orders[0].time_in_force == TimeInForce.FOK

    def test_trigger_sell_on_ask_imbalance(self):
        strategy = self._create_strategy()
        strategy.start()
        self._process_book(bid_size=10, ask_size=200)

        orders = self.cache.orders()
        assert len(orders) == 1
        assert orders[0].side == OrderSide.SELL
        assert orders[0].price == self.instrument.make_price(110.0)
        assert orders[0].quantity == self.instrument.make_qty(10)
        assert orders[0].time_in_force == TimeInForce.FOK

    def test_no_trigger_when_ratio_above_threshold(self):
        strategy = self._create_strategy()
        strategy.start()

        # Balanced book: ratio = 100/100 = 1.0 >= 0.20
        self._process_book(bid_size=100, ask_size=100)

        assert len(self.cache.orders()) == 0

    def test_no_trigger_when_larger_side_below_min_size(self):
        strategy = self._create_strategy(trigger_min_size=500.0)
        strategy.start()

        # Imbalanced but larger side (200) < trigger_min_size (500)
        self._process_book(bid_size=200, ask_size=10)

        assert len(self.cache.orders()) == 0

    def test_cooldown_prevents_second_trigger(self):
        strategy = self._create_strategy(min_seconds_between_triggers=10.0)
        strategy.start()

        self._process_book(bid_size=200, ask_size=10)
        assert len(self.cache.orders()) == 1

        # 5s < 10s cooldown
        self.clock.set_time(5_000_000_000)
        self._set_book(bid_size=200, ask_size=10)
        strategy.check_trigger()

        assert len(self.cache.orders()) == 1

    def test_trigger_after_cooldown_expires(self):
        strategy = self._create_strategy(min_seconds_between_triggers=1.0)
        strategy.start()

        self._process_book(bid_size=200, ask_size=10)
        assert len(self.cache.orders()) == 1

        # 2s > 1s cooldown
        self.clock.set_time(2_000_000_000)
        self._set_book(bid_size=200, ask_size=10)
        strategy.check_trigger()

        assert len(self.cache.orders()) == 2

    def test_max_trade_size_clamps_quantity(self):
        strategy = self._create_strategy(max_trade_size=Decimal(5))
        strategy.start()

        # Bid imbalance: level_size = ask_size = 10, but max_trade_size = 5
        self._process_book(bid_size=200, ask_size=10)

        orders = self.cache.orders()
        assert len(orders) == 1
        assert orders[0].quantity == self.instrument.make_qty(5)

    def test_dry_run_does_not_submit_order(self):
        strategy = self._create_strategy(dry_run=True)
        strategy.start()
        self._process_book(bid_size=200, ask_size=10)

        assert len(self.cache.orders()) == 0

    def test_dry_run_still_sets_cooldown(self):
        strategy = self._create_strategy(dry_run=True)
        strategy.start()
        self._process_book(bid_size=200, ask_size=10)

        assert strategy._last_trigger_timestamp is not None

    def test_on_stop_cancels_and_closes(self):
        strategy = self._create_strategy()
        strategy.start()
        self._process_book(bid_size=200, ask_size=10)
        assert len(self.cache.orders()) == 1

        strategy.stop()

        assert strategy.instrument is not None

    def test_on_reset_clears_state(self):
        strategy = self._create_strategy()
        strategy.start()
        self._process_book(bid_size=200, ask_size=10)
        assert strategy._last_trigger_timestamp is not None

        strategy.stop()
        strategy.reset()

        assert strategy._last_trigger_timestamp is None
        assert strategy._book is None
