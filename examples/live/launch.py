#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.enums import BarStructure, PriceType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.objects import BarSpecification
from nautilus_trader.live.node import TradingNode

from tests.test_kit.strategies import EMACross
# TODO: AtomicOrder with Market entry not working (needs peg)

# Requirements to run;
#   - A Redis instance listening on the default port 6379
#   - A NautilusData instance listening on the default ports
#   - A NautilusExecutor instance listening on the default ports

BAR_SPEC_FX = BarSpecification(1, BarStructure.MINUTE, PriceType.BID)
BAR_SPEC_CFD = BarSpecification(5, BarStructure.MINUTE, PriceType.BID)

# BAR_SPEC_FX = BarSpecification(100, BarStructure.TICK, PriceType.BID)
# BAR_SPEC_CFD = BarSpecification(500, BarStructure.TICK, PriceType.BID)

symbols_fx = [
    Symbol('AUDUSD', Venue('FXCM')),
    Symbol('EURUSD', Venue('FXCM')),
    Symbol('GBPUSD', Venue('FXCM')),
    Symbol('USDJPY', Venue('FXCM')),
]

strategies_fx = []
for symbol in symbols_fx:
    strategies_fx.append(EMACross(
        symbol,
        BAR_SPEC_FX,
        risk_bp=10.0,
        fast_ema=10,
        slow_ema=20,
        atr_period=20))

# symbols_cfd = [
#     Symbol('XAUUSD', Venue('FXCM')),
#     Symbol('SPX500', Venue('FXCM')),
#     Symbol('AUS200', Venue('FXCM')),
#     Symbol('WTIUSD', Venue('FXCM')),
#     Symbol('DE30EUR', Venue('FXCM')),
# ]

# strategies_cfd = []
# for symbol in symbols_cfd:
#     strategies_fx.append(EMACrossPy(
#         symbol,
#         BAR_SPEC_CFD,
#         risk_bp=10.0,
#         fast_ema=10,
#         slow_ema=20,
#         atr_period=20))

strategies = strategies_fx # + strategies_cfd

if __name__ == "__main__":

    node = TradingNode(
        config_path='config.json',
        strategies=strategies
    )

    input()
    node.connect()

    input()
    node.start()

    input()
    node.stop()

    input()
    node.disconnect()

    input()
    node.dispose()
