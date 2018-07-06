#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.enums import Venue, Resolution, QuoteType
from inv_trader.objects import Tick, BarType, Bar
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage


class EMACross(TradeStrategy):
    """"
    A simple moving average crossing example strategy.
    """
    def __init__(self, fast, slow):
        """
        Initializes a new instance of the EMACross class.
        """
        super().__init__()

        self.bar_type = BarType(
            'audusd',
            Venue.FXCM,
            1,
            Resolution.SECOND,
            QuoteType.MID)

        self.ema1 = ExponentialMovingAverage(fast)
        self.ema2 = ExponentialMovingAverage(slow)

        self.add_indicator(self.bar_type, self.ema1, self.ema1.update, 'ema1')
        self.add_indicator(self.bar_type, self.ema2, self.ema2.update, 'ema2')

    def on_start(self):
        pass

    def on_tick(self, tick: Tick):
        print(str(tick))

    def on_bar(
            self,
            bar_type: BarType,
            bar: Bar):

        if bar_type == self.bar_type:
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
