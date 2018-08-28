#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pytz

from datetime import datetime, timedelta
from decimal import Decimal

from inv_trader.model.enums import Resolution, QuoteType, OrderSide, TimeInForce, Venue
from inv_trader.model.objects import Symbol, Tick, BarType, Bar
from inv_trader.model.events import Event, OrderRejected, OrderFilled, OrderPartiallyFilled, OrderModified
from inv_trader.factories import OrderFactory
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage

# Constants
OrderId = str
AUDUSD_FXCM = Symbol("AUDUSD", Venue.FXCM)
AUDUSD_FXCM_1_SECOND_MID = BarType(AUDUSD_FXCM,
                                   1,
                                   Resolution.SECOND,
                                   QuoteType.MID)


class EMACross(TradeStrategy):
    """"
    A simple moving average crossing example strategy.
    """

    def __init__(self,
                 label,
                 fast,
                 slow):
        """
        Initializes a new instance of the EMACross class.
        """
        super().__init__(label, order_id_tag='001')  # Note you can add a unique order id tag

        # Create the indicators for the strategy
        self.ema1 = ExponentialMovingAverage(fast)
        self.ema2 = ExponentialMovingAverage(slow)

        # Register the indicators for updating
        self.register_indicator(AUDUSD_FXCM_1_SECOND_MID, self.ema1, self.ema1.update, 'ema1')
        self.register_indicator(AUDUSD_FXCM_1_SECOND_MID, self.ema2, self.ema2.update, 'ema2')

        # Users custom order management logic if you like...
        self.entry_orders = {}
        self.stop_loss_orders = {}

    def on_start(self):
        """
        This method is called when self.start() is called, and after internal
        start logic.
        """
        self._log(f"My strategy started at {datetime.utcnow()}.")

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
                if order.side == OrderSide.BUY and tick.ask - Decimal('0.00003') > order.price:
                    self.modify_order(order, tick.ask - Decimal('0.00003'))
                elif order.side == OrderSide.SELL and tick.bid + Decimal('0.00003') < order.price:
                    self.modify_order(order, tick.bid + Decimal('0.00003'))

        for order in self.stop_loss_orders.values():
            if not order.is_complete:
                if order.side == OrderSide.SELL and tick.bid - Decimal('0.00010') > order.price:
                    self.modify_order(order, tick.bid - Decimal('0.00010'))
                elif order.side == OrderSide.BUY and tick.ask + Decimal('0.00010') < order.price:
                    self.modify_order(order, tick.ask + Decimal('0.00010'))

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

        for order in self.stop_loss_orders.values():
            if len(self.bars(AUDUSD_FXCM_1_SECOND_MID)) > 20 and not order.is_complete:
                bars = self.bars(AUDUSD_FXCM_1_SECOND_MID)[-20:]
                # Trail stop to last minute
                if order.side is OrderSide.BUY:
                    min_low = min(bar.low for bar in bars)
                    if order.price < min_low - Decimal('0.00003'):
                        self.modify_order(order, min_low - Decimal('0.00003'))
                elif order.side is OrderSide.SELL:
                    max_high = max(bar.high for bar in bars)
                    if order.price > max_high + Decimal('0.00003'):
                        self.modify_order(order, max_high + Decimal('0.00003'))

        for order in self.entry_orders.values():
            if not order.is_complete:
                # Check if order should be expired
                if order.expire_time is not None and bar.timestamp >= order.expire_time:
                    self.cancel_order(order)
                    return

        if bar_type == AUDUSD_FXCM_1_SECOND_MID and AUDUSD_FXCM in self.ticks:
            # If any open positions or pending entry orders then return
            if len(self.positions) > 0:
                return
            if any(order.is_complete is False for order in self.entry_orders.values()):
                return

            expire_time = datetime.now(pytz.utc)

            # BUY LOGIC
            if self.ema1.value >= self.ema2.value:
                entry_order = OrderFactory.limit(
                    AUDUSD_FXCM,
                    self.generate_order_id(AUDUSD_FXCM),
                    'S1_E',
                    OrderSide.BUY,
                    100000,
                    self.ticks[AUDUSD_FXCM].ask - Decimal('0.00005'),
                    time_in_force=TimeInForce.GTD,
                    expire_time=expire_time + timedelta(seconds=10))
                self.entry_orders[entry_order.id] = entry_order
                self.submit_order(entry_order)
                self._log(f"Added {entry_order.id} to entry orders.")

            # SELL LOGIC
            elif self.ema1.value < self.ema2.value:
                entry_order = OrderFactory.limit(
                    AUDUSD_FXCM,
                    self.generate_order_id(AUDUSD_FXCM),
                    'S1_E',
                    OrderSide.SELL,
                    100000,
                    self.ticks[AUDUSD_FXCM].bid + Decimal('0.00005'),
                    time_in_force=TimeInForce.GTD,
                    expire_time=expire_time + timedelta(seconds=10))
                self.entry_orders[entry_order.id] = entry_order
                self.submit_order(entry_order)
                self._log(f"Added {entry_order.id} to entry orders.")

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
                stop_price = event.average_price - Decimal('0.00010') \
                    if stop_side is OrderSide.SELL \
                    else event.average_price + Decimal('0.00010')

                stop_order = OrderFactory.stop_market(
                    AUDUSD_FXCM,
                    self.generate_order_id(AUDUSD_FXCM),
                    'S1_SL',
                    stop_side,
                    event.filled_quantity,
                    stop_price,
                    time_in_force=TimeInForce.GTC)
                self.stop_loss_orders[stop_order.id] = stop_order
                self.submit_order(stop_order)
                self._log(f"Added {stop_order.id} to stop-loss orders.")

    def on_stop(self):
        """
        This method is called when self.stop() is called, and after internal
        stopping logic.

        You could put custom code to clean up existing positions and orders here.
        """
        # Flatten existing positions
        for position in self.positions.values():
            self._log(f"Flattening {position}.")
            order = OrderFactory.market(
                position.symbol,
                self.generate_order_id(position.symbol),
                "FLATTEN",
                self.get_flatten_side(position.market_position),
                position.quantity)
            self.submit_order(order)

        # Cancel all entry orders
        for order in self.entry_orders.values():
            self.cancel_order(order, "STOPPING STRATEGY")

        # Cancel all stop-loss orders
        for order in self.stop_loss_orders.values():
            self.cancel_order(order, "STOPPING STRATEGY")

    def on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.

        Put custom code to be run on a strategy reset here.
        """
        self.entry_orders = {}
        self.stop_loss_orders = {}
