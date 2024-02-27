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

import pytest

from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick_scheme.base import get_tick_scheme
from nautilus_trader.model.tick_scheme.implementations.fixed import FixedTickScheme
from nautilus_trader.model.tick_scheme.implementations.tiered import TieredTickScheme


class TestBettingTickScheme:
    def setup(self) -> None:
        self.tick_scheme: TieredTickScheme = get_tick_scheme("BETFAIR")

    def test_attrs(self):
        assert self.tick_scheme.min_price == Price.from_str("1.01")
        assert self.tick_scheme.max_price == Price.from_str("1000")

    def test_build_ticks(self):
        result = self.tick_scheme.ticks[:5].tolist()
        expected = [
            Price.from_str("1.01"),
            Price.from_str("1.02"),
            Price.from_str("1.03"),
            Price.from_str("1.04"),
            Price.from_str("1.05"),
        ]
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            (1.01, 0),
            (1.10, 9),
            (2.0, 99),
            (3.5, 159),
        ],
    )
    def test_find_tick_idx(self, value, expected):
        result = self.tick_scheme.find_tick_index(value)
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            (3.90, Price.from_str("3.95")),
            (4.0, Price.from_str("4.10")),
        ],
    )
    def test_tick_price_precision(self, value, expected):
        result = self.tick_scheme.next_ask_price(value, n=1)
        assert result == expected
        assert result.precision == 2

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (1.499, 0, "1.50"),
            (2.000, 0, "2.0"),
            (2.011, 0, "2.02"),
            (2.021, 0, "2.04"),
            (2.027, 2, "2.08"),
        ],
    )
    def test_next_ask_price(self, value, n, expected):
        result = self.tick_scheme.next_ask_price(value, n=n)
        expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (1.499, 0, "1.49"),
            (2.000, 0, "2.0"),
            (2.011, 0, "2.00"),
            (2.021, 0, "2.02"),
            (2.027, 2, "1.99"),
        ],
    )
    def test_next_bid_price(self, value, n, expected):
        result = self.tick_scheme.next_bid_price(value=value, n=n)
        expected = Price.from_str(expected)
        assert result == expected


class TestTopix100TickScheme:
    def setup(self) -> None:
        self.tick_scheme = get_tick_scheme("TOPIX100")

    def test_attrs(self):
        assert self.tick_scheme.min_price == Price.from_str("0.1")
        assert self.tick_scheme.max_price == Price.from_int(130_000_000)

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (1000, 0, "1000"),
            (1000.25, 0, "1000.50"),
            (10_001, 0, "10_005"),
            (10_000_001, 0, "10_005_000"),
            (9999, 2, "10_005"),
        ],
    )
    def test_next_ask_price(self, value, n, expected):
        result = self.tick_scheme.next_ask_price(value, n=n)
        expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            # (1000, 0, "1000"),  # TODO: Fails with 999.9
            (1000.75, 0, "1000.50"),
            (10_007, 0, "10_005"),
            (10_000_001, 0, "10_000_000"),
            (10_006, 2, "9999"),
        ],
    )
    def test_next_bid_price(self, value, n, expected):
        result = self.tick_scheme.next_bid_price(value=value, n=n)
        expected = Price.from_str(expected)
        assert result == expected


class TestBitmexSpotTickScheme:
    def setup(self) -> None:
        self.tick_scheme = FixedTickScheme(
            name="BitmexSpot",
            price_precision=1,
            increment=0.50,
            min_tick=Price.from_str("0.001"),
            max_tick=Price.from_str("999.999"),
        )

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (10.1, 0, "10.5"),
        ],
    )
    def test_next_ask_price(self, value, n, expected):
        result = self.tick_scheme.next_ask_price(value, n=n)
        expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (10.1, 0, "10.0"),
        ],
    )
    def test_next_bid_price(self, value, n, expected):
        result = self.tick_scheme.next_bid_price(value=value, n=n)
        expected = Price.from_str(expected)
        assert result == expected
