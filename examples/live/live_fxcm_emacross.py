#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="console_tester_fxcm.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.enums import BarStructure, PriceType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.objects import BarSpecification
from nautilus_trader.live.node import TradingNode

from examples.strategies.ema_cross import EMACrossPy
# TODO: AtomicOrder with Market entry not working (needs peg)

# Requirements to run;
#   - A Redis instance listening on the default port 6379
#   - A NautilusData instance listening on the default ports
#   - A NautilusExecutor instance listening on the default ports


BAR_SPEC = BarSpecification(1, BarStructure.MINUTE, PriceType.BID)

symbols_to_trade = [
    Symbol('AUDUSD', Venue('FXCM')),
    Symbol('EURUSD', Venue('FXCM')),
    Symbol('GBPUSD', Venue('FXCM')),
    Symbol('USDJPY', Venue('FXCM')),
]


if __name__ == "__main__":

    strategies = []
    for symbol in symbols_to_trade:
        strategies.append(EMACrossPy(
            symbol,
            BAR_SPEC,
            risk_bp=10.0,
            fast_ema=1,
            slow_ema=2,
            atr_period=4))

    node = TradingNode(
        config_path='config.json',
        strategies=strategies)

    node.connect()
    node.start()

    input()
    node.stop()

    input()
    node.disconnect()
    node.dispose()
