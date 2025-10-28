# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.common import MAX_BET_PRICE
from nautilus_trader.adapters.betfair.common import MIN_BET_PRICE
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing.core import create_sequence_completed
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument


class TestBetfairCommon:
    def setup(self):
        self.tick_scheme = BETFAIR_TICK_SCHEME

    def test_min_max_bet(self):
        assert betfair_float_to_price(1000) == MAX_BET_PRICE
        assert betfair_float_to_price(1.01) == MIN_BET_PRICE

    def test_betfair_ticks(self):
        assert self.tick_scheme.min_price == betfair_float_to_price(1.01)
        assert self.tick_scheme.max_price == betfair_float_to_price(1000)


class TestBettingInstrument:
    def setup(self):
        self.instrument = betting_instrument()

    def test_notional_value(self):
        notional = self.instrument.notional_value(
            quantity=Quantity.from_int(100),
            price=Price.from_str("0.5"),
            use_quote_for_inverse=False,
        ).as_decimal()
        # We are long 100 at 0.5 probability, aka 2.0 in odds terms
        assert notional == Decimal("100.0")

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (101, 0, "110"),
        ],
    )
    def test_next_ask_price(self, value, n, expected):
        result = self.instrument.next_ask_price(value, num_ticks=n)
        expected = Price.from_str(expected)
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "n", "expected"),
        [
            (1.999, 0, "1.99"),
        ],
    )
    def test_next_bid_price(self, value, n, expected):
        result = self.instrument.next_bid_price(value, num_ticks=n)
        expected = Price.from_str(expected)
        assert result == expected

    def test_min_max_price(self):
        assert self.instrument.min_price == Price.from_str("1.01")
        assert self.instrument.max_price == Price.from_str("1000")

    def test_to_dict(self):
        instrument = betting_instrument()
        data = instrument.to_dict(instrument)
        assert data["venue_name"] == "BETFAIR"
        new_instrument = BettingInstrument.from_dict(data)
        assert instrument == new_instrument

    @pytest.mark.parametrize(
        "price, quantity, expected",
        [
            (5.0, 100.0, 100),
            (1.50, 100.0, 100),
            (5.0, 100.0, 100),
            (5.0, 300.0, 300),
        ],
    )
    def test_betting_instrument_notional_value(self, price, quantity, expected):
        notional = self.instrument.notional_value(
            price=betfair_float_to_price(price),
            quantity=betfair_float_to_quantity(quantity),
        ).as_double()
        assert notional == expected


def test_betfair_sequence_completed_str_repr() -> None:
    # Arrange
    completed = create_sequence_completed(2, 1)

    # Act, Assert
    assert (
        str(completed)
        == "CustomData(data_type=BetfairSequenceCompleted, data=BetfairSequenceCompleted(ts_event=1970-01-01T00:00:00.000000002Z, ts_init=1970-01-01T00:00:00.000000001Z))"
    )
    assert (
        repr(completed)
        == "CustomData(data_type=BetfairSequenceCompleted, data=BetfairSequenceCompleted(ts_event=1970-01-01T00:00:00.000000002Z, ts_init=1970-01-01T00:00:00.000000001Z))"
    )
