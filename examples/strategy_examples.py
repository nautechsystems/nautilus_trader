#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.objects import Tick, BarType, Bar
from inv_trader.strategy import TradeStrategy


class ExampleStrategy(TradeStrategy):
    def on_start(self):
        pass

    def on_tick(self, tick: Tick):
        print("got a tick")

    def on_bar(
            self,
            bar_type: BarType,
            bar: Bar):
        print("got a bar")

    def on_account(self, message):
        pass

    def on_message(self):
        pass

    def on_stop(self):
        pass
