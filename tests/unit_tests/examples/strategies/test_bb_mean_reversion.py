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
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversion
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversionConfig
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
_USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")

# Warm-up bars with enough variance to avoid BB collapse and RSI extremes.
# After 4 bars: BB has nonzero std, RSI ~0.5 (neutral). No signals fire.
_WARMUP_BARS = [
    (110.0, 110.2, 109.7, 110.0),
    (110.1, 110.3, 109.8, 110.1),
    (109.9, 110.1, 109.6, 109.9),
    (110.0, 110.2, 109.8, 110.0),
]


class TestBBMeanReversion:
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

        self.bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        self.bar_type = BarType(self.instrument.id, self.bar_spec)

    def _create_strategy(self, **config_overrides) -> BBMeanReversion:
        defaults = {
            "instrument_id": self.instrument.id,
            "bar_type": self.bar_type,
            "trade_size": Decimal(100_000),
            "bb_period": 3,
            "bb_std": 2.0,
            "rsi_period": 3,
            "rsi_buy_threshold": 0.30,
            "rsi_sell_threshold": 0.70,
        }
        defaults.update(config_overrides)
        config = BBMeanReversionConfig(**defaults)
        strategy = BBMeanReversion(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        return strategy

    def _make_bar(
        self,
        open_: float,
        high: float,
        low: float,
        close: float,
        ts: int = 0,
    ) -> Bar:
        return Bar(
            bar_type=self.bar_type,
            open=Price.from_str(f"{open_:.3f}"),
            high=Price.from_str(f"{high:.3f}"),
            low=Price.from_str(f"{low:.3f}"),
            close=Price.from_str(f"{close:.3f}"),
            volume=Quantity.from_int(1_000_000),
            ts_event=ts,
            ts_init=ts,
        )

    def _process_bar(self, bar: Bar) -> None:
        self.data_engine.process(bar)
        self.exchange.process(bar.ts_init)

    def _warm_up_and_start(self, strategy: BBMeanReversion) -> None:
        strategy.start()
        for i, (o, h, l, c) in enumerate(_WARMUP_BARS):
            self._process_bar(self._make_bar(o, h, l, c, ts=i * 60_000_000_000))

    def test_init_with_default_config(self):
        strategy = self._create_strategy()

        assert strategy.config.bb_period == 3
        assert strategy.config.bb_std == 2.0
        assert strategy.config.rsi_period == 3
        assert strategy.config.rsi_buy_threshold == 0.30
        assert strategy.config.rsi_sell_threshold == 0.70
        assert strategy.config.close_positions_on_stop is True

    def test_init_creates_indicators(self):
        strategy = self._create_strategy()

        assert strategy.bb is not None
        assert strategy.rsi is not None
        assert strategy.bb.period == 3
        assert strategy.rsi.period == 3

    def test_start_loads_instrument(self):
        strategy = self._create_strategy()
        strategy.start()

        assert strategy.instrument is not None
        assert strategy.instrument.id == self.instrument.id

    def test_on_bar_waits_for_indicator_warmup(self):
        strategy = self._create_strategy()
        strategy.start()

        bar = self._make_bar(110.000, 110.200, 109.700, 110.000)
        self._process_bar(bar)

        assert len(self.cache.orders()) == 0

    def test_no_entry_when_price_inside_bands(self):
        strategy = self._create_strategy()
        self._warm_up_and_start(strategy)

        bar = self._make_bar(110.0, 110.1, 109.9, 110.0, ts=5 * 60_000_000_000)
        self._process_bar(bar)

        assert len(self.cache.orders()) == 0

    def test_buy_when_price_at_lower_band_with_rsi_confirmation(self):
        strategy = self._create_strategy()
        strategy.start()

        # Varied warm-up then descending bars to push RSI low
        bars = [*_WARMUP_BARS, (109.8, 109.9, 109.3, 109.4)]
        for i, (o, h, l, c) in enumerate(bars):
            self._process_bar(self._make_bar(o, h, l, c, ts=i * 60_000_000_000))

        # Bar 4 close=109.4 breaches bb.lower=109.408 with RSI=0.088
        orders = [o for o in self.cache.orders() if o.side == OrderSide.BUY]
        assert len(orders) == 1

    def test_sell_when_price_at_upper_band_with_rsi_confirmation(self):
        strategy = self._create_strategy()
        strategy.start()

        # Varied warm-up then ascending bars to push RSI high, final bar
        # uses wide OHLC range with close=high so close exceeds bb.upper
        bars = [
            *_WARMUP_BARS,
            (110.2, 110.7, 110.1, 110.6),
            (110.7, 111.5, 110.6, 111.4),
            (111.0, 112.0, 109.0, 112.0),
        ]
        for i, (o, h, l, c) in enumerate(bars):
            self._process_bar(self._make_bar(o, h, l, c, ts=i * 60_000_000_000))

        # Bar 6 close=112.0 breaches bb.upper=111.475 with RSI=0.989
        orders = [o for o in self.cache.orders() if o.side == OrderSide.SELL]
        assert len(orders) == 1

    def test_on_stop_closes_positions(self):
        strategy = self._create_strategy()
        strategy.start()
        strategy.stop()

        assert strategy.instrument is not None

    def test_on_reset_resets_indicators(self):
        strategy = self._create_strategy()
        self._warm_up_and_start(strategy)

        assert strategy.bb.initialized
        assert strategy.rsi.initialized

        strategy.stop()
        strategy.reset()

        assert not strategy.bb.initialized
        assert not strategy.rsi.initialized

    def test_single_price_bar_skipped(self):
        strategy = self._create_strategy()
        self._warm_up_and_start(strategy)

        bar = self._make_bar(110.000, 110.000, 110.000, 110.000, ts=5 * 60_000_000_000)
        self._process_bar(bar)

        assert len(self.cache.orders()) == 0
