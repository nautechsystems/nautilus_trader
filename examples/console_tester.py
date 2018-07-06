#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_console.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.data import LiveDataClient
from inv_trader.enums import Venue, Resolution, QuoteType
from examples.strategy_examples import EMACross

# Tests the live data client can receive ticks and bars.
if __name__ == "__main__":
    client = LiveDataClient()
    client.connect()
    client.subscribe_tick_data('audusd', Venue.FXCM)
    client.subscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.MID)


if __name__ == "__main__":
    strategy = EMACross('01', 10, 20)
    client = LiveDataClient()
    client.connect()
    client.register_strategy(strategy)
    client.subscribe_tick_data('audusd', Venue.FXCM)
    client.subscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.MID)
    strategy.start()
