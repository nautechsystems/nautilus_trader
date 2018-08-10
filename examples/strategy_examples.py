#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide
from inv_trader.model.objects import Symbol, Tick, BarType, Bar
from inv_trader.model.events import Event
from inv_trader.factories import OrderFactory
from inv_trader.broker.fxcm import FXCMSymbols
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage

# Constants
AUDUSD_FXCM = FXCMSymbols.AUDUSD()
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

    def on_start(self):
        pass

    def on_tick(self, tick: Tick):
        pass

    def on_bar(self, bar_type: BarType, bar: Bar):

        if bar_type == AUDUSD_FXCM_1_SECOND_MID and AUDUSD_FXCM in self.ticks:
            if self.ema1.value > self.ema2.value:
                order = OrderFactory.market(AUDUSD_FXCM,
                                            self.generate_order_id(AUDUSD_FXCM),
                                            'S1_E',
                                            OrderSide.BUY,
                                            100000)
                self.submit_order(order)
            elif self.ema1.value < self.ema2.value:
                order = OrderFactory.market(AUDUSD_FXCM,
                                            self.generate_order_id(AUDUSD_FXCM),
                                            'S1_E',
                                            OrderSide.SELL,
                                            100000)
                self.submit_order(order)

    def on_event(self, event: Event):
        pass

    def on_stop(self):
        pass

    def on_reset(self):
        pass
