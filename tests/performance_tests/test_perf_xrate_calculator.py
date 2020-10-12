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

import unittest

from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import PriceType
from nautilus_trader.trading.calculators import ExchangeRateCalculator
from tests.test_kit.performance import PerformanceHarness


class ExchangeRateOperations:

    @staticmethod
    def get_xrate():
        bid_quotes = {
            'BTC/USD': 11291.38,
            'ETH/USDT': 371.90,
            'XBT/USD': 11285.50,
        }

        ask_quotes = {
            'BTC/USD': 11292.58,
            'ETH/USDT': 372.11,
            'XBT/USD': 11286.00,
        }

        ExchangeRateCalculator().get_rate(
            from_currency=ETH,
            to_currency=USDT,
            price_type=PriceType.MID,
            bid_quotes=bid_quotes,
            ask_quotes=ask_quotes,
        )


class ExchangeRateCalculatorPerformanceTests(unittest.TestCase):

    def test_get_xrate(self):
        result = PerformanceHarness.profile_function(ExchangeRateOperations.get_xrate, 3, 10000)
        # ~43ms (43821μs) minimum of 3 runs @ 10,000 iterations each run.
        self.assertTrue(result < 0.2)
