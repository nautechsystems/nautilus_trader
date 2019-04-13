#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategies.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from decimal import Decimal
from datetime import timedelta

from inv_trader.common.clock cimport Clock, TestClock
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.time_in_force cimport TimeInForce
from inv_trader.model.objects cimport Symbol, Price, Tick, BarType, Bar, Instrument
from inv_trader.model.events cimport Event
from inv_trader.model.identifiers cimport Label, OrderId, PositionId
from inv_trader.model.order cimport Order, AtomicOrder
from inv_trader.strategy cimport TradeStrategy
from inv_trader.portfolio.sizing cimport PositionSizer, FixedRiskSizer

from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.atr import AverageTrueRange
from test_kit.objects cimport ObjectStorer


class PyStrategy(TradeStrategy):
    """
    A strategy which is empty and does nothing.
    """

    def __init__(self, bar_type: BarType):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__()
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
    cpdef void on_start(self):
        pass

    cpdef void on_tick(self, Tick tick):
        pass

    cpdef void on_bar(self, BarType bar_type, Bar bar):
        pass

    cpdef void on_event(self, Event event):
        pass

    cpdef void on_stop(self):
        pass

    cpdef void on_reset(self):
        pass

    cpdef void on_dispose(self):
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

    def __init__(self, BarType bar_type, Clock clock=TestClock()):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__(clock=clock)
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

    cpdef void on_start(self):
        self.object_storer.store('custom start logic')

    cpdef void on_tick(self, Tick tick):
        self.object_storer.store(tick)

    cpdef void on_bar(self, BarType bar_type, Bar bar):

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

    cpdef void on_event(self, Event event):
        self.object_storer.store(event)

    cpdef void on_stop(self):
        self.object_storer.store('custom stop logic')

    cpdef void on_reset(self):
        self.object_storer.store('custom reset logic')

    cpdef void on_dispose(self):
        self.object_storer.store('custom dispose logic')


cdef class EMACross(TradeStrategy):
    """"
    A simple moving average cross example strategy. When the fast EMA crosses
    the slow EMA then a STOP_MARKET atomic order is placed for that direction
    with a trailing stop and profit target at 1R risk.
    """
    cdef readonly Instrument instrument
    cdef readonly Symbol symbol
    cdef readonly BarType bar_type
    cdef readonly PositionSizer position_sizer
    cdef readonly float risk_bp
    cdef readonly int tick_precision
    cdef readonly object entry_buffer
    cdef readonly float SL_atr_multiple
    cdef readonly object SL_buffer
    cdef readonly object spread
    cdef readonly object fast_ema
    cdef readonly object slow_ema
    cdef readonly object atr
    cdef readonly list trailing_stops

    def __init__(self,
                 str label,
                 str id_tag_trader,
                 str id_tag_strategy,
                 Instrument instrument,
                 BarType bar_type,
                 float risk_bp=10,
                 int fast_ema=10,
                 int slow_ema=20,
                 int atr_period=20,
                 float sl_atr_multiple=2.0,
                 flatten_on_sl_reject=True,
                 flatten_on_stop=True,
                 cancel_all_orders_on_stop=True):
        """
        Initializes a new instance of the EMACross class.

        :param label: The optional unique label for the strategy.
        :param id_tag_trader: The unique order identifier tag for the trader.
        :param id_tag_strategy: The unique order identifier tag for the strategy.
        :param bar_type: The bar type for the strategy (could also input any number of them)
        :param risk_bp: The risk per trade (basis points).
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param atr_period: The ATR period.
        :param sl_atr_multiple: The ATR multiple for stop-loss prices.
        :param flatten_on_sl_reject: The flag indicating whether the position with an
        associated stop order should be flattened if the order is rejected.
        :param flatten_on_stop: The flag indicating whether the strategy should
        be flattened on stop.
        :param cancel_all_orders_on_stop: The flag indicating whether all residual
        orders should be cancelled on stop.
        """
        # Send the below arguments into the base class
        super().__init__(label=label,
                         id_tag_trader=id_tag_trader,
                         id_tag_strategy=id_tag_strategy,
                         flatten_on_sl_reject=flatten_on_sl_reject,
                         flatten_on_stop=flatten_on_stop,
                         cancel_all_orders_on_stop=cancel_all_orders_on_stop)

        # Custom strategy variables
        self.instrument = instrument
        self.symbol = instrument.symbol
        self.bar_type = bar_type
        self.risk_bp = risk_bp
        self.position_sizer = FixedRiskSizer(self.instrument)
        self.tick_precision = instrument.tick_precision
        self.entry_buffer = instrument.tick_size
        self.SL_atr_multiple = sl_atr_multiple
        self.SL_buffer = instrument.tick_size * 10
        self.spread = Decimal(0)

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)
        self.atr = AverageTrueRange(atr_period)

        # Register the indicators for updating
        self.register_indicator(self.bar_type, self.fast_ema, self.fast_ema.update)
        self.register_indicator(self.bar_type, self.slow_ema, self.slow_ema.update)
        self.register_indicator(self.bar_type, self.atr, self.atr.update)

    cpdef void on_start(self):
        """
        This method is called when self.start() is called, and after internal
        start logic.
        """
        # Put custom code to be run on strategy start here
        self.historical_bars(self.bar_type)
        self.subscribe_bars(self.bar_type)
        self.subscribe_ticks(self.symbol)

    cpdef void on_tick(self, Tick tick):
        """
        This method is called whenever a Tick is received by the strategy, and 
        after the Tick has been processed by the base class.
        The received Tick object is then passed into this method.

        :param tick: The received tick.
        """
        self.spread = tick.ask - tick.bid
        self.log.info(f"Received Tick({tick})")  # For demonstration purposes

    cpdef void on_bar(self, BarType bar_type, Bar bar):
        """
        This method is called whenever the strategy receives a Bar, and after the
        Bar has been processed by the base class.
        The received BarType and Bar objects are then passed into this method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        if not self.fast_ema.initialized or not self.slow_ema.initialized:
            # Wait for indicators to warm up...
            return

        cdef AtomicOrder atomic_order

        if self.entry_orders_count() == 0 and self.is_flat():
            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                price_entry = Price(self.last_bar(self.bar_type).high + self.entry_buffer + self.spread)
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
                price_stop_loss = Price(self.last_bar(self.bar_type).high + (self.atr.value * self.SL_atr_multiple) + self.spread)
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
                self.submit_atomic_order(atomic_order, self.generate_position_id(self.symbol))

        # TRAILING STOP LOGIC
        cdef Order trailing_stop
        cdef Price temp_price
        for trailing_stop in self.stop_loss_orders().values():
            if trailing_stop.is_active:
                if trailing_stop.side == OrderSide.SELL:
                    temp_price = Price(bar.low - (self.atr.value * self.SL_atr_multiple))
                    if temp_price > trailing_stop.price:
                        self.modify_order(trailing_stop, temp_price)
                elif trailing_stop.side == OrderSide.BUY:
                    temp_price = Price(bar.high + (self.atr.value * self.SL_atr_multiple) + self.spread)
                    if temp_price < trailing_stop.price:
                        self.modify_order(trailing_stop, temp_price)

    cpdef void on_event(self, Event event):
        """
        This method is called whenever the strategy receives an Event object,
        after the event has been processed by the base class (updating any objects it needs to).
        These events could be AccountEvent, OrderEvent, PositionEvent, TimeEvent.

        :param event: The received event.
        """
        # Put custom code for event handling here (or pass)
        pass

    cpdef void on_stop(self):
        """
        This method is called when self.stop() is called after internal
        stopping logic.
        """
        # Put custom code to be run on strategy stop here (or pass)
        pass

    cpdef void on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.
        """
        # Put custom code to be run on a strategy reset here (or pass)
        pass

    cpdef void on_dispose(self):
        """
        This method is called when self.dispose() is called. Dispose of any resources
        that had been used by the strategy here.
        """
        # Put custom code to be run on a strategy disposal here (or pass)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_ticks(self.symbol)
