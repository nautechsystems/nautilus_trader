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
Regression tests for v2 bar-aggregation delivery in the backtest.

Cover three fixes:
- Composite (bar-from-bar) aggregation delivers aggregated bars to subscribers.
- Internal aggregation includes the first underlying tick.
- A quote fed to an indicator with a `Last` price type does not crash the run.

"""

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.model import AccountType
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Venue
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from tests.providers import TestInstrumentProvider


_START = 1_577_836_800_000_000_000


def _engine(instrument) -> BacktestEngine:
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )
    engine.add_instrument(instrument)
    return engine


def _one_min_bars(instrument, bar_type: BarType, count: int) -> list[Bar]:
    pp = instrument.price_precision
    bars = []

    for i in range(count):
        close = Decimal("0.70000") + Decimal(i) * Decimal("0.00010")
        bars.append(
            Bar(
                bar_type=bar_type,
                open=Price.from_decimal_dp(close, pp),
                high=Price.from_decimal_dp(close + Decimal("0.00050"), pp),
                low=Price.from_decimal_dp(close - Decimal("0.00050"), pp),
                close=Price.from_decimal_dp(close, pp),
                volume=Quantity.from_int(1_000_000),
                ts_event=_START + i * 60_000_000_000,
                ts_init=_START + i * 60_000_000_000,
            ),
        )
    return bars


def _quotes(instrument, count: int) -> list[QuoteTick]:
    pp = instrument.price_precision
    quotes = []

    for i in range(count):
        bid = Decimal("0.70000") + Decimal(i) * Decimal("0.00001")
        quotes.append(
            QuoteTick(
                instrument.id,
                Price.from_decimal_dp(bid, pp),
                Price.from_decimal_dp(bid + Decimal("0.00002"), pp),
                Quantity.from_int(1_000_000),
                Quantity.from_int(1_000_000),
                _START + i * 1_000_000_000,
                _START + i * 1_000_000_000,
            ),
        )
    return quotes


class _BarCollector(Strategy):
    def __init__(self, config=None):
        super().__init__(config)

    def configure(self, bar_type: str) -> None:
        self.bar_type = BarType.from_str(bar_type)
        self.bars: list[Bar] = []

    def on_start(self):
        self.subscribe_bars(self.bar_type)

    def on_bar(self, bar: Bar):
        self.bars.append(bar)


def test_composite_bar_aggregation_delivers_to_subscriber():
    # Delivered aggregated bars carry the standard bar type (no `@source` suffix)
    instrument = TestInstrumentProvider.audusd_sim()
    engine = _engine(instrument)
    source = BarType.from_str(f"{instrument.id}-1-MINUTE-BID-EXTERNAL")
    engine.add_data(_one_min_bars(instrument, source, count=60))

    strategy = _BarCollector(StrategyConfig())
    strategy.configure(f"{instrument.id}-5-MINUTE-BID-INTERNAL@1-MINUTE-EXTERNAL")
    engine.add_strategy(strategy)
    engine.run()

    standard = BarType.from_str(f"{instrument.id}-5-MINUTE-BID-INTERNAL")
    assert len(strategy.bars) == 12
    assert all(bar.bar_type == standard for bar in strategy.bars)
    engine.dispose()


def test_internal_tick_aggregation_includes_first_tick():
    # The first underlying tick is not dropped: 10 quotes make 2 bars, bar 1 opens at tick 1
    instrument = TestInstrumentProvider.audusd_sim()
    engine = _engine(instrument)
    engine.add_data(_quotes(instrument, count=10))

    strategy = _BarCollector(StrategyConfig())
    strategy.configure(f"{instrument.id}-5-TICK-BID-INTERNAL")
    engine.add_strategy(strategy)
    engine.run()

    assert len(strategy.bars) == 2
    assert strategy.bars[0].open == Price.from_str("0.70000")  # first tick, not the second
    engine.dispose()


class _QuoteEmaStrategy(Strategy):
    def __init__(self, config=None):
        super().__init__(config)

    def configure(self, instrument_id) -> None:
        self.iid = instrument_id
        self.ema = ExponentialMovingAverage(10)  # default price type is Last

    def on_start(self):
        self.register_indicator_for_quote_ticks(self.iid, self.ema)
        self.subscribe_quotes(self.iid)


def test_indicator_registered_for_quotes_with_last_price_does_not_crash():
    # A `Last`-price indicator fed quotes must not panic: the run completes, indicator uninitialized
    instrument = TestInstrumentProvider.audusd_sim()
    engine = _engine(instrument)
    engine.add_data(_quotes(instrument, count=20))

    strategy = _QuoteEmaStrategy(StrategyConfig())
    strategy.configure(instrument.id)
    engine.add_strategy(strategy)
    engine.run()  # must not raise

    result = engine.get_result()
    assert result.iterations == 20
    assert not strategy.ema.initialized
    engine.dispose()
