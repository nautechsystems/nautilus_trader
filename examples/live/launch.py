#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.enums import BarStructure, PriceType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.objects import BarSpecification
from nautilus_trader.live.node import TradingNode

from examples.strategies.ema_cross import EMACross
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
    Symbol('AUD/USD', Venue('FXCM')),
    Symbol('EUR/USD', Venue('FXCM')),
    Symbol('GBP/USD', Venue('FXCM')),
    Symbol('USD/JPY', Venue('FXCM')),
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
#     Symbol('USOil', Venue('FXCM')),
#     Symbol('GER30', Venue('FXCM')),
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

strategies = strategies_fx  # + strategies_cfd

if __name__ == "__main__":

    node = TradingNode(
        config_path='config.json',
        strategies=strategies
    )

    node.connect()
    node.start()

    input()

    node.stop()
    node.disconnect()
    node.dispose()
