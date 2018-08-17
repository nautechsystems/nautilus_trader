#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import time

from decimal import Decimal
from typing import List, Dict

from inv_trader.model.enums import Resolution, QuoteType, OrderSide, TimeInForce, Venue
from inv_trader.model.objects import Symbol, Tick, BarType, Bar
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderFilled, OrderPartiallyFilled, OrderModified
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
        super().__init__(label, order_id_tag='001')

        self.ema1 = ExponentialMovingAverage(fast)
        self.ema2 = ExponentialMovingAverage(slow)

        self.add_indicator(AUDUSD_FXCM_1_SECOND_MID, self.ema1, self.ema1.update, 'ema1')
        self.add_indicator(AUDUSD_FXCM_1_SECOND_MID, self.ema2, self.ema2.update, 'ema2')

        self.entry_orders = {}
        self.stop_loss_orders = {}

    def on_start(self):
        pass

    def on_tick(self, tick: Tick):
        for order in self.entry_orders.values():
            if not order.is_complete:
                if order.side == OrderSide.BUY and tick.bid - Decimal('0.00010') > order.price:
                    self.modify_order(order, tick.bid - Decimal('0.00010'))
                elif order.side == OrderSide.SELL and tick.ask + Decimal('0.00010') < order.price:
                    self.modify_order(order, tick.ask + Decimal('0.00010'))

        for order in self.stop_loss_orders.values():
            if not order.is_complete:
                if order.side == OrderSide.SELL and tick.bid - Decimal('0.00020') > order.price:
                    self.modify_order(order, tick.bid - Decimal('0.00020'))
                elif order.side == OrderSide.BUY and tick.ask + Decimal('0.00020') < order.price:
                    self.modify_order(order, tick.ask + Decimal('0.00020'))

    def on_bar(self, bar_type: BarType, bar: Bar):

        if not self.ema1.initialized and not self.ema2.initialized:
            return

        if bar_type == AUDUSD_FXCM_1_SECOND_MID and AUDUSD_FXCM in self.ticks:
            # If any open positions or pending entry orders then return
            if len(self.positions) > 0:
                return
            if any(order.is_complete is False for order in self.entry_orders.values()):
                return

            # BUY LOGIC
            if self.ema1.value >= self.ema2.value:
                entry_order = OrderFactory.limit(
                    AUDUSD_FXCM,
                    self.generate_order_id(AUDUSD_FXCM),
                    'S1_E',
                    OrderSide.BUY,
                    100000,
                    self.ticks[AUDUSD_FXCM].bid - Decimal('0.00010'))
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
                    self.ticks[AUDUSD_FXCM].ask + Decimal('0.00010'))
                self.entry_orders[entry_order.id] = entry_order
                self.submit_order(entry_order)
                self._log(f"Added {entry_order.id} to entry orders.")

    def on_event(self, event: Event):
        pass
        if isinstance(event, OrderFilled):
            # SET TRAILING STOP
            if event.order_id in self.entry_orders.keys():
                stop_side = self.get_opposite_side(event.order_side)
                stop_price = event.average_price - Decimal('0.00020') if stop_side is OrderSide.SELL else event.average_price + Decimal('0.00020')

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

        # Wait for all orders to cancel
        while (any(order.is_complete is False for order in self.entry_orders.values()) or
               any(order.is_complete is False for order in self.stop_loss_orders.values())):
            self._log("Waiting for orders to cancel...")
            time.sleep(1000)

    def on_reset(self):
        pass
