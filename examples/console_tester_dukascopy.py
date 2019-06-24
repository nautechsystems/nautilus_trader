#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="console_tester_dukascopy.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logger import Logger
from nautilus_trader.network.data import LiveDataClient
from nautilus_trader.network.execution import LiveExecClient
from nautilus_trader.enums import Venue, Resolution, QuoteType
from nautilus_trader.model.objects import Symbol, BarType
from test_kit.strategies import EMACross

AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
AUDUSD_FXCM_1_SEC_MID = BarType(AUDUSD_FXCM, 1, Resolution.SECOND, QuoteType.MID)


if __name__ == "__main__":

    logger = Logger(log_to_file=False)
    data_client = LiveDataClient(logger=logger)
    exec_client = LiveExecClient(logger=logger)
    data_client.connect()
    exec_client.connect()

    data_client.update_all_instruments()

    instrument = data_client.get_instrument(AUDUSD_FXCM)
    strategy = EMACross(
        'AUDUSD-01',
        '001',
        instrument,
        AUDUSD_FXCM_1_SEC_MID,
        1,
        10,
        20,
        20,
        2.0,
        1000,
        logger=logger)

    data_client.register_strategy(strategy)
    exec_client.register_strategy(strategy)

    strategy.start()

    input("Press Enter to stop strategy...\n")
    strategy.stop()

    input("Press Enter to disconnect...\n")
    print("Disconnecting...")
    exec_client.disconnect()
