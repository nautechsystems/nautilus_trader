#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="console_tester_fxcm.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import logging
import zmq

from nautilus_trader.common.logger import LiveLogger
from nautilus_trader.common.account import Account
from nautilus_trader.live.data import LiveDataClient
from nautilus_trader.live.execution import LiveExecClient
from nautilus_trader.model.enums import Resolution, QuoteType, Currency
from nautilus_trader.model.objects import Venue, Symbol, BarType, BarSpecification
from nautilus_trader.trade.portfolio import Portfolio
from nautilus_trader.trade.trader import Trader

from examples.ema_cross import EMACrossPy

AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.MINUTE, QuoteType.BID))
#BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.SECOND, QuoteType.BID))


if __name__ == "__main__":
    zmq_context = zmq.Context()
    logger = LiveLogger(level_console=logging.DEBUG, log_to_file=False)
    data_client = LiveDataClient(zmq_context=zmq_context, venue=Venue('FXCM'), logger=logger)
    exec_client = LiveExecClient(zmq_context=zmq_context, logger=logger)
    data_client.connect()
    exec_client.connect()

    data_client.update_instruments()

    strategy = EMACrossPy(
        AUDUSD_FXCM,
        BAR_TYPE,
        0.1,
        10,
        20,
        20)

    trader = Trader(
        '000',
        [strategy],
        data_client,
        exec_client,
        Account(currency=Currency.USD),
        Portfolio(),
        logger=logger)

    input()
    trader.start()

    input()
    trader.stop()
    data_client.disconnect()
    exec_client.disconnect()

    print("Stopped")
