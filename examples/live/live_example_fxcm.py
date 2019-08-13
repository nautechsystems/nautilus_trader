#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="console_tester_fxcm.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.enums import Resolution, QuoteType
from nautilus_trader.model.objects import Venue, Symbol, BarType, BarSpecification
from nautilus_trader.live.node import TradingNode

from examples.strategies.ema_cross import EMACrossPy

AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
#BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.MINUTE, QuoteType.BID))
BAR_TYPE = BarType(AUDUSD_FXCM, BarSpecification(1, Resolution.SECOND, QuoteType.BID))


# Requirements to run;
#   - A Redis instance listening on the default port 6379
#   - A NautilusData instance listening on the default ports
#   - A NautilusExecutor instance listening on the default ports


if __name__ == "__main__":
    strategy = EMACrossPy(
        AUDUSD_FXCM,
        BAR_TYPE,
        0.1,
        10,
        20,
        20)

    node = TradingNode(strategies=[strategy])

    node.connect()
    node.start()

    input()
    node.stop()
    node.disconnect()
    node.dispose()
