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
from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.common import MAX_BET_PRICE
from nautilus_trader.adapters.betfair.common import MIN_BET_PRICE
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price


class TestBetfairCommon:
    def setup(self):
        self.tick_scheme = BETFAIR_TICK_SCHEME

    def test_min_max_bet(self):
        assert betfair_float_to_price(1000) == MAX_BET_PRICE
        assert betfair_float_to_price(1.01) == MIN_BET_PRICE

    def test_betfair_ticks(self):
        assert self.tick_scheme.min_price == betfair_float_to_price(1.01)
        assert self.tick_scheme.max_price == betfair_float_to_price(1000)
