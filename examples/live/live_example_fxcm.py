#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="console_tester_fxcm.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.enums import Resolution, QuoteType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.objects import BarType, BarSpecification
from nautilus_trader.live.node import TradingNode

from examples.strategies.ema_cross import EMACrossPy

AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
EURUSD_FXCM = Symbol('EURUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))
USDJPY_FXCM = Symbol('USDJPY', Venue('FXCM'))
WTIUSD_FXCM = Symbol('WTIUSD', Venue('FXCM'))


#BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.MINUTE, QuoteType.BID))
BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.SECOND, QuoteType.BID))


# Requirements to run;
#   - A Redis instance listening on the default port 6379
#   - A NautilusData instance listening on the default ports
#   - A NautilusExecutor instance listening on the default ports

symbols_to_trade = [
    AUDUSD_FXCM,
    EURUSD_FXCM,
    GBPUSD_FXCM,
    USDJPY_FXCM,
    WTIUSD_FXCM
]

if __name__ == "__main__":

    strategies = []
    for symbol in symbols_to_trade:
        strategies.append(EMACrossPy(
            symbol,
            BAR_TYPE,
            0.1,
            10,
            20,
            20))

    node = TradingNode(
        config_path='config.json',
        strategies=strategies)

    node.connect()

    input()
    node.start()

    input()
    node.stop()

    input()
    node.disconnect()
    node.dispose()
