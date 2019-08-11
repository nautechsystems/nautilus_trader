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
from nautilus_trader.live.node import TradingNode

from examples.strategies.ema_cross import EMACrossPy

AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.MINUTE, QuoteType.BID))
#BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.SECOND, QuoteType.BID))


if __name__ == "__main__":
    node = TradingNode()

    input()
    # trader.start()

    input()
    # trader.stop()
    # data_client.disconnect()
    # exec_client.disconnect()
