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

from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick_scheme.base import get_tick_scheme
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up
from nautilus_trader.model.tick_scheme.implementations.fixed import FixedTickScheme
from nautilus_trader.model.tick_scheme.implementations.tiered import TieredTickScheme
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD")
JPYUSD = TestInstrumentProvider.default_fx_ccy("JPY/USD")


class TestFixedTickScheme:
    def setup(self) -> None:
        self.tick_scheme = get_tick_scheme("FOREX_3DECIMAL")
        assert self.tick_scheme.price_precision

    @pytest.mark.parametrize(
        ("value", "precision", "expected"),
        [
            (0.727775, 4, "0.7277"),
            (0.72777, 4, "0.7277"),
            (0.727741111, 4, "0.7277"),
            (0.799999, 2, "0.79"),
        ],
    )
    def test_round_down(self, value, precision, expected):
        base = 1 * 10**-precision
        assert round_down(value, base=base) == Price.from_str(expected).as_double()

    @pytest.mark.parametrize(
        ("value", "precision", "expected"),
        [
            (0.72775, 4, "0.7278"),
            (0.7277, 4, "0.7278"),
            (0.727741111, 4, "0.7278"),
            (0.799999, 2, "0.80"),
        ],
    )
    def test_round_up(self, value, precision, expected):
        base = 1 * 10**-precision
        assert round_up(value, base) == Price.from_str(expected).as_double()

    def test_attrs(self):
        assert self.tick_scheme.price_precision == 3
        assert self.tick_scheme.min_price == Price.from_str("0.001")
        assert self.tick_scheme.max_price == Price.from_str("999.999")
        assert self.tick_scheme.increment == Price.from_str("0.001")

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            (0.727, "0.728"),
            (0.99999, "1.0000"),
            (0.72775, "0.728"),
            (10000, None),
            (0.0005, "0.001"),
            (0.7271, "0.728"),
        ],
    )
    def test_next_ask_price(self, value, expected):
        result = self.tick_scheme.next_ask_price(value)
        expected = expected if expected is None else Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            (0.7271, "0.727"),
            (0.001, "0.001"),
            (0.72750, "0.727"),
            (0.00001, None),
            (0.7271, "0.727"),
        ],
    )
    def test_next_bid_price(self, value, expected):
        result = self.tick_scheme.next_bid_price(value)
        expected = expected if expected is None else Price.from_str(expected)
        assert result == expected


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
