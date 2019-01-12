#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from datetime import timedelta
from typing import Dict

from inv_trader.core.precondition import Precondition
from inv_trader.core.logger import Logger
from inv_trader.model.enums import OrderSide, OrderStatus, TimeInForce
from inv_trader.model.objects import Price, Symbol, Tick, BarType, Bar
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderFilled, TimeEvent
from inv_trader.model.identifiers import Label, OrderId, PositionId
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage


class EMACrossLimitEntry(TradeStrategy):
    """"
    A simple moving average cross example strategy.
    """

    def __init__(self,
                 label: str,
                 order_id_tag: str,
                 symbol: Symbol,
                 tick_decimals: int,
                 tick_size: Decimal,
                 bar_type: BarType,
                 position_size: int,
                 fast_ema: int,
                 slow_ema: int,
                 bar_capacity: int=1000,
                 logger: Logger=None):
        """
        Initializes a new instance of the EMACrossLimitEntry class.

        :param label: The unique label for this instance of the strategy.
        :param order_id_tag: The unique order id tag for this instance of the strategy.
        :param symbol: The trading symbol for the strategy.
        :param tick_decimals: The tick decimal precision for the instrument.
        :param tick_size: The tick size for the instrument.
        :param bar_type: The bar type for the strategy (could also input any number of them)
        :param position_size: The position unit size.
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param bar_capacity: The historical bar capacity.
        :param logger: The logger for the strategy (can be None, will just print).
        """
        Precondition.true(slow_ema > fast_ema, 'slow_ema > fast_ema')

        super().__init__(label,
                         order_id_tag=order_id_tag,
                         bar_capacity=bar_capacity,
                         logger=logger)

        self.symbol = symbol
        self.bar_type = bar_type
        self.position_size = position_size
        self.tick_decimals = tick_decimals
        self.entry_buffer_initial = tick_size * 5
        self.entry_buffer = tick_size * 3
        self.SL_buffer = tick_size * 10

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)

        # Register the indicators for updating
        self.register_indicator(self.bar_type, self.fast_ema, self.fast_ema.update, Label('fast_ema'))
        self.register_indicator(self.bar_type, self.slow_ema, self.slow_ema.update, Label('slow_ema'))

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
        self.subscribe_ticks(self.symbol)

        self.set_time_alert(Label("test-alert1"), self.time_now + timedelta(seconds=10))

    def on_tick(self, tick: Tick):
        """
        This method is called whenever a Tick is received by the strategy, after
        the Tick has been processed by the base class (update last received Tick
        for the Symbol).
        The received Tick object is also passed into the method.

        :param tick: The received tick.
        """
        for order in self.entry_orders.values():
            if order.status is OrderStatus.WORKING:
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
        if not self.fast_ema.initialized or not self.slow_ema.initialized:
            return

        for order in self.entry_orders.values():
            if not order.is_complete:
                # Check if order should be expired
                if order.expire_time is not None and bar.timestamp >= order.expire_time:
                    self.cancel_order(order)
                    return

        if self.symbol in self.ticks:
            # If any open positions or pending entry orders then return
            if not self.is_flat:
                return
            if len(self.active_orders) > 0:
                return

            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                entry_order = self.order_factory.limit(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_E'),
                    OrderSide.BUY,
                    self.position_size,
                    Price.create(self.ticks[self.symbol].ask - self.entry_buffer_initial,
                                 self.tick_decimals),
                    time_in_force=TimeInForce.GTD,
                    expire_time=self.time_now + timedelta(seconds=10))
                self.entry_orders[entry_order.id] = entry_order
                self.position_id = entry_order.id
                self.submit_order(entry_order, PositionId(str(self.position_id)))
                self.log.info(f"Added {entry_order.id} to entry orders.")

            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                entry_order = self.order_factory.limit(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_E'),
                    OrderSide.SELL,
                    self.position_size,
                    Price.create(self.ticks[self.symbol].bid + self.entry_buffer_initial,
                                 self.tick_decimals),
                    time_in_force=TimeInForce.GTD,
                    expire_time=self.time_now + timedelta(seconds=10))
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
                    stop_price = Price.create(event.average_price + self.SL_buffer,
                                              self.tick_decimals)
                else:
                    stop_price = Price.create(event.average_price - self.SL_buffer,
                                              self.tick_decimals)

                stop_order = self.order_factory.stop_market(
                    self.symbol,
                    self.generate_order_id(self.symbol),
                    Label('S1_SL'),
                    stop_side,
                    event.filled_quantity,
                    stop_price,
                    time_in_force=TimeInForce.GTC)
                self.stop_loss_orders[stop_order.id] = stop_order
                self.submit_order(stop_order, self.position_id)
                self.log.info(f"Added {stop_order.id} to stop-loss orders.")

        if isinstance(event, TimeEvent):
            if event.label == 'test-alert1':
                self.set_timer(Label("test-timer1"), interval=timedelta(seconds=30), repeat=True)

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
