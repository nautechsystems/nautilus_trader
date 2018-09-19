#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_console.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.data import DataClient
from inv_trader.execution import LiveExecClient
from inv_trader.model.enums import Venue, Resolution, QuoteType
from examples.strategy_examples import EMACrossLimitEntry


if __name__ == "__main__":

    strategy = EMACrossLimitEntry('01', 10, 20)
    data_client = DataClient()
    exec_client = LiveExecClient()
    data_client.register_strategy(strategy)
    exec_client.register_strategy(strategy)

    data_client.connect()
    data_client.update_all_instruments()
    data_client.subscribe_ticks('AUDUSD', Venue.FXCM)
    data_client.historical_bars('AUDUSD', Venue.FXCM, 1, Resolution.SECOND, QuoteType.MID)
    data_client.subscribe_bars('AUDUSD', Venue.FXCM, 1, Resolution.SECOND, QuoteType.MID)

    exec_client.connect()
    strategy.start()

    input("Press Enter to stop strategy...\n")
    strategy.stop()

    input("Press Enter to disconnect...\n")
    print("Disconnecting...")
    exec_client.disconnect()
