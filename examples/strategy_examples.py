#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from inv_trader.model.enums import Resolution, QuoteType, OrderSide, MarketPosition, Venue
from inv_trader.model.objects import Symbol, Tick, BarType, Bar
from inv_trader.model.events import Event, OrderFilled, OrderPartiallyFilled
from inv_trader.factories import OrderFactory
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage

# Constants
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
        pass

    def on_bar(self, bar_type: BarType, bar: Bar):

        if not self.ema1.initialized and not self.ema2.initialized:
            return

        if bar_type == AUDUSD_FXCM_1_SECOND_MID and AUDUSD_FXCM in self.ticks:
            if len(self.positions) == 0:
                if self.ema1.value > self.ema2.value:
                    entry_order = OrderFactory.market(
                        AUDUSD_FXCM,
                        self.generate_order_id(AUDUSD_FXCM),
                        'S1_E',
                        OrderSide.BUY,
                        100000)
                    self.entry_orders[entry_order.id] = entry_order
                    self.submit_order(entry_order)
                    print(f"Added {entry_order.id} to entry orders.")
                elif self.ema1.value < self.ema2.value:
                    entry_order = OrderFactory.market(
                        AUDUSD_FXCM,
                        self.generate_order_id(AUDUSD_FXCM),
                        'S1_E',
                        OrderSide.SELL,
                        100000)
                    self.entry_orders[entry_order.id] = entry_order
                    self.submit_order(entry_order)
                    print(f"Added {entry_order.id} to entry orders.")

    def on_event(self, event: Event):
        if isinstance(event, OrderFilled):
            if event.order_id in self.entry_orders.keys():
                stop_side = self.get_opposite_side(event.order_side)
                stop_price = event.average_price - Decimal('0.00020') if stop_side is OrderSide.SELL else event.average_price + Decimal('0.00020')

                stop_order = OrderFactory.stop_market(
                    AUDUSD_FXCM,
                    self.generate_order_id(AUDUSD_FXCM),
                    'S1_SL',
                    stop_side,
                    event.filled_quantity,
                    stop_price)
                self.stop_loss_orders[stop_order.id] = stop_order
                self.submit_order(stop_order)
                print(f"Added {stop_order.id} to stop-loss orders.")

    def on_stop(self):
        pass

    def on_reset(self):
        pass
