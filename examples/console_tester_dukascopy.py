#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="console_tester_dukascopy.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from inv_trader.core.logger import Logger
from inv_trader.data import LiveDataClient
from inv_trader.execution import LiveExecClient
from inv_trader.enums import Venue, Resolution, QuoteType
from inv_trader.model.objects import Symbol, BarType
from examples.strategy_examples import EMACrossLimitEntry

AUDUSD = Symbol('AUDUSD', Venue.DUKASCOPY)
AUDUSD_1_SEC_MID = BarType(AUDUSD, 1, Resolution.SECOND, QuoteType.MID)

if __name__ == "__main__":

    logger = Logger(log_to_file=True)
    data_client = LiveDataClient(logger=logger)
    exec_client = LiveExecClient(logger=logger)
    data_client.connect()
    exec_client.connect()

    strategy = EMACrossLimitEntry(
        'AUDUSD-01',
        '001',
        AUDUSD,
        5,
        Decimal('0.00001'),
        AUDUSD_1_SEC_MID,
        100000,
        10,
        20,
        logger=logger)

    data_client.register_strategy(strategy)
    exec_client.register_strategy(strategy)

    strategy.start()

    input("Press Enter to stop strategy...\n")
    strategy.stop()

    input("Press Enter to disconnect...\n")
    print("Disconnecting...")
    exec_client.disconnect()
