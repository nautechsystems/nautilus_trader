# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.betfair.common import price_probability_map
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.adapters.betfair.common import round_price
from nautilus_trader.adapters.betfair.common import round_probability
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price


def test_round_probability():
    assert round_probability(0.5, side=OrderSide.BUY) == 0.5
    assert round_probability(0.49999, side=OrderSide.BUY) == 0.49505
    assert round_probability(0.49999, side=OrderSide.SELL) == 0.5
    assert round_probability(0, side=OrderSide.SELL) == 0.001
    assert round_probability(1, side=OrderSide.SELL) == 0.9901


def test_round_price():
    # Test rounding betting prices
    assert round_price(2.0, side=OrderSide.BUY) == 2.0
    assert round_price(2.01, side=OrderSide.BUY) == 2.02
    assert round_price(2.01, side=OrderSide.SELL) == 2.0


def test_price_to_probability():
    # Exact match
    assert price_to_probability(1.69, side=OrderSide.BUY) == Price.from_str("0.59172")
    # Rounding match
    assert price_to_probability(2.01, side=OrderSide.BUY) == Price.from_str("0.49505")
    assert price_to_probability(2.01, side=OrderSide.SELL) == Price.from_str("0.50000")
    # Force for TradeTicks which can have non-tick prices
    assert price_to_probability(10.4, force=True) == Price.from_str("0.09615")


def test_probability_to_price():
    # Exact match
    assert probability_to_price(0.5, side=OrderSide.BUY) == Price.from_str("2.0")
    # Rounding match
    assert probability_to_price(0.499, side=OrderSide.BUY) == Price.from_str("2.02")
    assert probability_to_price(0.501, side=OrderSide.BUY) == Price.from_str("2.0")
    assert probability_to_price(0.501, side=OrderSide.SELL) == Price.from_str("1.99")
    assert probability_to_price(Price.from_str("0.125"), side=OrderSide.BUY) == Price.from_str(
        "8.0"
    )


def test_price_probability_map():
    for prob in price_probability_map.values():
        assert len(prob) == 7
