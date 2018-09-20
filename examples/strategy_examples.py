#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import datetime, timedelta, timezone
from typing import Dict

from inv_trader.core.precondition import Precondition
from inv_trader.core.logger import Logger
from inv_trader.model.enums import OrderSide, TimeInForce
from inv_trader.model.objects import Price, Tick, BarType, Bar, Instrument
from inv_trader.model.order import Order, OrderFactory
from inv_trader.model.events import Event, OrderFilled
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage

# Note: A 'valid_string' is not None, not empty or not white-space only and less than 1024 chars.
OrderId = str


class EMACrossLimitEntry(TradeStrategy):
    """"
    A simple moving average cross example strategy.
    """

    def __init__(self,
                 label: str,
                 order_id_tag: str,
                 instrument: Instrument,
                 bar_type: BarType,
                 position_size: int,
                 fast: int,
                 slow: int,
                 bar_capacity: int=1000,
                 logger: Logger=None):
        """
        Initializes a new instance of the EMACrossLimitEntry class.

        :param label: The unique label for this instance of the strategy.
        :param label: The unique order id tag for this instance of the strategy.
        :param instrument: The trading instrument for the strategy (could also input any number of them).
        :param bar_type: The bar type for the strategy (could also input any number of them)
        :param position_size: The position unit size.
        :param fast: The fast EMA period.
        :param slow: The slow EMA period.
        :param slow: The historical bar capacity.
        """
        Precondition.valid_string(label, 'label')
        Precondition.positive(fast, 'fast')
        Precondition.positive(slow, 'slow')
        Precondition.true(slow > fast, 'slow > fast')

        super().__init__(label,
                         order_id_tag=order_id_tag,
                         bar_capacity=bar_capacity,
                         logger=logger)

        self.instrument = instrument
        self.symbol = instrument.symbol
        self.bar_type = bar_type
        self.position_size = position_size
        self.tick_decimals = instrument.tick_decimals

        tick_size = instrument.tick_size
        self.entry_buffer_initial = tick_size * 5
        self.entry_buffer = tick_size * 3
        self.SL_buffer = tick_size * 10

        # Create the indicators for the strategy
        self.ema1 = ExponentialMovingAverage(fast)
        self.ema2 = ExponentialMovingAverage(slow)

        # Register the indicators for updating
        self.register_indicator(self.bar_type, self.ema1, self.ema1.update, 'ema1')
        self.register_indicator(self.bar_type, self.ema2, self.ema2.update, 'ema2')

        # Users custom order management logic if you like...
        self.entry_orders = {}      # type: Dict[OrderId, Order]
        self.stop_loss_orders = {}  # type: Dict[OrderId, Order]
        self.position_id = None

    def on_start(self):
        """
        This method is called when self.start() is called, and after internal
        start logic.
        """
        self.log.info(f"Started at {datetime.utcnow()}")
        self.log.info(f"EMA1 bar count={self.ema1.count}")
        self.log.info(f"EMA2 bar count={self.ema2.count}")

        bars = self.bars(self.bar_type)
        self.log.info(f"Bar[-1]={bars[-1]}")
        self.log.info(f"Bar[-2]={bars[-2]}")
        self.log.info(f"Bar[0]={bars[0]}")

    def on_tick(self, tick: Tick):
        """
        This method is called whenever a Tick is received by the strategy, after
        the Tick has been processed by the base class (update last received Tick
        for the Symbol).
        The received Tick object is also passed into the method.

        :param tick: The received tick.
        """
        for order in self.entry_orders.values():
            if not order.is_complete:
                # Slide entry price with market
                if order.side == OrderSide.BUY:
                    temp_entry_slide = Price.create(tick.ask - self.entry_buffer, self.tick_decimals)
                    if temp_entry_slide > order.price:
                        self.modify_order(order, temp_entry_slide)
                elif order.side == OrderSide.SELL:
                    temp_entry_slide = Price.create(tick.bid + self.entry_buffer, self.tick_decimals)
                    if temp_entry_slide < order.price:
                        self.modify_order(order, temp_entry_slide)

        for order in self.stop_loss_orders.values():
            if not order.is_complete:
                if order.side == OrderSide.SELL:
                    temp_stop_slide = Price.create(tick.bid - self.SL_buffer, self.tick_decimals)
                    if temp_stop_slide > order.price:
                        self.modify_order(order, temp_stop_slide)
                elif order.side == OrderSide.BUY:
                    temp_stop_slide = Price.create(tick.ask + self.SL_buffer, self.tick_decimals)
                    if temp_stop_slide < order.price:
                        self.modify_order(order, temp_stop_slide)

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        This method is called whenever the strategy receives a Bar, after the
        Bar has been processed by the base class (update indicators etc).
        The received BarType and Bar objects are also passed into the method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        if not self.ema1.initialized and not self.ema2.initialized:
            return

        for order in self.entry_orders.values():
            if not order.is_complete:
                # Check if order should be expired
                if order.expire_time is not None and bar.timestamp >= order.expire_time:
                    self.cancel_order(order)
                    return

        if bar_type == self.bar_type and self.symbol in self.ticks:
            # If any open positions or pending entry orders then return
            if len(self.positions) > 0:
                return
            if any(order.is_complete is False for order in self.entry_orders.values()):
                return

            expire_time = datetime.now(timezone.utc)

            # BUY LOGIC
            if self.ema1.value >= self.ema2.value:
                entry_order = OrderFactory.limit(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    'S1_E',
                    OrderSide.BUY,
                    self.position_size,
                    Price.create(self.ticks[self.symbol].ask - self.entry_buffer_initial,
                                 self.tick_decimals),
                    time_in_force=TimeInForce.GTD,
                    expire_time=expire_time + timedelta(seconds=10))
                self.entry_orders[entry_order.id] = entry_order
                self.position_id = entry_order.id
                self.submit_order(entry_order, self.position_id)
                self.log.info(f"Added {entry_order.id} to entry orders.")

            # SELL LOGIC
            elif self.ema1.value < self.ema2.value:
                entry_order = OrderFactory.limit(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    'S1_E',
                    OrderSide.SELL,
                    self.position_size,
                    Price.create(self.ticks[self.symbol].bid + self.entry_buffer_initial,
                                 self.tick_decimals),
                    time_in_force=TimeInForce.GTD,
                    expire_time=expire_time + timedelta(seconds=10))
                self.entry_orders[entry_order.id] = entry_order
                self.position_id = entry_order.id
                self.submit_order(entry_order, self.position_id)
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
                    stop_price = Price.create(event.average_price + self.SL_buffer,
                                              self.tick_decimals)
                else:
                    stop_price = Price.create(event.average_price - self.SL_buffer,
                                              self.tick_decimals)

                stop_order = OrderFactory.stop(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    'S1_SL',
                    stop_side,
                    event.filled_quantity,
                    stop_price,
                    time_in_force=TimeInForce.GTC)
                self.stop_loss_orders[stop_order.id] = stop_order
                self.submit_order(stop_order, self.position_id)
                self.log.info(f"Added {stop_order.id} to stop-loss orders.")

    def on_stop(self):
        """
        This method is called when self.stop() is called before internal
        stopping logic.
        """
        self.flatten_all_positions()
        self.cancel_all_orders("STOPPING STRATEGY")

    def on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.

        Put custom code to be run on a strategy reset here.
        """
        self.entry_orders = {}
        self.stop_loss_orders = {}
        self.position_id = None
