#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

# from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.fixtures.memory import snapshot_memory

# from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


@snapshot_memory(1000)
def run(*args, **kwargs):
    # provider = TestDataProvider()
    # wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
    # _ = wrangler.process(provider.read_csv_ticks("binance-ethusdt-trades.csv"))
    _ = TradeTick(
        ETHUSDT_BINANCE.id,
        Price.from_str("1.00000"),
        Quantity.from_str("1"),
        AggressorSide.BUYER,
        TradeId("123456789"),
        1,
        2,
    )


if __name__ == "__main__":
    run()
