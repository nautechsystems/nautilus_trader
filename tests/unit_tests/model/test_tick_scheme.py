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
import pytest

from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick_scheme.base import get_tick_scheme
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up
from nautilus_trader.model.tick_scheme.implementations.tiered import TieredTickScheme
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD")
JPYUSD = TestInstrumentProvider.default_fx_ccy("JPY/USD")


class TestFixedTickScheme:
    def setup(self) -> None:
        self.tick_scheme = get_tick_scheme("FixedTickScheme3Decimal")
        assert self.tick_scheme.price_precision

    @pytest.mark.parametrize(
        "value,precision,expected",
        [
            (0.727775, 4, "0.7277"),
            (0.72777, 4, "0.7277"),
            (0.727741111, 4, "0.7277"),
            (0.799999, 2, "0.79"),
        ],
    )
    def test_round_down(self, value, precision, expected):
        base = 1 * 10 ** -precision
        assert round_down(value, base=base) == Price.from_str(expected).as_double()

    @pytest.mark.parametrize(
        "value,precision,expected",
        [
            (0.72775, 4, "0.7278"),
            (0.7277, 4, "0.7278"),
            (0.727741111, 4, "0.7278"),
            (0.799999, 2, "0.80"),
        ],
    )
    def test_round_up(self, value, precision, expected):
        base = 1 * 10 ** -precision
        assert round_up(value, base) == Price.from_str(expected).as_double()

    def test_attrs(self):
        assert self.tick_scheme.price_precision == 3
        assert self.tick_scheme.min_tick == Price.from_str("0.001")
        assert self.tick_scheme.max_tick == Price.from_str("999.999")

    @pytest.mark.parametrize(
        "value, expected",
        [
            (0.727, "0.728"),
            (0.99999, "1.0000"),
            (0.72775, "0.728"),
            (10000, None),
            (0.0005, "0.001"),
            (0.7271, "0.728"),
        ],
    )
    def test_next_ask_tick(self, value, expected):
        result = self.tick_scheme.next_ask_tick(value)
        if expected is None:
            expected = expected
        else:
            expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            (0.7271, "0.727"),
            (0.001, "0.001"),
            (0.72750, "0.727"),
            (0.00001, None),
            (0.7271, "0.727"),
        ],
    )
    def test_next_bid_tick(self, value, expected):
        result = self.tick_scheme.next_bid_tick(value)
        if expected is None:
            expected = expected
        else:
            expected = Price.from_str(expected)
        assert result == expected


class TestBettingTickScheme:
    def setup(self) -> None:
        self.tick_scheme: TieredTickScheme = get_tick_scheme("BetfairTickScheme")

    def test_attrs(self):
        assert self.tick_scheme.min_tick == Price.from_str("1.01")
        assert self.tick_scheme.max_tick == Price.from_str("990")

    def test_build_ticks(self):
        result = self.tick_scheme.ticks[:5].tolist()
        expected = [Price.from_str(f"1.0{n}") for n in range(1, 6)]
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            (1.005, 0),
            (1.01, 0),
            (2.01, 100),
            (3.50, 159),
        ],
    )
    def test_find_tick_idx(self, value, expected):
        result = self.tick_scheme.find_tick_index(value)
        assert result == expected

    @pytest.mark.parametrize(
        "value, n, expected",
        [
            (1.50, 0, "1.50"),
            (2.0, 0, "2.00"),
            (2.01, 0, "2.02"),
            (2.02, 0, "2.02"),
            (2.02, 2, "2.06"),
        ],
    )
    def test_next_ask_tick(self, value, n, expected):
        result = self.tick_scheme.next_ask_tick(value, n=n)
        expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        "value, n, expected",
        [
            (1.50, 0, "1.50"),
            (2.0, 0, "2.00"),
            (2.001, 0, "2.00"),
            (2.01, 0, "2.00"),
            (2.01, 2, "1.98"),
        ],
    )
    def test_next_bid_tick(self, value, n, expected):
        result = self.tick_scheme.next_bid_tick(value=value, n=n)
        expected = Price.from_str(expected)
        assert result == expected


# class TestTopix100TickScheme:
#     def setup(self) -> None:
#         self.tick_scheme = get_tick_scheme("TOPIX100TickScheme")
#
#     #
#     def test_attrs(self):
#         assert self.tick_scheme.min_tick == Price.from_str("1.01")
#         assert self.tick_scheme.max_tick == Price.from_str("990")
#
#     def test_next_ask_tick_basic(self):
#         # Standard checks
#         result = self.tick_scheme.next_ask_tick(0.7277)
#         expected = Price.from_str("0.7278")
#         assert result == expected
#
#         result = self.tick_scheme.next_ask_tick(0.9999)
#         expected = Price.from_str("1.0000")
#         assert result == expected
#
#     def test_next_ask_price_between_ticks(self):
#         result = self.tick_scheme.next_ask_tick(value=Price.from_str("72775001"))
#         expected = Price.from_str("0.7278")
#         assert result == expected
#
#     def test_next_ask_price_max_tick(self):
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("10000")) is None
#
#     def test_next_ask_price_near_boundary(self):
#         result = self.tick_scheme.next_ask_tick(value=Price.from_str("0.00005"))
#         expected = Price.from_str("0.0001")
#         assert result == expected
#
#     def test_next_bid_tick_basic(self):
#         # Standard checks at change points
#         result = self.tick_scheme.next_bid_tick(value=Price.from_str("0.7277"))
#         expected = Price.from_str("0.7276")
#         assert result == expected
#
#         result = self.tick_scheme.next_ask_tick(value=Price.from_str("1.0001"))
#         expected = Price.from_str("1.0000")
#         assert result == expected
#
#     def test_next_bid_price_between_ticks(self):
#         result = self.tick_scheme.next_ask_tick(value=Price.from_str("72775001"))
#         expected = Price.from_str("0.7277")
#         assert result == expected
