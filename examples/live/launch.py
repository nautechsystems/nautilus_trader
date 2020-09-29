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

from examples.strategies.ema_cross_complex import EMACrossFiltered
from nautilus_trader.enterprise.node import TradingNode
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue

# Requirements to run;
#   - A Redis instance listening on the default port 6379
#   - A NautilusData instance listening on the default ports
#   - A NautilusExecutor instance listening on the default ports

BAR_SPEC_FX = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

symbols_fx = [
    Symbol('AUD/USD', Venue('FXCM')),
    Symbol('AUD/JPY', Venue('FXCM')),
    Symbol('EUR/USD', Venue('FXCM')),
    Symbol('EUR/GBP', Venue('FXCM')),
    Symbol('EUR/JPY', Venue('FXCM')),
    Symbol('GBP/USD', Venue('FXCM')),
    Symbol('GBP/JPY', Venue('FXCM')),
    Symbol('USD/JPY', Venue('FXCM')),
    Symbol('USD/CAD', Venue('FXCM')),
    Symbol('USD/CHF', Venue('FXCM')),
]

news_impacts = ['HIGH', 'MEDIUM']
strategies = []

for symbol in symbols_fx:
    ccy1 = symbol.code[:3]
    ccy2 = symbol.code[-3:]
    strategies.append(EMACrossFiltered(
        symbol,
        BAR_SPEC_FX,
        risk_bp=10.0,
        fast_ema=10,
        slow_ema=20,
        atr_period=20,
        news_currencies=[ccy1, ccy2],
        news_impacts=news_impacts))


if __name__ == "__main__":

    node = TradingNode(
        config_path="config.json",
        strategies=strategies,
    )

    node.connect()
    node.start()

    input()

    node.stop()
    node.disconnect()
    node.dispose()
