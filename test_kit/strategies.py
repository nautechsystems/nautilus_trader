#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="objects.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.objects import Tick, BarType, Bar
from inv_trader.enums import Venue, Resolution, QuoteType
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage


class TestStrategy1(TradeStrategy):
    """"
    A simple strategy for unit testing.
    """
    def __init__(self, object_storer):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__()
        self.object_storer = object_storer

        self.gbpusd_1sec_mid = BarType('gbpusd',
                                       Venue.FXCM,
                                       1,
                                       Resolution.SECOND,
                                       QuoteType.MID)

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.add_indicator(self.gbpusd_1sec_mid, self.ema1, self.ema1.update, 'ema1')
        self.add_indicator(self.gbpusd_1sec_mid, self.ema2, self.ema2.update, 'ema2')

    def on_start(self):
        pass

    def on_tick(self, tick: Tick):
        print("tick")

    def on_bar(
            self,
            bar_type: BarType,
            bar: Bar):

        if bar_type == self.gbpusd_1sec_mid:
            if self.ema1.value > self.ema2.value:
                print('BUY')
            elif self.ema1.value < self.ema2.value:
                print('SELL')

    def on_account(self, message):
        pass

    def on_message(self, message):
        pass

    def on_stop(self):
        pass
