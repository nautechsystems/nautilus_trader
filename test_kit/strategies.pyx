#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategies.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import timedelta

from inv_indicators.atr import AverageTrueRange
from inv_indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport Label, PositionId
from nautilus_trader.model.objects cimport Symbol, Price, Tick, BarType, Bar, Instrument
from nautilus_trader.model.order cimport Order, AtomicOrder
from nautilus_trader.trade.strategy cimport TradeStrategy
from nautilus_trader.data.analyzers cimport SpreadAnalyzer, LiquidityAnalyzer
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.common.clock cimport Clock, TestClock
from nautilus_trader.trade.sizing cimport PositionSizer, FixedRiskSizer
from test_kit.objects cimport ObjectStorer


class PyStrategy(TradeStrategy):
    """
    A strategy which is empty and does nothing.
    """

    def __init__(self, bar_type: BarType):
        """
        Initializes a new instance of the PyStrategy class.
        """
        super().__init__(id_tag_strategy='001')
        self.bar_type = bar_type
        self.object_storer = ObjectStorer()

    def on_start(self):
        self.subscribe_bars(self.bar_type)

    def on_tick(self, tick):
        pass

    def on_bar(self, bar_type, bar):
        print(bar)
        self.object_storer.store_2(bar_type, bar)

    def on_event(self, event):
        self.object_storer.store(event)

    def on_stop(self):
        pass

    def on_reset(self):
        pass

    def on_dispose(self):
        pass


cdef class EmptyStrategy(TradeStrategy):
    """
    A strategy which is empty and does nothing.
    """

    def __init__(self, str id_tag_strategy):
        """
        Initializes a new instance of the EmptyStrategy class.
        """
        super().__init__(id_tag_strategy=id_tag_strategy)

    cpdef on_start(self):
        pass

    cpdef on_tick(self, Tick tick):
        pass

    cpdef on_bar(self, BarType bar_type, Bar bar):
        pass

    cpdef on_event(self, Event event):
        pass

    cpdef on_stop(self):
        pass

    cpdef on_reset(self):
        pass

    cpdef on_dispose(self):
        pass


cdef class TickTock(TradeStrategy):
    """
    A strategy to test correct sequencing of tick data and timers.
    """
    cdef readonly Instrument instrument
    cdef readonly BarType bar_type
    cdef readonly list store
    cdef readonly bint timer_running
    cdef readonly int time_alert_counter

    def __init__(self, Instrument instrument,  BarType bar_type,):
        """
        Initializes a new instance of the TickTock class.
        """
        super().__init__(id_tag_strategy='000')

        self.instrument = instrument
        self.bar_type = bar_type
        self.store = []
        self.timer_running = False
        self.time_alert_counter = 0

    cpdef on_start(self):
        self.subscribe_bars(self.bar_type)
        self.subscribe_ticks(self.instrument.symbol)

    cpdef on_tick(self, Tick tick):
        self.log.info(f'Received Tick({tick})')
        self.store.append(tick)

    cpdef on_bar(self, BarType bar_type, Bar bar):
        self.log.info(f'Received {bar_type} Bar({bar})')
        self.store.append(bar)
        if not self.timer_running:
            self.clock.set_timer(label=Label(f'Test-Timer'),
                                 interval=timedelta(seconds=10))
            self.timer_running = True

        self.time_alert_counter += 1
        self.clock.set_time_alert(label=Label(f'Test-Alert-{self.time_alert_counter}'),
                            alert_time=bar.timestamp + timedelta(seconds=30))

    cpdef on_event(self, Event event):
        self.store.append(event)

    cpdef on_stop(self):
        pass

    cpdef on_reset(self):
        pass

    cpdef on_dispose(self):
        pass


cdef class TestStrategy1(TradeStrategy):
    """"
    A simple strategy for unit testing.
    """
    cdef readonly ObjectStorer object_storer
    cdef readonly BarType bar_type
    cdef readonly object ema1
    cdef readonly object ema2
    cdef readonly PositionId position_id

    def __init__(self,
                 BarType bar_type,
                 str id_tag_strategy='001',
                 Clock clock=TestClock()):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__(id_tag_strategy=id_tag_strategy,
                         clock=clock)
        self.object_storer = ObjectStorer()
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.register_indicator(bar_type=self.bar_type,
                                indicator=self.ema1,
                                update_method=self.ema1.update)
        self.register_indicator(bar_type=self.bar_type,
                                indicator=self.ema2,
                                update_method=self.ema2.update)

        self.position_id = None

    cpdef on_start(self):
        self.object_storer.store('custom start logic')

    cpdef on_tick(self, Tick tick):
        self.object_storer.store(tick)

    cpdef on_bar(self, BarType bar_type, Bar bar):

        self.object_storer.store((bar_type, Bar))

        if bar_type == self.bar_type:
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

    cpdef on_event(self, Event event):
        self.object_storer.store(event)

    cpdef on_stop(self):
        self.object_storer.store('custom stop logic')

    cpdef on_reset(self):
        self.object_storer.store('custom reset logic')

    cpdef on_dispose(self):
        self.object_storer.store('custom dispose logic')


cdef class EMACross(TradeStrategy):
    """"
    A simple moving average cross example strategy. When the fast EMA crosses
    the slow EMA then a STOP_MARKET atomic order is placed for that direction
    with a trailing stop and profit target at 1R risk.
    """
    cdef readonly bint warmed_up
    cdef readonly Instrument instrument
    cdef readonly Symbol symbol
    cdef readonly BarType bar_type
    cdef readonly PositionSizer position_sizer
    cdef readonly SpreadAnalyzer spread_analyzer
    cdef readonly LiquidityAnalyzer liquidity
    cdef readonly float risk_bp
    cdef readonly object entry_buffer
    cdef readonly float SL_atr_multiple
    cdef readonly object SL_buffer
    cdef readonly object fast_ema
    cdef readonly object slow_ema
    cdef readonly object atr
    cdef readonly list trailing_stops

    def __init__(self,
                 Instrument instrument,
                 BarType bar_type,
                 float risk_bp=10,
                 int fast_ema=10,
                 int slow_ema=20,
                 int atr_period=20,
                 float sl_atr_multiple=2.0,
                 str extra_id_tag=''):
        """
        Initializes a new instance of the EMACross class.

        :param bar_type: The bar type for the strategy (could also input any number of them)
        :param risk_bp: The risk per trade (basis points).
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param atr_period: The ATR period.
        :param sl_atr_multiple: The ATR multiple for stop-loss prices.
        :param extra_id_tag: The extra tag to appends to the strategies identifier tag.
        """
        # Order id tag must be unique at trader level
        super().__init__(id_tag_strategy=instrument.symbol.code + extra_id_tag)

        # Custom strategy variables
        self.warmed_up = False
        self.instrument = instrument
        self.symbol = instrument.symbol
        self.bar_type = bar_type
        self.risk_bp = risk_bp
        self.position_sizer = FixedRiskSizer(self.instrument)
        self.spread_analyzer = SpreadAnalyzer(self.instrument.tick_precision)
        self.liquidity = LiquidityAnalyzer()
        self.entry_buffer = instrument.tick_size
        self.SL_atr_multiple = sl_atr_multiple
        self.SL_buffer = instrument.tick_size * 10

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)
        self.atr = AverageTrueRange(atr_period)

        # Register the indicators for updating
        self.register_indicator(self.bar_type, self.fast_ema, self.fast_ema.update)
        self.register_indicator(self.bar_type, self.slow_ema, self.slow_ema.update)
        self.register_indicator(self.bar_type, self.atr, self.atr.update)

    cpdef on_start(self):
        """
        This method is called when self.start() is called, and after internal
        start logic.
        """
        # Put custom code to be run on strategy start here
        self.historical_bars(self.bar_type)
        self.subscribe_bars(self.bar_type)
        self.subscribe_ticks(self.symbol)

    cpdef on_tick(self, Tick tick):
        """
        This method is called whenever a Tick is received by the strategy, and 
        after the Tick has been processed by the base class.
        The received Tick object is then passed into this method.

        :param tick: The received tick.
        """
        self.log.info(f"Received Tick({tick})")  # For demonstration purposes
        self.spread_analyzer.update(tick)

    cpdef on_bar(self, BarType bar_type, Bar bar):
        """
        This method is called whenever the strategy receives a Bar, and after the
        Bar has been processed by the base class.
        The received BarType and Bar objects are then passed into this method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        if not self.warmed_up:
            if self.fast_ema.initialized and self.slow_ema.initialized and self.atr.initialized:
                self.warmed_up = True
            else:
                return  # Wait for indicators to warm up...

        cdef AtomicOrder atomic_order

        self.spread_analyzer.calculate_metrics()
        self.liquidity.update(self.spread_analyzer.average_spread, self.atr.value)

        if self.liquidity.is_liquid and self.entry_orders_count() == 0 and self.is_flat():
            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                price_entry = Price(self.last_bar(self.bar_type).high + self.entry_buffer + self.spread_analyzer.average_spread)
                price_stop_loss = Price(self.last_bar(self.bar_type).low - (self.atr.value * self.SL_atr_multiple))
                price_take_profit = Price(price_entry + (price_entry - price_stop_loss))

                exchange_rate = self.get_exchange_rate(quote_currency=self.instrument.quote_currency)
                position_size = self.position_sizer.calculate(
                    equity=self.account.free_equity,
                    risk_bp=self.risk_bp,
                    price_entry=price_entry,
                    price_stop_loss=price_stop_loss,
                    exchange_rate=exchange_rate,
                    commission_rate_bp=0.15,
                    hard_limit=0,
                    units=1,
                    unit_batch_size=1000)

                if position_size.value > 0:
                    atomic_order = self.order_factory.atomic_stop_market(
                        symbol=self.symbol,
                        order_side=OrderSide.BUY,
                        quantity=position_size,
                        price_entry=price_entry,
                        price_stop_loss=price_stop_loss,
                        price_take_profit=price_take_profit,
                        label=Label('S1'),
                        time_in_force=TimeInForce.GTD,
                        expire_time=self.time_now() + timedelta(minutes=1))

            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                price_entry = Price(self.last_bar(self.bar_type).low - self.entry_buffer)
                price_stop_loss = Price(self.last_bar(self.bar_type).high + (self.atr.value * self.SL_atr_multiple) + self.spread_analyzer.average_spread)
                price_take_profit = Price(price_entry - (price_stop_loss - price_entry))

                exchange_rate = self.get_exchange_rate(quote_currency=self.instrument.quote_currency)
                position_size = self.position_sizer.calculate(
                    equity=self.account.free_equity,
                    risk_bp=self.risk_bp,
                    price_entry=price_entry,
                    price_stop_loss=price_stop_loss,
                    exchange_rate=exchange_rate,
                    commission_rate_bp=0.15,
                    hard_limit=0,
                    units=1,
                    unit_batch_size=1000)

                if position_size.value > 0:
                    atomic_order = self.order_factory.atomic_stop_market(
                        symbol=self.symbol,
                        order_side=OrderSide.SELL,
                        quantity=position_size,
                        price_entry=price_entry,
                        price_stop_loss=price_stop_loss,
                        price_take_profit=price_take_profit,
                        label=Label('S1'),
                        time_in_force=TimeInForce.GTD,
                        expire_time=self.time_now() + timedelta(minutes=1))

            # ENTRY ORDER SUBMISSION
            if atomic_order is not None:
                self.submit_atomic_order(atomic_order, self.position_id_generator.generate())

        # TRAILING STOP LOGIC
        cdef Order trailing_stop
        cdef Price temp_price
        for trailing_stop in self.stop_loss_orders().values():
            if trailing_stop.is_active:
                # SELL SIDE ORDERS
                if trailing_stop.is_sell:
                    temp_price = Price(bar.low - (self.atr.value * self.SL_atr_multiple))
                    if temp_price > trailing_stop.price:
                        self.modify_order(trailing_stop, temp_price)
                # BUY SIDE ORDERS
                elif trailing_stop.is_buy:
                    temp_price = Price(
                        bar.high + (self.atr.value * self.SL_atr_multiple) + self.spread_analyzer.average_spread)
                    if temp_price < trailing_stop.price:
                        self.modify_order(trailing_stop, temp_price)

    cpdef on_event(self, Event event):
        """
        This method is called whenever the strategy receives an Event object,
        after the event has been processed by the base class (updating any objects it needs to).
        These events could be AccountEvent, OrderEvent, PositionEvent, TimeEvent.

        :param event: The received event.
        """
        # Put custom code for event handling here (or pass)
        pass

    cpdef on_stop(self):
        """
        This method is called when self.stop() is called after internal
        stopping logic.
        """
        # Put custom code to be run on strategy stop here (or pass)
        pass

    cpdef on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.
        """
        # Put custom code to be run on a strategy reset here (or pass)
        self.warmed_up = False
        self.spread_analyzer.reset()
        self.liquidity.reset()

    cpdef on_dispose(self):
        """
        This method is called when self.dispose() is called. Dispose of any resources
        that had been used by the strategy here.
        """
        # Put custom code to be run on a strategy disposal here (or pass)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_ticks(self.symbol)
