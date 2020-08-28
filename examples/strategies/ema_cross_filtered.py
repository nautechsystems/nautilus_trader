# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime
from datetime import timedelta

import pytz

from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.uuid import TestUUIDFactory
from nautilus_trader.common.timer import TimeEvent
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import OrderPurpose
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.analyzers import SpreadAnalyzer
from nautilus_trader.trading.filters import EconomicNewsEventFilter
from nautilus_trader.trading.filters import ForexSession
from nautilus_trader.trading.filters import ForexSessionFilter
from nautilus_trader.trading.sizing import FixedRiskSizer
from nautilus_trader.trading.strategy import TradingStrategy

UPDATE_SESSIONS = 'UPDATE-SESSIONS'
UPDATE_NEWS = 'UPDATE-NEWS'
NEWS_FLATTEN = 'NEWS-FLATTEN'
DONE_FOR_DAY = 'DONE-FOR-DAY'


class EMACrossFiltered(TradingStrategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then a STOP entry bracket order is
    placed for that direction with a trailing stop and profit target at 1R risk.
    """

    def __init__(self,
                 symbol: Symbol,
                 bar_spec: BarSpecification,
                 risk_bp: float=10.0,
                 fast_ema: int=10,
                 slow_ema: int=20,
                 atr_period: int=20,
                 sl_atr_multiple: float=2.0,
                 news_currencies: list=None,
                 news_impacts: list=None,
                 extra_id_tag: str=""):
        """
        Initialize a new instance of the EMACrossPy class.

        :param symbol: The symbol for the strategy.
        :param bar_spec: The bar specification for the strategy.
        :param risk_bp: The risk per trade (basis points).
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param atr_period: The ATR period.
        :param sl_atr_multiple: The ATR multiple for stop-loss prices.
        :param extra_id_tag: An optional extra tag to append to order ids.
        """
        clock = TestClock()
        super().__init__(
            clock=clock,
            uuid_factory=TestUUIDFactory(),
            logger=TestLogger(clock),
            order_id_tag=symbol.code.replace('/', '') + extra_id_tag)

        if news_currencies is None:
            news_currencies = []
        if news_impacts is None:
            news_impacts = []

        # Custom strategy variables (all optional)
        self.symbol = symbol
        self.bar_type = BarType(symbol, bar_spec)
        self.precision = 5          # dummy initial value for FX
        self.risk_bp = risk_bp
        self.entry_buffer = 0.0     # instrument.tick_size
        self.SL_buffer = 0.0        # instrument.tick_size * 10
        self.SL_atr_multiple = sl_atr_multiple

        self.spread_analyzer = SpreadAnalyzer(self.symbol, 100)
        self.position_sizer = None  # initialized in on_start()
        self.quote_currency = None  # initialized in on_start()

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)
        self.atr = AverageTrueRange(atr_period)

        # Create trading session filter
        self.session_filter = ForexSessionFilter()
        self.session_start_zone = ForexSession.TOKYO
        self.session_end_zone = ForexSession.NEW_YORK
        self.session_next_start = None
        self.session_next_end = None
        self.trading_end_buffer = timedelta(minutes=10)
        self.trading_start = None
        self.trading_end = None

        # Create news event filter
        self.news_filter = EconomicNewsEventFilter(currencies=news_currencies, impacts=news_impacts)
        self.news_event_next = None
        self.news_buffer_high_before = timedelta(minutes=10)
        self.news_buffer_high_after = timedelta(minutes=20)
        self.news_buffer_medium_before = timedelta(minutes=5)
        self.news_buffer_medium_after = timedelta(minutes=10)
        self.trading_pause_start = None
        self.trading_pause_end = None

    def on_start(self):
        """
        Actions to be performed on strategy start.
        """
        instrument = self.get_instrument(self.symbol)

        self.precision = instrument.price_precision
        self.entry_buffer = instrument.tick_size.as_double() * 3.0
        self.SL_buffer = instrument.tick_size * 10.0
        self.position_sizer = FixedRiskSizer(instrument)
        self.quote_currency = instrument.quote_currency

        # Register the indicators for updating
        self.register_indicator(
            data_source=self.bar_type,
            indicator=self.fast_ema,
            update_method=self.fast_ema.update)
        self.register_indicator(
            data_source=self.bar_type,
            indicator=self.slow_ema,
            update_method=self.slow_ema.update)
        self.register_indicator(
            data_source=self.bar_type,
            indicator=self.atr,
            update_method=self.atr.update)

        # Set trading sessions
        self._update_session_times()

        # Set next news event
        self._update_news_event()

        # Get historical data
        self.get_quote_ticks(self.symbol)
        self.get_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_instrument(self.symbol)
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.symbol)

    def on_quote_tick(self, tick: QuoteTick):
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        :param tick: The quote tick received.
        """
        # self.log.info(f"Received Tick({tick})")  # For debugging
        self.spread_analyzer.update(tick)

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        Actions to be performed when the strategy is running and receives a bar.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        self.log.info(f"Received {bar_type} Bar({bar})")  # For debugging

        time_now = self.clock.time_now()

        if time_now >= self.trading_end:
            self.log.info(f"Trading ended at {self.trading_end}. "
                          f"{self.session_end_zone.name} session close at {self.session_next_end}.")
            return

        if time_now < self.trading_start:
            self.log.info(f"Trading start at {self.trading_start}. "
                          f"{self.session_start_zone.name} session open at {self.session_next_start}.")
            return

        # Check news events
        if time_now >= self.trading_pause_start:
            if time_now < self.trading_pause_end:
                self.log.info(f"Trading paused until {self.trading_pause_end} "
                              f"for news event {self.news_event_next.name} "
                              f"affecting {self.news_event_next.currency} "
                              f"with expected {self.news_event_next.impact} impact "
                              f"at {self.news_event_next.timestamp}")
                return  # Waiting for end of pause period

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(f"Waiting for indicators to warm up "
                          f"[{self.bar_count(self.bar_type)}]...")
            return  # Wait for indicators to warm up...

        # Check if tick data available
        if not self.has_quote_ticks(self.symbol):
            self.log.info(f"Waiting for {self.symbol.value} ticks...")
            return  # Wait for ticks...

        # Check average spread
        average_spread = self.spread_analyzer.average_spread
        if average_spread == 0.0:
            self.log.warning(f"average_spread == {average_spread} (not initialized).")
            return  # Protect divide by zero

        spread_buffer = max(average_spread, self.spread_analyzer.current_spread)
        sl_buffer = self.atr.value * self.SL_atr_multiple

        # Check liquidity
        liquidity_ratio = self.atr.value / average_spread
        if liquidity_ratio >= 2.0:
            self._check_signal(bar, sl_buffer, spread_buffer)
        else:
            self.log.info(f"liquidity_ratio == {liquidity_ratio} (low liquidity).")

        self._check_trailing_stops(bar, sl_buffer, spread_buffer)

    def on_data(self, data):
        """
        Actions to be performed when the strategy is running and receives a data object.

        :param data: The data object received.
        """
        pass

    def on_event(self, event):
        """
        Actions to be performed when the strategy is running and receives an event.

        :param event: The event received.
        """
        if isinstance(event, TimeEvent):
            if event.name.startswith(DONE_FOR_DAY):
                self._done_for_day()
                return
            if event.name.startswith(NEWS_FLATTEN):
                self._news_flatten()
                return
            if event.name.startswith(UPDATE_SESSIONS):
                self._update_session_times()
                return
            if event.name.startswith(UPDATE_NEWS):
                self._update_news_event()
                return
            self.log.warning(f"Received unknown time event {event}.")

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        # Put custom code to be run on strategy stop here (or pass)
        pass

    def on_reset(self):
        """
        Actions to be performed when the strategy is reset.
        """
        # Trading session times
        self.session_next_start = None
        self.session_next_end = None
        self.trading_start = None
        self.trading_end = None

        # News event times
        self.news_event_next = None
        self.trading_pause_start = None
        self.trading_pause_end = None

    def on_save(self) -> {}:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Note: 'OrderIdCount' and 'PositionIdCount' are reserved keys for
        the returned state dictionary.
        """
        return {}

    def on_load(self, state: {}):
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.
        """
        pass

    def on_dispose(self):
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.
        """
        # Put custom code to be run on a strategy disposal here (or pass)
        self.unsubscribe_instrument(self.symbol)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_quote_ticks(self.symbol)

    def _check_signal(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        if self.count_orders_working() == 0 and self.is_flat():  # No active or pending positions
            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                self._enter_long(bar, sl_buffer, spread_buffer)
            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                self._enter_short(bar, sl_buffer, spread_buffer)

    def _enter_long(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        price_entry = Price(bar.high.as_double() + self.entry_buffer + spread_buffer, self.precision)
        price_stop_loss = Price(bar.low.as_double() - sl_buffer, self.precision)

        risk = price_entry.as_double() - price_stop_loss.as_double()
        price_take_profit = Price(price_entry.as_double() + risk, self.precision)

        # Calculate exchange rate
        exchange_rate = 0.0
        try:
            exchange_rate = self.get_exchange_rate_for_account(
                quote_currency=self.quote_currency,
                price_type=PriceType.ASK)
        except ValueError as ex:
            self.log.error(ex)

        if exchange_rate == 0.0:
            return

        position_size = self.position_sizer.calculate(
            equity=self.account().free_equity,
            risk_bp=self.risk_bp,
            entry=price_entry,
            stop_loss=price_stop_loss,
            exchange_rate=exchange_rate,
            commission_rate_bp=0.15,
            hard_limit=20000000,
            units=1,
            unit_batch_size=10000)
        if position_size > 0:
            bracket_order = self.order_factory.bracket_stop(
                symbol=self.symbol,
                order_side=OrderSide.BUY,
                quantity=position_size,
                entry=price_entry,
                stop_loss=price_stop_loss,
                take_profit=price_take_profit,
                time_in_force=TimeInForce.GTD,
                expire_time=bar.timestamp + timedelta(minutes=1))

            self.submit_bracket_order(bracket_order, self.position_id_generator.generate())
        else:
            self.log.info("Insufficient equity for BUY signal.")

    def _enter_short(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        price_entry = Price(bar.low.as_double() - self.entry_buffer, self.precision)
        price_stop_loss = Price(bar.high.as_double() + sl_buffer + spread_buffer, self.precision)

        risk = price_stop_loss.as_double() - price_entry.as_double()
        price_take_profit = Price(price_entry.as_double() - risk, self.precision)

        # Calculate exchange rate
        exchange_rate = 0.0
        try:
            exchange_rate = self.get_exchange_rate_for_account(
                quote_currency=self.quote_currency,
                price_type=PriceType.BID)
        except ValueError as ex:
            self.log.error(ex)

        if exchange_rate == 0.0:
            return

        position_size = self.position_sizer.calculate(
            equity=self.account().free_equity,
            risk_bp=self.risk_bp,
            entry=price_entry,
            stop_loss=price_stop_loss,
            exchange_rate=exchange_rate,
            commission_rate_bp=0.15,
            hard_limit=20000000,
            units=1,
            unit_batch_size=10000)

        if position_size > 0:  # Sufficient equity for a position
            bracket_order = self.order_factory.bracket_stop(
                symbol=self.symbol,
                order_side=OrderSide.SELL,
                quantity=position_size,
                entry=price_entry,
                stop_loss=price_stop_loss,
                take_profit=price_take_profit,
                time_in_force=TimeInForce.GTD,
                expire_time=bar.timestamp + timedelta(minutes=1))

            self.submit_bracket_order(bracket_order, self.position_id_generator.generate())
        else:
            self.log.info("Insufficient equity for SELL signal.")

    def _check_trailing_stops(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        for working_order in self.orders_working().values():
            if working_order.purpose == OrderPurpose.STOP_LOSS:
                # SELL SIDE ORDERS
                if working_order.is_sell:
                    temp_price = Price(bar.low.as_double() - sl_buffer, self.precision)
                    if temp_price.gt(working_order.price):
                        self.modify_order(working_order, working_order.quantity, temp_price)
                # BUY SIDE ORDERS
                elif working_order.is_buy:
                    temp_price = Price(bar.high.as_double() + sl_buffer + spread_buffer, self.precision)
                    if temp_price.lt(working_order.price):
                        self.modify_order(working_order, working_order.quantity, temp_price)

    def _update_session_times(self):
        time_now = self.clock.time_now()

        # Set trading sessions
        self.session_next_start = self.session_filter.next_start(self.session_start_zone, time_now)
        self.session_next_end = self.session_filter.next_end(self.session_end_zone, time_now)
        if self.session_next_start > time_now:
            # If in the middle of a session then
            self.session_next_start = self.session_filter.prev_start(self.session_start_zone, time_now)

        tactical_start = datetime(
            time_now.year,
            time_now.month,
            time_now.day,
            time_now.hour,
            time_now.minute,
            tzinfo=pytz.utc) + timedelta(minutes=2)

        self.trading_start = max(self.session_next_start, tactical_start)
        self.trading_end = self.session_next_end - self.trading_end_buffer
        self.log.info(f"Set next {self.session_start_zone.name} session open to {self.session_next_start}")
        self.log.info(f"Set next {self.session_end_zone.name} session close to {self.session_next_end}")
        self.log.info(f"Set trading start to {self.trading_start}")
        self.log.info(f"Set trading end to {self.trading_end}")

        # Set session update event
        alert_label = f"-{time_now.date()}"
        self.clock.set_time_alert(DONE_FOR_DAY + alert_label, self.trading_end)
        self.clock.set_time_alert(UPDATE_SESSIONS + alert_label, self.session_next_end + timedelta(seconds=1))

    def _done_for_day(self):
        self.log.info("Done for day - commencing trading end flatten...")
        self.flatten_all_positions(order_label=DONE_FOR_DAY)
        self.cancel_all_orders(cancel_reason=DONE_FOR_DAY)
        self.log.info("Done for day...")

    def _update_news_event(self):
        time_now = self.clock.time_now()

        # Set next news event
        self.news_event_next = self.news_filter.next_event(time_now)
        if self.news_event_next.impact == 'HIGH':
            self.trading_pause_start = self.news_event_next.timestamp - self.news_buffer_high_before
            self.trading_pause_end = self.news_event_next.timestamp + self.news_buffer_high_after
        elif self.news_event_next.impact == 'MEDIUM':
            self.trading_pause_start = self.news_event_next.timestamp - self.news_buffer_medium_before
            self.trading_pause_end = self.news_event_next.timestamp + self.news_buffer_medium_after

        self.log.info(f"Set next news event {self.news_event_next.name} "
                      f"affecting {self.news_event_next.currency} "
                      f"with expected {self.news_event_next.impact} impact "
                      f"at {self.news_event_next.timestamp}")
        self.log.info(f"Set next trading pause start to {self.trading_pause_start}")
        self.log.info(f"Set next trading pause end to {self.trading_pause_end}")

        if time_now >= self.trading_pause_start:
            return  # Already in pause window

        # Set news update event
        news_time = self.news_event_next.timestamp
        news_name = self.news_event_next.name.replace(' ', '')
        alert_label = f"-{news_time.date()}-{news_time.hour:02d}{news_time.minute:02d}-{news_name}"
        self.clock.set_time_alert(UPDATE_NEWS + alert_label, self.trading_pause_end)
        self.clock.set_time_alert(NEWS_FLATTEN + alert_label, self.trading_pause_start + timedelta(seconds=1))

    def _news_flatten(self):
        self.log.info("Within trading pause window - commencing news flatten...")
        self.flatten_all_positions(order_label=NEWS_FLATTEN)
        self.cancel_all_orders(cancel_reason=NEWS_FLATTEN)
        self.log.info("Trading paused...")
