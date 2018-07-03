#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy_examples.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.strategy import TradeStrategy


class ExampleStrategy(TradeStrategy):
    def reset(self):
        pass

    def on_tick(self):
        pass

    def on_bar(self):
        pass

    def on_message(self):
        pass

