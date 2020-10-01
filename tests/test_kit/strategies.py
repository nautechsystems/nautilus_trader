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

from datetime import timedelta

from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.indicators.spread_analyzer import SpreadAnalyzer
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.price_type import PriceType
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.sizing import FixedRiskSizer
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import ObjectStorer


class PyStrategy(TradingStrategy):
    """
    A strategy which is empty and does nothing.
    """

    def __init__(self, bar_type: BarType):
        """Initialize a new instance of the PyStrategy class."""
        super().__init__(order_id_tag="001")

        self.bar_type = bar_type
        self.object_storer = ObjectStorer()

    def on_start(self):
        self.subscribe_bars(self.bar_type)

    def on_bar(self, bar_type, bar):
        self.object_storer.store_2(bar_type, bar)

    def on_event(self, event):
        self.object_storer.store(event)


class EmptyStrategy(TradingStrategy):
    """
    An empty strategy which does nothing.
    """

    def __init__(self, order_id_tag):
        """
        Initialize a new instance of the EmptyStrategy class.

        :param order_id_tag: The order_id tag for the strategy (should be unique at trader level).
        """
        super().__init__(order_id_tag=order_id_tag)


class TickTock(TradingStrategy):
    """
    A strategy to test correct sequencing of tick data and timers.
    """

    def __init__(self, instrument, bar_type):
        """Initialize a new instance of the TickTock class."""
        super().__init__(order_id_tag="000")

        self.instrument = instrument
        self.bar_type = bar_type
        self.store = []
        self.timer_running = False
        self.time_alert_counter = 0

    def on_start(self):
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.bar_type.symbol)

    def on_quote_tick(self, tick):
        self.log.info(f"Received Tick({tick})")
        self.store.append(tick)

    def on_bar(self, bar_type, bar):
        self.log.info(f"Received {bar_type} Bar({bar})")
        self.store.append(bar)
        if not self.timer_running:
            timer_name = "Test-Timer"
            self.clock.set_timer(name=timer_name, interval=timedelta(seconds=10))
            self.timer_running = True
            self.log.info(f"Started timer {timer_name}.")

        self.time_alert_counter += 1

        time_alert_name = f"Test-Alert-{self.time_alert_counter}"
        alert_time = bar.timestamp + timedelta(seconds=30)

        self.clock.set_time_alert(name=time_alert_name, alert_time=alert_time)
        self.log.info(f"Set time alert time_alert_name for {alert_time}.")

    def on_event(self, event):
        self.store.append(event)


class TestStrategy1(TradingStrategy):
    """
    A simple strategy for unit testing.
    """

    __test__ = False

    def __init__(self, bar_type, id_tag_strategy="001"):
        """Initialize a new instance of the TestStrategy1 class."""
        super().__init__(order_id_tag=id_tag_strategy)

        self.object_storer = ObjectStorer()
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.register_indicator_for_bars(self.bar_type, self.ema1)
        self.register_indicator_for_bars(self.bar_type, self.ema2)

        self.position_id = None

    def on_start(self):
        self.object_storer.store("custom start logic")

    def on_quote_tick(self, tick):
        self.object_storer.store(tick)

    def on_bar(self, bar_type, bar):
        self.object_storer.store((bar_type, Bar))

        if bar_type.equals(self.bar_type):
            if self.ema1.value > self.ema2.value:
                buy_order = self.order_factory.market(
                    self.bar_type.symbol,
                    OrderSide.BUY,
                    100000,
                )

                self.submit_order(buy_order)
                self.position_id = buy_order.cl_ord_id

            elif self.ema1.value < self.ema2.value:
                sell_order = self.order_factory.market(
                    self.bar_type.symbol,
                    OrderSide.SELL,
                    100000,
                )

                self.submit_order(sell_order)
                self.position_id = sell_order.cl_ord_id

    def on_instrument(self, instrument):
        self.object_storer.store(instrument)

    def on_event(self, event):
        self.object_storer.store(event)

    def on_stop(self):
        self.object_storer.store("custom stop logic")

    def on_reset(self):
        self.object_storer.store("custom reset logic")

    def on_save(self):
        self.object_storer.store("custom save logic")
        return {}

    def on_load(self, state):
        self.object_storer.store("custom load logic")

    def on_dispose(self):
        self.object_storer.store("custom dispose logic")


class EMACross(TradingStrategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then a STOP entry bracket order is
    placed for that direction with a trailing stop and profit target at 1R risk.
    """

    def __init__(
            self,
            symbol: Symbol,
            bar_spec: BarSpecification,
            risk_bp: float=10.0,
            fast_ema: int=10,
            slow_ema: int=20,
            atr_period: int=20,
            sl_atr_multiple: float=2.0,
            extra_id_tag: str=""
    ):
        """
        Initialize a new instance of the EMACross class.

        :param symbol: The symbol for the strategy.
        :param bar_spec: The bar specification for the strategy.
        :param risk_bp: The risk per trade (basis points).
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param atr_period: The ATR period.
        :param sl_atr_multiple: The ATR multiple for stop-loss prices.
        :param extra_id_tag: An optional extra tag to append to order ids.
        """
        super().__init__(order_id_tag=symbol.code.replace('/', '') + extra_id_tag)

        # Custom strategy variables
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

    def on_start(self):
        """Actions to be performed on strategy start."""
        instrument = self.instrument(self.symbol)

        self.precision = instrument.price_precision
        self.entry_buffer = instrument.tick_size.as_double() * 3.0
        self.SL_buffer = instrument.tick_size * 10.0
        self.position_sizer = FixedRiskSizer(instrument)
        self.quote_currency = instrument.quote_currency

        # Register the indicators for updating
        self.register_indicator_for_quote_ticks(self.symbol, self.spread_analyzer)
        self.register_indicator_for_bars(self.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.bar_type, self.slow_ema)
        self.register_indicator_for_bars(self.bar_type, self.atr)

        # Get historical data
        self.request_quote_ticks(self.symbol)
        self.request_bars(self.bar_type)

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

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        Actions to be performed when the strategy is running and receives a bar.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        self.log.info(f"Received {bar_type} Bar({bar})")  # For debugging

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(f"Waiting for indicators to warm up "
                          f"[{self.bar_count(self.bar_type)}]...")
            return  # Wait for indicators to warm up...

        # Check if tick data available
        if not self.has_quote_ticks(self.symbol):
            self.log.info(f"Waiting for {self.symbol} ticks...")
            return  # Wait for ticks...

        # Check average spread
        average_spread = self.spread_analyzer.average
        if average_spread == 0.0:
            self.log.warning(f"average_spread == {average_spread} (not initialized).")
            return  # Protect divide by zero

        spread_buffer = max(average_spread, self.spread_analyzer.current)
        sl_buffer = self.atr.value * self.SL_atr_multiple

        # Check liquidity
        liquidity_ratio = self.atr.value / average_spread
        if liquidity_ratio >= 2.0:
            self._check_signal(bar, sl_buffer, spread_buffer)
        else:
            pass
            # self.log.info(f"liquidity_ratio == {liquidity_ratio} (low liquidity).")

        self._check_trailing_stops(bar, sl_buffer, spread_buffer)

    def _check_signal(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        if self.execution.orders_working_count() == 0 and self.execution.is_completely_flat():  # No active or pending positions
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
                price_type=PriceType.ASK,
            )
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
            unit_batch_size=10000,
        )

        if position_size == 0:
            self.log.info("Insufficient equity for BUY signal.")
            return

        entry_order = self.order_factory.stop(
            symbol=self.symbol,
            order_side=OrderSide.BUY,
            quantity=position_size,
            price=price_entry,
            time_in_force=TimeInForce.GTD,
            expire_time=bar.timestamp + timedelta(minutes=1),
        )

        bracket_order = self.order_factory.bracket(
            entry_order=entry_order,
            stop_loss=price_stop_loss,
            take_profit=price_take_profit,
        )

        self.submit_bracket_order(bracket_order)

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
                price_type=PriceType.BID,
            )
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
            unit_batch_size=10000,
        )

        if position_size == 0:
            self.log.info("Insufficient equity for SELL signal.")
            return

        entry_order = self.order_factory.stop(
            symbol=self.symbol,
            order_side=OrderSide.SELL,
            quantity=position_size,
            price=price_entry,
            time_in_force=TimeInForce.GTD,
            expire_time=bar.timestamp + timedelta(minutes=1),
        )

        bracket_order = self.order_factory.bracket(
            entry_order=entry_order,
            stop_loss=price_stop_loss,
            take_profit=price_take_profit,
        )

        self.submit_bracket_order(bracket_order)

    def _check_trailing_stops(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        for order in self.execution.orders_working():
            if not self.execution.is_stop_loss(order.cl_ord_id):
                return

            # SELL SIDE ORDERS
            if order.is_sell():
                temp_price = Price(bar.low.as_double() - sl_buffer, self.precision)
                if temp_price.gt(order.price):
                    self.modify_order(order, order.quantity, temp_price)
            # BUY SIDE ORDERS
            elif order.is_buy():
                temp_price = Price(bar.high.as_double() + sl_buffer + spread_buffer, self.precision)
                if temp_price.lt(order.price):
                    self.modify_order(order, order.quantity, temp_price)

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
        pass

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders_for_symbol(self.symbol)
        self.flatten_all_positions_for_symbol(self.symbol)

    def on_reset(self):
        """
        Actions to be performed when the strategy is reset.
        """
        pass

    def on_save(self) -> {}:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Note: "OrderIdCount' and 'PositionIdCount' are reserved keys for
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
        self.unsubscribe_instrument(self.symbol)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_quote_ticks(self.symbol)
