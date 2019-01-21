#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategies.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from datetime import timedelta
from typing import Dict

from inv_trader.core.decimal cimport Decimal
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.common.logger cimport Logger
from inv_trader.enums.venue cimport Venue
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.time_in_force cimport TimeInForce
from inv_trader.model.objects cimport Symbol, Tick, BarType, Bar, Instrument
from inv_trader.model.price cimport price
from inv_trader.model.events cimport Event
from inv_trader.model.identifiers cimport Label, OrderId, PositionId
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport OrderFilled, OrderExpired, OrderRejected, OrderWorking
from inv_trader.strategy cimport TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.atr import AverageTrueRange
from test_kit.objects import ObjectStorer

GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class TestStrategy1(TradeStrategy):
    """"
    A simple strategy for unit testing.
    """

    def __init__(self, bar_type: BarType, clock: Clock=LiveClock()):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__(label='UnitTests', order_id_tag='TS01', clock=clock)
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

    def on_start(self):
        self.object_storer.store('custom start logic')

    def on_tick(self, tick: Tick):
        self.object_storer.store(tick)

    def on_bar(
            self,
            bar_type: BarType,
            bar: Bar):

        self.object_storer.store((bar_type, Bar))

        if bar_type == self.bar_type:
            if self.ema1.value > self.ema2.value:
                buy_order = self.order_factory.market(
                    self.bar_type.symbol, Venue.FXCM,
                    self.generate_order_id(self.bar_type.symbol),
                    Label('TestStrategy1_E'),
                    OrderSide.BUY,
                    100000)

                self.submit_order(buy_order, PositionId(str(buy_order.id)))
                self.position_id = buy_order.id

            elif self.ema1.value < self.ema2.value:
                sell_order = self.order_factory.market(
                    self.bar_type.symbol, Venue.FXCM,
                    self.generate_order_id(self.bar_type.symbol),
                    Label('TestStrategy1_E'),
                    OrderSide.SELL,
                    100000)

                self.submit_order(sell_order, PositionId(str(sell_order.id)))
                self.position_id = sell_order.id

    def on_event(self, event: Event):
        self.object_storer.store(event)

    def on_stop(self):
        self.object_storer.store('custom stop logic')

    def on_reset(self):
        self.object_storer.store('custom reset logic')


cdef class EMACross(TradeStrategy):
    """"
    A simple moving average cross example strategy.
    """
    cdef readonly Instrument instrument
    cdef readonly Symbol symbol
    cdef readonly BarType bar_type
    cdef readonly int position_size
    cdef readonly int tick_precision
    cdef readonly Decimal entry_buffer
    cdef readonly float SL_atr_multiple
    cdef readonly Decimal SL_buffer
    cdef readonly object fast_ema
    cdef readonly object slow_ema
    cdef readonly object atr
    cdef readonly dict entry_orders
    cdef readonly dict stop_loss_orders
    cdef readonly PositionId position_id

    def __init__(self,
                 label: str,
                 order_id_tag: str,
                 instrument: Instrument,
                 bar_type: BarType,
                 position_size: int=100000,
                 fast_ema: int=10,
                 slow_ema: int=20,
                 atr_period: int=20,
                 sl_atr_multiple: float=2,
                 bar_capacity: int=1000,
                 logger: Logger=None):
        """
        Initializes a new instance of the EMACrossLimitEntry class.

        :param label: The unique label for this instance of the strategy.
        :param order_id_tag: The unique order id tag for this instance of the strategy.
        :param bar_type: The bar type for the strategy (could also input any number of them)
        :param position_size: The position unit size.
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param bar_capacity: The historical bar capacity.
        :param logger: The logger for the strategy (can be None, will just print).
        """
        super().__init__(label,
                         order_id_tag=order_id_tag,
                         bar_capacity=bar_capacity,
                         logger=logger)

        self.instrument = instrument
        self.symbol = instrument.symbol
        self.bar_type = bar_type
        self.position_size = position_size
        self.tick_precision = instrument.tick_precision
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

        # Users custom order management logic if you like...
        self.entry_orders = {}      # type: Dict[OrderId, Order]
        self.stop_loss_orders = {}  # type: Dict[OrderId, Order]
        self.position_id = None

    cpdef void on_start(self):
        """
        This method is called when self.start() is called, and after internal
        start logic.
        """
        # Subscribe to the necessary data.
        self.subscribe_bars(self.bar_type)

    cpdef void on_tick(self, Tick tick):
        """
        This method is called whenever a Tick is received by the strategy, after
        the Tick has been processed by the base class (update last received Tick
        for the Symbol).
        The received Tick object is also passed into the method.

        :param tick: The received tick.
        """
        pass

    cpdef void on_bar(self, BarType bar_type, Bar bar):
        """
        This method is called whenever the strategy receives a Bar, after the
        Bar has been processed by the base class (update indicators etc).
        The received BarType and Bar objects are also passed into the method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        if not self.fast_ema.initialized or not self.slow_ema.initialized:
            return

        print(f"LAST BAR: {self.last_bar(self.bar_type)}")
        # TODO: Account for spread, bid bars only at the moment
        if self.position_id is None:
            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                entry_order = self.order_factory.stop_market(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_E'),
                    OrderSide.BUY,
                    self.position_size,
                    price(self.last_bar(self.bar_type).high + self.entry_buffer, self.tick_precision),
                    time_in_force=TimeInForce.GTD,
                    expire_time=self.time_now() + timedelta(minutes=1))
                self.entry_orders[entry_order.id] = entry_order
                self.position_id = PositionId(str(entry_order.id))
                self.submit_order(entry_order, self.position_id)
                self.log.info(f"Added {entry_order.id} to entry orders.")

            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                entry_order = self.order_factory.stop_market(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_E'),
                    OrderSide.SELL,
                    self.position_size,
                    price(self.last_bar(self.bar_type).low - self.entry_buffer, self.tick_precision),
                    time_in_force=TimeInForce.GTD,
                    expire_time=self.time_now() + timedelta(minutes=1))
                self.entry_orders[entry_order.id] = entry_order
                self.position_id = PositionId(str(entry_order.id))
                self.submit_order(entry_order, self.position_id)
                self.log.info(f"Added {entry_order.id} to entry orders.")

        for order_id, order in self.stop_loss_orders.items():
            if order.side is OrderSide.SELL:
                temp_price = price(self.last_bar(self.bar_type).low
                                   - self.atr.value * self.SL_atr_multiple,
                                   self.tick_precision)
                if order.price < temp_price:
                    self.modify_order(order, temp_price)
            elif order.side is OrderSide.BUY:
                temp_price = price(self.last_bar(self.bar_type).low
                                   - self.atr.value * self.SL_atr_multiple,
                                   self.tick_precision)
                if order.price > temp_price:
                    self.modify_order(order, temp_price)

    cpdef void on_event(self, Event event):
        """
        This method is called whenever the strategy receives an Event object,
        after the event has been processed by the base class (updating any objects it needs to).
        These events could be AccountEvent, OrderEvent.

        :param event: The received event.
        """
        # If an entry order is rejected them remove it
        if isinstance(event, OrderRejected):
            if event.order_id in self.entry_orders:
                del self.entry_orders[event.order_id]
                self.position_id = None

        if isinstance(event, OrderFilled):
            # A real strategy should also cover the OrderPartiallyFilled case...

            if event.order_id in self.entry_orders:
                # SET TRAILING STOP
                stop_side = self.get_opposite_side(event.order_side)
                if stop_side is OrderSide.BUY:
                    stop_price = price(self.last_bar(self.bar_type).high
                                       + self.atr.value * self.SL_atr_multiple,
                                       self.tick_precision)
                else:
                    stop_price = price(self.last_bar(self.bar_type).low
                                       - self.atr.value * self.SL_atr_multiple,
                                       self.tick_precision)

                stop_order = self.order_factory.stop_market(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_SL'),
                    stop_side,
                    event.filled_quantity,
                    stop_price)
                self.stop_loss_orders[stop_order.id] = stop_order
                self.submit_order(stop_order, self.position_id)
                self.log.info(f"Added {stop_order.id} to stop-loss orders.")
            elif event.order_id in self.stop_loss_orders:
                del self.stop_loss_orders[event.order_id]
                self.position_id = None

        if isinstance(event, OrderExpired):
            if event.order_id in self.entry_orders:
                del self.entry_orders[event.order_id]
                self.position_id = None

        if isinstance(event, OrderRejected):
            print(f"OrderRejected({event.order_id}): {event.rejected_reason}")
            if event.order_id in self.entry_orders:
                del self.entry_orders[event.order_id]
                self.position_id = None
            # If a stop-loss order is rejected then flatten the entered position
            if event.order_id in self.stop_loss_orders:
                self.flatten_all_positions()
                self.entry_orders = {}      # type: Dict[OrderId, Order]
                self.stop_loss_orders = {}  # type: Dict[OrderId, Order]
                self.position_id = None

    cpdef void on_stop(self):
        """
        This method is called when self.stop() is called before internal
        stopping logic.
        """
        if not self.is_flat():
            self.flatten_all_positions()

        self.cancel_all_orders("STOPPING STRATEGY")

    cpdef void on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.

        Put custom code to be run on a strategy reset here.
        """
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_ticks(self.symbol)

        self.entry_orders = {}
        self.stop_loss_orders = {}
        self.position_id = None
