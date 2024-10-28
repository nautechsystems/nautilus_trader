# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.accounting.calculators import ExchangeRateCalculator
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import PriceType


def test_get_rate(benchmark):
    bid_quotes = {
        "BTC/USD": 11291.38,
        "ETH/USDT": 371.90,
        "XBT/USD": 11285.50,
    }

    ask_quotes = {
        "BTC/USD": 11292.58,
        "ETH/USDT": 372.11,
        "XBT/USD": 11286.0,
    }

    calculator = ExchangeRateCalculator()

    benchmark(calculator.get_rate, ETH, USDT, PriceType.MID, bid_quotes, ask_quotes)
