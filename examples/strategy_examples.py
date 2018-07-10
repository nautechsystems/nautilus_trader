#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.model.enums import Venue, Resolution, QuoteType
from inv_trader.model.objects import Tick, BarType, Bar
from inv_trader.model.events import Event
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage

AUDUSD_FXCM = 'audusd.fxcm'


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
        super().__init__(label)

        self.audusd_fxcm_1_second_mid = BarType(
            'audusd',
            Venue.FXCM,
            1,
            Resolution.SECOND,
            QuoteType.MID)

        self.ema1 = ExponentialMovingAverage(fast)
        self.ema2 = ExponentialMovingAverage(slow)

        self.add_indicator(self.audusd_fxcm_1_second_mid, self.ema1, self.ema1.update, 'ema1')
        self.add_indicator(self.audusd_fxcm_1_second_mid, self.ema2, self.ema2.update, 'ema2')

    def on_start(self):
        pass

    def on_tick(self, tick: Tick):
        pass

    def on_bar(self, bar_type: BarType, bar: Bar):

        if bar_type == self.audusd_fxcm_1_second_mid and AUDUSD_FXCM in self.ticks:
            if self.ema1.value > self.ema2.value:
                print(f"BUY at {self.last_tick('audusd', Venue.FXCM).ask}")
            elif self.ema1.value < self.ema2.value:
                print(f"SELL at {self.last_tick('audusd', Venue.FXCM).bid}")

    def on_event(self, event: Event):
        pass

    def on_stop(self):
        pass

    def on_reset(self):
        pass
