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

from nautilus_trader.accounting.calculators import ExchangeRateCalculator
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import PriceType
from nautilus_trader.test_kit.performance import PerformanceBench


class TestExchangeRateCalculatorPerformanceTests:
    @staticmethod
    def get_xrate():
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
        ExchangeRateCalculator().get_rate(
            from_currency=ETH,
            to_currency=USDT,
            price_type=PriceType.MID,
            bid_quotes=bid_quotes,
            ask_quotes=ask_quotes,
        )

    def test_get_xrate(self):
        PerformanceBench.profile_function(
            target=self.get_xrate,
            runs=100_000,
            iterations=1,
        )
        # ~0.0ms / ~8.2μs / 8198ns minimum of 100,000 runs @ 1 iteration each run.
        # ~0.0ms / ~4.7μs / 4732ns minimum of 100,000 runs @ 1 iteration each run.
