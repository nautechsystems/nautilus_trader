# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model import Price
from nautilus_trader.model import TieredTickScheme
from nautilus_trader.model import get_tick_scheme
from nautilus_trader.model import list_tick_schemes
from nautilus_trader.model import register_tick_scheme


def test_register_tiered_tick_scheme():
    scheme = TieredTickScheme(
        name="TEST_PYO3_ES_OPTIONS",
        tiers=[
            (0.05, 10.00, 0.05),
            (10.00, float("inf"), 0.25),
        ],
        price_precision=2,
        max_ticks_per_tier=1000,
    )

    register_tick_scheme(scheme)
    registered = get_tick_scheme("TEST_PYO3_ES_OPTIONS")

    assert registered.name == "TEST_PYO3_ES_OPTIONS"
    assert registered.next_bid_price(9.99) == Price.from_str("9.95")
    assert registered.next_ask_price(9.99) == Price.from_str("10.00")
    assert registered.next_bid_price(10.63) == Price.from_str("10.50")
    assert registered.next_ask_price(10.63) == Price.from_str("10.75")
    assert list_tick_schemes() == sorted(list_tick_schemes())
    assert "TEST_PYO3_ES_OPTIONS" in list_tick_schemes()

    with pytest.raises(ValueError, match="already registered"):
        register_tick_scheme(scheme)
