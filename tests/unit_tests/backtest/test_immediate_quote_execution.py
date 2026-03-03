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

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import BestPriceFillModel
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD", Venue("SIM"))


class ImmediateOrderStrategy(Strategy):
    """
    Strategy that submits a limit order on the first quote tick.
    """

    def __init__(self):
        super().__init__()
        self.instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        self.quote_count = 0
        self.order_submitted = False
        self.fill_timestamps = []
        self.submit_timestamps = []

    def on_start(self):
        self.subscribe_quote_ticks(self.instrument_id)

    def on_quote_tick(self, tick: QuoteTick):
        self.quote_count += 1

        if not self.order_submitted:
            instrument = self.cache.instrument(self.instrument_id)
            mid = (tick.bid_price.as_double() + tick.ask_price.as_double()) / 2.0
            limit_price = instrument.next_ask_price(mid, num_ticks=0)

            order = self.order_factory.limit(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(100_000),
                price=limit_price,
            )
            self.submit_order(order)
            self.order_submitted = True
            self.submit_timestamps.append(tick.ts_init)

    def on_order_filled(self, event):
        self.fill_timestamps.append(event.ts_init)


class DeltaHedgeOnFillStrategy(Strategy):
    """
    Submits one order on first quote tick; when it fills, submits a second order in
    on_order_filled (e.g. delta hedge).

    Both should be able to fill at the same timestamp via the settle loop in
    _process_and_settle_venues.

    """

    def __init__(self):
        super().__init__()
        self.instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        self.first_order_submitted = False
        self.hedge_submitted = False
        self.fill_timestamps = []

    def on_start(self):
        self.subscribe_quote_ticks(self.instrument_id)

    def on_quote_tick(self, tick: QuoteTick):
        if not self.first_order_submitted:
            instrument = self.cache.instrument(self.instrument_id)
            mid = (tick.bid_price.as_double() + tick.ask_price.as_double()) / 2.0
            limit_price = instrument.next_ask_price(mid, num_ticks=0)

            order = self.order_factory.limit(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(100_000),
                price=limit_price,
            )
            self.submit_order(order)
            self.first_order_submitted = True

    def on_order_filled(self, event):
        self.fill_timestamps.append(event.ts_init)
        if not self.hedge_submitted:
            hedge = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=Quantity.from_int(100_000),
            )
            self.submit_order(hedge)
            self.hedge_submitted = True


class TestImmediateQuoteExecution:
    """
    Test same-timestamp order execution.

    With iterate() now running in _process_and_settle_venues (after strategy callbacks
    and command draining), orders submitted during on_quote_tick can fill on the same
    data point.

    """

    def setup(self):
        self.instrument = AUDUSD_SIM
        self.venue = Venue("SIM")

    def _make_quotes(self, n=2):
        quotes = []
        base_ts = 1_000_000_000_000_000_000
        for i in range(n):
            bid_price = 0.70000 + (i * 0.00001)
            ask_price = bid_price + 0.00002
            quote = QuoteTick(
                instrument_id=self.instrument.id,
                bid_price=Price.from_str(f"{bid_price:.5f}"),
                ask_price=Price.from_str(f"{ask_price:.5f}"),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=base_ts + (i * 60_000_000_000),
                ts_init=base_ts + (i * 60_000_000_000),
            )
            quotes.append(quote)
        return quotes

    def _run_with_message_queue(self, quotes, use_message_queue):
        engine = BacktestEngine(config=BacktestEngineConfig())
        engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=BestPriceFillModel(),
            use_message_queue=use_message_queue,
        )
        engine.add_instrument(self.instrument)
        engine.add_data(quotes)
        strategy = ImmediateOrderStrategy()
        engine.add_strategy(strategy)
        engine.run()
        return strategy

    def test_same_timestamp_fill_with_message_queue(self):
        """
        With use_message_queue=True the command goes through _message_queue, but
        _drain_commands runs before iterate in _process_and_settle_venues, so the order
        still fills on the same quote tick.
        """
        quotes = self._make_quotes()
        strategy = self._run_with_message_queue(quotes, use_message_queue=True)

        orders = list(strategy.cache.orders())
        assert len(orders) == 1
        assert orders[0].status == OrderStatus.FILLED
        assert len(strategy.fill_timestamps) == 1
        assert strategy.fill_timestamps[0] == quotes[0].ts_init

    def test_same_timestamp_fill_for_order_submitted_in_on_order_filled(self):
        """
        Order submitted in on_order_filled (e.g. delta hedge) fills at the same
        timestamp as the fill that triggered it, via the iterate+drain loop in
        _process_and_settle_venues.
        """
        quotes = self._make_quotes(n=3)
        engine = BacktestEngine(config=BacktestEngineConfig())
        engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=BestPriceFillModel(),
            use_message_queue=True,
        )
        engine.add_instrument(self.instrument)
        engine.add_data(quotes)
        strategy = DeltaHedgeOnFillStrategy()
        engine.add_strategy(strategy)
        engine.run()

        orders = list(strategy.cache.orders())
        assert len(orders) == 2
        assert orders[0].status == OrderStatus.FILLED
        assert orders[1].status == OrderStatus.FILLED
        assert len(strategy.fill_timestamps) == 2
        assert strategy.fill_timestamps[0] == quotes[0].ts_init
        assert strategy.fill_timestamps[1] == quotes[0].ts_init


# ---------------------------------------------------------------------------
# Bar execution tests -- limit orders submitted during on_bar should only
# be checked against the close price (not past O/H/L).
# ---------------------------------------------------------------------------

USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY", Venue("SIM"))


class BarLimitOrderStrategy(Strategy):
    """
    Strategy that submits a limit BUY order on the first bar.

    The limit price is configurable so we can test both cases:
      - price above close -> fills immediately via is_limit_marketable
      - price below close -> rests (does not see past O/H/L)

    """

    def __init__(self, limit_price: str):
        super().__init__()
        self.instrument_id = InstrumentId.from_str("USD/JPY.SIM")
        self._limit_price_str = limit_price
        self.bar_count = 0
        self.order_submitted = False
        self.fill_timestamps: list[int] = []

    def on_start(self):
        bar_type = BarType.from_str(f"{self.instrument_id.value}-1-MINUTE-LAST-EXTERNAL")
        self.subscribe_bars(bar_type)

    def on_bar(self, bar: Bar):
        self.bar_count += 1

        if not self.order_submitted:
            order = self.order_factory.limit(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(100_000),
                price=Price.from_str(self._limit_price_str),
            )
            self.submit_order(order)
            self.order_submitted = True

    def on_order_filled(self, event):
        self.fill_timestamps.append(event.ts_init)


class TestBarSameTimestampExecution:
    """
    Test that limit orders submitted during on_bar do NOT execute against past OHLC
    prices.

    The matching engine sets _bar_iterated after the OHLC walk, so
    iterate_matching_engines skips re-iteration. New orders only get the
    is_limit_marketable check (against close state).

    """

    def setup(self):
        self.instrument = USDJPY_SIM
        self.venue = Venue("SIM")

    def _make_bars(self, n=2):
        # Bar: open=90.000, high=90.030, low=89.970, close=90.010
        bar_type = BarType.from_str(f"{self.instrument.id.value}-1-MINUTE-LAST-EXTERNAL")
        bars = []
        base_ts = 1_000_000_000_000_000_000
        for i in range(n):
            bar = Bar(
                bar_type=bar_type,
                open=Price.from_str("90.000"),
                high=Price.from_str("90.030"),
                low=Price.from_str("89.970"),
                close=Price.from_str("90.010"),
                volume=Quantity.from_str("1000000"),
                ts_event=base_ts + (i * 60_000_000_000),
                ts_init=base_ts + (i * 60_000_000_000),
            )
            bars.append(bar)
        return bars

    def _run_bar_strategy(self, bars, limit_price: str):
        engine = BacktestEngine(config=BacktestEngineConfig())
        engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=BestPriceFillModel(),
        )
        engine.add_instrument(self.instrument)
        engine.add_data(bars)
        strategy = BarLimitOrderStrategy(limit_price=limit_price)
        engine.add_strategy(strategy)
        engine.run()
        return strategy

    def test_limit_buy_above_close_fills_on_same_bar(self):
        """
        Limit BUY at 90.020 (above close 90.010): the ask after the bar OHLC walk
        reflects the close.

        is_limit_marketable in process_order sees price >= ask, so the order fills on
        the same bar.

        """
        bars = self._make_bars()
        strategy = self._run_bar_strategy(bars, limit_price="90.020")

        orders = list(strategy.cache.orders())
        assert len(orders) == 1
        assert orders[0].status == OrderStatus.FILLED
        assert len(strategy.fill_timestamps) == 1
        assert strategy.fill_timestamps[0] == bars[0].ts_init

    def test_limit_buy_below_close_rests(self):
        """
        Limit BUY at 89.980 (below close 90.010): even though the bar's low was 89.970
        (which would have filled during the OHLC walk if the order had been present),
        the order was submitted *after* the walk.

        iterate_matching_engines skips bar-iterated engines, so the order rests and
        fills on the next bar's OHLC walk instead.

        """
        bars = self._make_bars()
        strategy = self._run_bar_strategy(bars, limit_price="89.980")

        orders = list(strategy.cache.orders())
        assert len(orders) == 1
        assert orders[0].status == OrderStatus.FILLED
        assert len(strategy.fill_timestamps) == 1
        # Fills on the second bar (next OHLC walk), not the first
        assert strategy.fill_timestamps[0] == bars[1].ts_init
