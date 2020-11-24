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

import asyncio
from decimal import Decimal

from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.live.node import TradingNode
from examples.strategies.ema_cross_simple import EMACross


strategy = EMACross(
    symbol=Symbol("ETHUSDT", Venue("BINANCE")),
    bar_spec=BarSpecification(200, BarAggregation.TICK, PriceType.LAST),
    fast_ema=10,
    slow_ema=20,
    trade_size=Decimal(0.1),
)

config = {
    "trader": {
        "name": "TESTER",
        "id_tag": "001",
    },

    "logging": {
        "log_level_console": "INFO",
        "log_level_file": "DEBUG",
        "log_level_store": "WARNING",
    },

    "exec_database": {
        "type": "redis",
        "host": "localhost",
        "port": 6379,
    },

    "strategy": {
        "load_state": True,
        "save_state": True,
    }
}

loop = asyncio.get_event_loop()  # TODO: Implement async run

node = TradingNode(
    loop=loop,
    strategies=[strategy],
    config=config,
)

if __name__ == "__main__":

    node.connect()
    node.start()

    input()

    node.stop()
    node.disconnect()
    node.dispose()
