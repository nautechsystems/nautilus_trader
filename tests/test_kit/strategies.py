# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from datetime import timedelta

from nautilus_trader.core.types import Label
from nautilus_trader.model.identifiers import Symbol,PositionId
from nautilus_trader.model.objects import Price, Tick, Instrument
from nautilus_trader.model.objects import BarSpecification, BarType, Bar
from nautilus_trader.model.c_enums.price_type import PriceType
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.order_purpose import OrderPurpose
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.common.clock import TestClock
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.trading.sizing import FixedRiskSizer
from tests.test_kit.mocks import ObjectStorer


class PyStrategy(TradingStrategy):
    """
    A strategy which is empty and does nothing.
    """

    def __init__(self, bar_type: BarType):
        """
        Initializes a new instance of the PyStrategy class.
        """
        super().__init__(order_id_tag='001')

        self.bar_type = bar_type
        self.object_storer = ObjectStorer()

    def on_start(self):
        self.subscribe_bars(self.bar_type)

    def on_tick(self, tick):
        pass

    def on_bar(self, bar_type, bar):
        print(bar)
        self.object_storer.store_2(bar_type, bar)

    def on_instrument(self, instrument):
        pass

    def on_event(self, event):
        self.object_storer.store(event)

    def on_stop(self):
        pass

    def on_reset(self):
        pass

    def on_save(self):
        return {}

    def on_load(self, state):
        pass

    def on_dispose(self):
        pass


class EmptyStrategy(TradingStrategy):
    """
    A strategy which is empty and does nothing.
    """

    def __init__(self, order_id_tag):
        """
        Initializes a new instance of the EmptyStrategy class.

        :param order_id_tag: The order_id tag for the strategy (should be unique at trader level).
        """
        super().__init__(order_id_tag=order_id_tag)

    def on_start(self):
        pass

    def on_tick(self, tick):
        pass

    def on_bar(self, bar_type, bar):
        pass

    def on_instrument(self, instrument):
        pass

    def on_event(self, event):
        pass

    def on_stop(self):
        pass

    def on_reset(self):
        pass

    def on_save(self):
        return {}

    def on_load(self, state):
        pass

    def on_dispose(self):
        pass


class TickTock(TradingStrategy):
    """
    A strategy to test correct sequencing of tick data and timers.
    """

    def __init__(self, instrument, bar_type):
        """
        Initializes a new instance of the TickTock class.
        """
        super().__init__(order_id_tag='000')

        self.instrument = instrument
        self.bar_type = bar_type
        self.store = []
        self.timer_running = False
        self.time_alert_counter = 0

    def on_start(self):
        self.subscribe_bars(self.bar_type)
        self.subscribe_ticks(self.instrument.symbol)

    def on_tick(self, tick):
        self.log.info(f'Received Tick({tick})')
        self.store.append(tick)

    def on_bar(self, bar_type, bar):
        self.log.info(f'Received {bar_type} Bar({bar})')
        self.store.append(bar)
        if not self.timer_running:
            self.clock.set_timer(label=Label(f'Test-Timer'), interval=timedelta(seconds=10))
            self.timer_running = True

        self.time_alert_counter += 1
        self.clock.set_time_alert(
            label=Label(f'Test-Alert-{self.time_alert_counter}'),
            alert_time=bar.timestamp + timedelta(seconds=30))

    def on_instrument(self, instrument):
        pass

    def on_event(self, event):
        self.store.append(event)

    def on_stop(self):
        pass

    def on_reset(self):
        pass

    def on_save(self):
        return {}

    def on_load(self, state):
        pass

    def on_dispose(self):
        pass


class TestStrategy1(TradingStrategy):
    """"
    A simple strategy for unit testing.
    """

    def __init__(self,
                 bar_type,
                 id_tag_strategy='001',
                 clock=TestClock()):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__(order_id_tag=id_tag_strategy, clock=clock)

        self.object_storer = ObjectStorer()
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.register_indicator(self.bar_type, self.ema1, self.ema1.update)
        self.register_indicator(self.bar_type, self.ema2, self.ema2.update)

        self.position_id = None

    def on_start(self):
        self.object_storer.store('custom start logic')
        self.account_inquiry()

    def on_tick(self, tick):
        self.object_storer.store(tick)

    def on_bar(self, bar_type, bar):

        self.object_storer.store((bar_type, Bar))

        if bar_type.equals(self.bar_type):
            if self.ema1.value > self.ema2.value:
                buy_order = self.order_factory.market(
                    self.bar_type.symbol,
                    Label('TestStrategy1_E'),
                    OrderSide.BUY,
                    100000)

                self.submit_order(buy_order, PositionId(str(buy_order.id)))
                self.position_id = buy_order.id

            elif self.ema1.value < self.ema2.value:
                sell_order = self.order_factory.market(
                    self.bar_type.symbol,
                    Label('TestStrategy1_E'),
                    OrderSide.SELL,
                    100000)

                self.submit_order(sell_order, PositionId(str(sell_order.id)))
                self.position_id = sell_order.id

    def on_instrument(self, instrument):
        self.object_storer.store(instrument)

    def on_event(self, event):
        self.object_storer.store(event)

    def on_stop(self):
        self.object_storer.store('custom stop logic')

    def on_reset(self):
        self.object_storer.store('custom reset logic')

    def on_save(self):
        self.object_storer.store('custom save logic')
        return {}

    def on_load(self, state):
        self.object_storer.store('custom load logic')

    def on_dispose(self):
        self.object_storer.store('custom dispose logic')


class EMACross(TradingStrategy):
    """"
    A simple moving average cross example strategy. When the fast EMA crosses
    the slow EMA then a STOP entry atomic order is placed for that direction
    with a trailing stop and profit target at 1R risk.
    """

    def __init__(self,
                 symbol: Symbol,
                 bar_spec: BarSpecification,
                 risk_bp: float=10.0,
                 fast_ema: int=10,
                 slow_ema: int=20,
                 atr_period: int=20,
                 sl_atr_multiple: float=2.0,
                 extra_id_tag: str=''):
        """
        Initializes a new instance of the EMACrossPy class.

        :param symbol: The symbol for the strategy.
        :param bar_spec: The bar specification for the strategy.
        :param risk_bp: The risk per trade (basis points).
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param atr_period: The ATR period.
        :param sl_atr_multiple: The ATR multiple for stop-loss prices.
        """
        super().__init__(order_id_tag=symbol.code + extra_id_tag, bar_capacity=40)

        # Custom strategy variables
        self.symbol = symbol
        self.bar_type = BarType(symbol, bar_spec)
        self.precision = 5          # dummy initial value for FX
        self.risk_bp = risk_bp
        self.entry_buffer = 0.0     # instrument.tick_size
        self.SL_buffer = 0.0        # instrument.tick_size * 10
        self.SL_atr_multiple = sl_atr_multiple

        self.instrument = None      # initialized in on_start()
        self.position_sizer = None  # initialized in on_start()

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)
        self.atr = AverageTrueRange(atr_period)

    def on_start(self):
        """
        This method is called when self.start() is called, and after internal start logic.
        """
        # Put custom code to be run on strategy start here (or pass)
        self.instrument = self.get_instrument(self.symbol)
        self.precision = self.instrument.price_precision
        self.entry_buffer = self.instrument.tick_size.as_double() * 3.0
        self.SL_buffer = self.instrument.tick_size * 10.0
        self.position_sizer = FixedRiskSizer(self.instrument)

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

        # Get historical data
        self.get_ticks(self.symbol)
        self.get_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_instrument(self.symbol)
        self.subscribe_bars(self.bar_type)
        self.subscribe_ticks(self.symbol)

    def on_tick(self, tick: Tick):
        """
        This method is called whenever a Tick is received by the strategy, and
        after the Tick has been processed by the base class.
        The received Tick object is then passed into this method.

        :param tick: The received tick.
        """
        # self.log.info(f"Received Tick({tick})")  # For debugging

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        This method is called whenever the strategy receives a Bar, and after the
        Bar has been processed by the base class.
        The received BarType and Bar objects are then passed into this method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        self.log.info(f"Received {bar_type} Bar({bar})")  # For debugging

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(f"Waiting for indicators to warm up "
                          f"[{self.bar_count(self.bar_type)}]...")
            return  # Wait for indicators to warm up...

        # Check if tick data available
        if not self.has_ticks(self.symbol):
            self.log.info(f"Waiting for {self.symbol.value} ticks...")
            return  # Wait for ticks...

        # Check average spread
        average_spread = self.spread_average(self.symbol)
        if average_spread == 0.0:
            self.log.warning(f"average_spread == {average_spread} (not initialized).")
            return  # Protect divide by zero

        # Check liquidity
        liquidity_ratio = self.atr.value / average_spread
        if liquidity_ratio < 2.0:
            self.log.info(f"liquidity_ratio == {liquidity_ratio} (no liquidity).")
            return

        spread_buffer = max(average_spread, self.spread(self.symbol))
        sl_buffer = self.atr.value * self.SL_atr_multiple

        if self.count_orders_working() == 0 and self.is_flat():  # No active or pending positions
            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                self._enter_long(bar, sl_buffer, spread_buffer)
            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                self._enter_short(bar, sl_buffer, spread_buffer)

        self._check_trailing_stops(bar, sl_buffer, spread_buffer)

    def _enter_long(self, bar: Bar, sl_buffer: float, spread_buffer: float):
        price_entry = Price(bar.high.as_double() + self.entry_buffer + spread_buffer, self.precision)
        price_stop_loss = Price(bar.low.as_double() - sl_buffer, self.precision)

        risk = price_entry.as_double() - price_stop_loss.as_double()
        price_take_profit = Price(price_entry.as_double() + risk, self.precision)

        # Calculate exchange rate
        exchange_rate = 0.0
        try:
            exchange_rate = self.get_exchange_rate_for_account(
                quote_currency=self.instrument.quote_currency,
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
            atomic_order = self.order_factory.atomic_stop_market(
                symbol=self.symbol,
                order_side=OrderSide.BUY,
                quantity=position_size,
                entry=price_entry,
                stop_loss=price_stop_loss,
                take_profit=price_take_profit,
                time_in_force=TimeInForce.GTD,
                expire_time=bar.timestamp + timedelta(minutes=1))

            self.submit_atomic_order(atomic_order, self.position_id_generator.generate())
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
                quote_currency=self.instrument.quote_currency,
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
            atomic_order = self.order_factory.atomic_stop_market(
                symbol=self.symbol,
                order_side=OrderSide.SELL,
                quantity=position_size,
                entry=price_entry,
                stop_loss=price_stop_loss,
                take_profit=price_take_profit,
                time_in_force=TimeInForce.GTD,
                expire_time=bar.timestamp + timedelta(minutes=1))

            self.submit_atomic_order(atomic_order, self.position_id_generator.generate())
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

    def on_instrument(self, instrument: Instrument):
        """
        This method is called whenever the strategy receives an Instrument update.

        :param instrument: The received instrument.
        """
        if self.instrument.symbol.equals(instrument.symbol):
            self.instrument = instrument

        self.log.info(f"Updated instrument {instrument}.")

    def on_event(self, event):
        """
        This method is called whenever the strategy receives an Event object,
        and after the event has been processed by the TradingStrategy base class.
        These events could be AccountEvent, OrderEvent, PositionEvent, TimeEvent.

        :param event: The received event.
        """
        # Put custom code for event handling here (or pass)
        pass

    def on_stop(self):
        """
        This method is called when self.stop() is called and after internal
        stopping logic.
        """
        # Put custom code to be run on strategy stop here (or pass)
        pass

    def on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.
        """
        # Put custom code to be run on a strategy reset here (or pass)
        pass

    def on_save(self) -> {}:
        # Put custom state to be saved here (or return empty dictionary)
        return {}

    def on_load(self, state: {}):
        # Put custom state to be loaded here (or pass)
        pass

    def on_dispose(self):
        """
        This method is called when self.dispose() is called. Dispose of any
        resources that has been used by the strategy here.
        """
        # Put custom code to be run on a strategy disposal here (or pass)
        self.unsubscribe_instrument(self.symbol)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_ticks(self.symbol)
