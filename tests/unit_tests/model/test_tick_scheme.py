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
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD")
JPYUSD = TestInstrumentProvider.default_fx_ccy("JPY/USD")


class TestFixedTickScheme:
    def setup(self) -> None:
        self.tick_scheme = get_tick_scheme("FixedTickScheme4Decimal")

    @pytest.mark.parametrize(
        "value,precision,expected",
        [
            (0.72775, 4, "0.7277"),
            (0.7277, 4, "0.7277"),
            (0.727741111, 4, "0.7277"),
            (0.799999, 2, "0.79"),
        ],
    )
    def test_round_down(self, value, precision, expected):
        assert round_down(value, precision) == Price.from_str(expected)

    @pytest.mark.parametrize(
        "value,precision,expected",
        [
            (0.72775, 4, "0.7278"),
            (0.7277, 4, "0.7277"),
            (0.727741111, 4, "0.7278"),
            (0.799999, 2, "0.80"),
        ],
    )
    def test_round_up(self, value, precision, expected):
        assert round_up(value, precision) == Price.from_str(expected)

    def test_attrs(self):
        assert self.tick_scheme.price_precision == 4
        assert self.tick_scheme.min_tick == Price.from_str("0.0001")
        assert self.tick_scheme.max_tick == Price.from_str("9.9999")

    def test_next_ask_tick_basic(self):
        # Standard checks
        result = self.tick_scheme.next_ask_tick(0.7277)
        expected = Price.from_str("0.7278")
        assert result == expected

        result = self.tick_scheme.next_ask_tick(0.9999)
        expected = Price.from_str("1.0000")
        assert result == expected

    def test_next_ask_price_between_ticks(self):
        result = self.tick_scheme.next_ask_tick(price=Price.from_str("72775001"))
        expected = Price.from_str("0.7278")
        assert result == expected

    def test_next_ask_price_max_tick(self):
        assert self.tick_scheme.next_ask_tick(price=Price.from_str("10000")) is None

    def test_next_ask_price_near_boundary(self):
        result = self.tick_scheme.next_ask_tick(price=Price.from_str("0.00005"))
        expected = Price.from_str("0.0001")
        assert result == expected

    def test_next_bid_tick_basic(self):
        # Standard checks at change points
        result = self.tick_scheme.next_bid_tick(price=Price.from_str("0.7277"))
        expected = Price.from_str("0.7276")
        assert result == expected

        result = self.tick_scheme.next_ask_tick(price=Price.from_str("1.0001"))
        expected = Price.from_str("1.0000")
        assert result == expected

    def test_next_bid_price_between_ticks(self):
        result = self.tick_scheme.next_ask_tick(price=Price.from_str("72775001"))
        expected = Price.from_str("0.7277")
        assert result == expected


class TestBettingTickScheme:
    def setup(self) -> None:
        self.tick_scheme = get_tick_scheme("BetfairTickScheme")

    def test_attrs(self):
        assert self.tick_scheme.min_tick == Price.from_str("1.01")
        assert self.tick_scheme.max_tick == Price.from_str("990")

    def test_next_ask_tick(self):
        # Standard checks at switch points
        assert self.tick_scheme.next_ask_tick(value=Price.from_str("0.01")) == Price.from_str(
            "0.02"
        )
        assert self.tick_scheme.next_ask_tick(value=Price.from_str("1.0")) == Price.from_str("1.10")
        assert self.tick_scheme.next_ask_tick(value=Price.from_str("9.90")) == Price.from_str(
            "10.0"
        )
        assert self.tick_scheme.next_ask_tick(value=Price.from_str("10.0")) == Price.from_str(
            "10.50"
        )
        # Check prices within ticks still work as expected
        assert self.tick_scheme.next_ask_tick(value=Price.from_str("10.25")) == Price.from_str(
            "10.50"
        )

        # Check tick boundary (max tick is 100.0)
        assert self.tick_scheme.next_ask_tick(value=Price.from_str("99.50")) is None

        # Check near tick boundary
        assert self.tick_scheme.next_ask_tick(value=Price.from_str("99.49")) == Price.from_str(
            "99.50"
        )

    def test_next_bid_tick(self):
        # Standard checks at change points
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.02")) == Price.from_str(
            "0.01"
        )
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("1.10")) == Price.from_str(
            "1.00"
        )
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.0")) == Price.from_str(
            "9.90"
        )
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.50")) == Price.from_str(
            "10.00"
        )

        # Check prices within ticks still work as expected
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.25")) == Price.from_str(
            "10.00"
        )

        # Check tick boundary (min tick is 0.01)
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.01")) is None
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.005")) is None

        # Check near tick boundary
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.015")) == Price.from_str(
            "0.01"
        )

    def test_nearest_bid_tick(self):
        # Standard checks at change points
        assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("0.001")) == Price.from_str(
            "0.01"
        )
        assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("0.01")) == Price.from_str(
            "0.01"
        )
        assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("0.015")) == Price.from_str(
            "0.01"
        )
        assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("10.0")) == Price.from_str(
            "9.90"
        )
        assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("10.50")) == Price.from_str(
            "10.00"
        )

        # Check prices within ticks still work as expected
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.25")) == Price.from_str(
            "10.00"
        )

        # Check tick boundary (min tick is 0.01)
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.01")) is None
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.005")) is None

        # Check near tick boundary
        assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.015")) == Price.from_str(
            "0.01"
        )


#
class TestTopix100TickScheme:
    def setup(self) -> None:
        self.tick_scheme = get_tick_scheme("TOPIX100TickScheme")

    #
    def test_attrs(self):
        assert self.tick_scheme.min_tick == Price.from_str("1.01")
        assert self.tick_scheme.max_tick == Price.from_str("990")


#
#     def test_next_ask_tick(self):
#         # Standard checks at switch points
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("0.01")) == Price.from_str(
#             "0.02"
#         )
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("1.0")) == Price.from_str("1.10")
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("9.90")) == Price.from_str(
#             "10.0"
#         )
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("10.0")) == Price.from_str(
#             "10.50"
#         )
#         # Check prices within ticks still work as expected
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("10.25")) == Price.from_str("10.50")
#
#         # Check tick boundary (max tick is 100.0)
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("99.50")) is None
#
#         # Check near tick boundary
#         assert self.tick_scheme.next_ask_tick(value=Price.from_str("99.49")) == Price.from_str("99.50")
#
#     def test_next_bid_tick(self):
#         # Standard checks at change points
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.02")) == Price.from_str("0.01")
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("1.10")) == Price.from_str("1.00")
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.0")) == Price.from_str("9.90")
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.50")) == Price.from_str("10.00")
#
#         # Check prices within ticks still work as expected
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.25")) == Price.from_str("10.00")
#
#         # Check tick boundary (min tick is 0.01)
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.01")) is None
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.005")) is None
#
#         # Check near tick boundary
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.015")) == Price.from_str("0.01")
#
#     def test_nearest_bid_tick(self):
#         # Standard checks at change points
#         assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("0.001")) == Price.from_str("0.01")
#         assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("0.01")) == Price.from_str("0.01")
#         assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("0.015")) == Price.from_str("0.01")
#         assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("10.0")) == Price.from_str("9.90")
#         assert self.tick_scheme.nearest_bid_tick(value=Price.from_str("10.50")) == Price.from_str("10.00")
#
#         # Check prices within ticks still work as expected
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("10.25")) == Price.from_str("10.00")
#
#         # Check tick boundary (min tick is 0.01)
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.01")) is None
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.005")) is None
#
#         # Check near tick boundary
#         assert self.tick_scheme.next_bid_tick(value=Price.from_str("0.015")) == Price.from_str("0.01")
