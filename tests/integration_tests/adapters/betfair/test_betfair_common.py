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

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.common import MAX_BET_PROB
from nautilus_trader.adapters.betfair.common import MIN_BET_PROB
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.model.objects import Price


class TestBetfairCommon:
    def setup(self):
        self.tick_scheme = BETFAIR_TICK_SCHEME

    def test_min_max_bet(self):
        assert MAX_BET_PROB == Price.from_str("0.9900990")
        assert MIN_BET_PROB == Price.from_str("0.0010000")

    def test_betfair_ticks(self):
        assert self.tick_scheme.min_price == Price.from_str("0.0010000")
        assert self.tick_scheme.max_price == Price.from_str("0.9900990")

    @pytest.mark.parametrize(
        "price, prob",
        [
            # Exact match
            ("1.69", "0.591716"),
            # Rounding match
            ("2.02", "0.4950495"),
            ("2.005", "0.50000"),
            # Force for TradeTicks which can have non-tick prices
            ("10.4", "0.0952381"),
        ],
    )
    def test_price_to_probability(self, price, prob):
        result = price_to_probability(price)
        expected = Price.from_str(prob)
        assert result == expected

    @pytest.mark.parametrize(
        "raw_prob, price",
        [
            (0.5, "2.0"),
            (0.499, "2.02"),
            (0.501, "2.0"),
            (0.503, "1.99"),
            (0.125, "8.0"),
        ],
    )
    def test_probability_to_price(self, raw_prob, price):
        # Exact match
        prob = self.tick_scheme.next_bid_price(raw_prob)
        assert probability_to_price(prob) == Price.from_str(price)
