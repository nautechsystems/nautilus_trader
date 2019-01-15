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

from inv_trader.common.logger import Logger
from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide, TimeInForce
from inv_trader.model.objects import Symbol, Tick, BarType, Bar, Instrument, Price
from inv_trader.model.events import Event
from inv_trader.model.identifiers import Label, OrderId, PositionId
from inv_trader.model.order import Order
from inv_trader.model.events import OrderFilled
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.atr import AverageTrueRange
from test_kit.objects import ObjectStorer

GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class TestStrategy1(TradeStrategy):
    """"
    A simple strategy for unit testing.
    """

    def __init__(self, bar_type: BarType):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__(label='UnitTests', order_id_tag='TS01')
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
                    Symbol('GBPUSD', Venue.FXCM),
                    OrderId('O123456'),
                    Label('TestStrategy1_E'),
                    OrderSide.BUY,
                    100000)

                self.submit_order(buy_order, PositionId(str(buy_order.id)))
                self.position_id = buy_order.id

            elif self.ema1.value < self.ema2.value:
                sell_order = self.order_factory.market(
                    Symbol('GBPUSD', Venue.FXCM),
                    OrderId('O123456'),
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


class EMACross(TradeStrategy):
    """"
    A simple moving average cross example strategy.
    """

    def __init__(self,
                 label: str,
                 order_id_tag: str,
                 instrument: Instrument,
                 bar_type: BarType,
                 position_size: int,
                 fast_ema: int,
                 slow_ema: int,
                 atr_period: int,
                 sl_atr_multiple: int,
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

    def on_start(self):
        """
        This method is called when self.start() is called, and after internal
        start logic.
        """
        # Subscribe to the necessary data.
        self.historical_bars(self.bar_type)
        self.subscribe_bars(self.bar_type)

    def on_tick(self, tick: Tick):
        """
        This method is called whenever a Tick is received by the strategy, after
        the Tick has been processed by the base class (update last received Tick
        for the Symbol).
        The received Tick object is also passed into the method.

        :param tick: The received tick.
        """
        pass

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        This method is called whenever the strategy receives a Bar, after the
        Bar has been processed by the base class (update indicators etc).
        The received BarType and Bar objects are also passed into the method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        if not self.fast_ema.initialized or not self.slow_ema.initialized:
            return

        for order in self.entry_orders.values():
            if not order.is_complete:
                # Check if order should be expired
                if order.expire_time is not None and bar.timestamp >= order.expire_time:
                    self.cancel_order(order)
                    return

            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                entry_order = self.order_factory.stop_market(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_E'),
                    OrderSide.BUY,
                    self.position_size,
                    Price.create(self.bar(self.bar_type)[0].high + self.entry_buffer, self.tick_precision),
                    time_in_force=TimeInForce.GTD,
                    expire_time=self.time_now + timedelta(minutes=1))
                self.entry_orders[entry_order.id] = entry_order
                self.position_id = entry_order.id
                self.submit_order(entry_order, PositionId(str(self.position_id)))
                self.log.info(f"Added {entry_order.id} to entry orders.")

            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                entry_order = self.order_factory.stop_market(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_E'),
                    OrderSide.SELL,
                    self.position_size,
                    Price.create(self.bar(self.bar_type)[0].low - self.entry_buffer, self.tick_precision),
                    time_in_force=TimeInForce.GTD,
                    expire_time=self.time_now + timedelta(minutes=1))
                self.entry_orders[entry_order.id] = entry_order
                self.position_id = entry_order.id
                self.submit_order(entry_order, PositionId(str(self.position_id)))
                self.log.info(f"Added {entry_order.id} to entry orders.")

    def on_event(self, event: Event):
        """
        This method is called whenever the strategy receives an Event object,
        after the event has been processed by the base class (updating any objects it needs to).
        These events could be AccountEvent, OrderEvent.

        :param event: The received event.
        """
        if isinstance(event, OrderFilled):
            # A real strategy should also cover the OrderPartiallyFilled case...

            if event.order_id in self.entry_orders.keys():
                # SET TRAILING STOP
                stop_side = self.get_opposite_side(event.order_side)
                if stop_side is OrderSide.BUY:
                    stop_price = Price.create(self.bar(self.bar_type)[0].high
                                              + self.atr.value * self.SL_atr_multiple,
                                              self.tick_precision)
                else:
                    stop_price = Price.create(self.bar(self.bar_type)[0].low
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

    def on_stop(self):
        """
        This method is called when self.stop() is called before internal
        stopping logic.
        """
        if not self.is_flat:
            self.flatten_all_positions()

        self.cancel_all_orders("STOPPING STRATEGY")

    def on_reset(self):
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
