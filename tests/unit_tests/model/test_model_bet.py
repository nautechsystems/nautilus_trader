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

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.model.data.bet import Bet
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class DecimaL:
    pass


class TestBet:
    def setup(self):
        self.instrument = TestInstrumentProvider.betting_instrument
        self.price = Price.from_str("2.0")
        self.size = Quantity.from_int(10)

    def test_bet_equality(self):
        # Arrange
        bet1 = Bet(price=self.price, quantity=self.size, side=OrderSide.BUY)
        bet2 = Bet(price=self.price, quantity=self.size, side=OrderSide.BUY)
        bet3 = Bet(price=self.price, quantity=self.size, side=OrderSide.SELL)

        # Act, Assert
        assert bet1 == bet1
        assert bet1 == bet2
        assert bet1 != bet3

    def test_bet_hash_str_and_repr(self):
        # Arrange
        bet = Bet(price=self.price, quantity=self.size, side=OrderSide.BUY)

        # Act, Assert
        assert isinstance(hash(bet), int)
        assert str(bet) == "1,2.0,10"
        assert repr(bet) == "Bet(1,2.0,10)"

    @pytest.mark.parametrize(
        "price, size, side, expected",
        [
            ("1.50", 10, "BUY", 5),
            ("2", 10, "BUY", 10),
            ("10", 10, "BUY", 90),
            ("1.50", 10, "SELL", -5),
            ("2", 10, "SELL", -10),
            ("10", 10, "SELL", -90),
        ],
    )
    def test_win_payoff(self, price, size, side, expected):
        side = getattr(OrderSide, side)
        result = Bet(
            price=Price.from_str(price),
            quantity=Quantity.from_int(size),
            side=side,
        ).win_payoff()
        assert result == expected

    @pytest.mark.parametrize(
        "price, size, side, expected",
        [
            ("1.50", 10, "BUY", -10),
            ("2", 10, "BUY", -10),
            ("10", 10, "BUY", -10),
            ("1.50", 10, "SELL", 10),
            ("2", 10, "SELL", 10),
            ("10", 10, "SELL", 10),
        ],
    )
    def test_lose_payoff(self, price, size, side, expected):
        side = getattr(OrderSide, side)
        result = Bet(
            price=Price.from_str(price),
            quantity=Quantity.from_int(size),
            side=side,
        ).lose_payoff()
        assert result == expected

    @pytest.mark.parametrize(
        "price, size, side, expected",
        [
            ("1.50", 10, "BUY", 15),
            ("2", 10, "BUY", 20),
            ("10", 10, "BUY", 100),
            ("1.50", 10, "SELL", -15),
            ("2", 10, "SELL", -20),
            ("10", 10, "SELL", -100),
        ],
    )
    def test_exposure(self, price, size, side, expected):
        side = getattr(OrderSide, side)
        result = Bet(
            price=Price.from_str(price),
            quantity=Quantity.from_int(size),
            side=side,
        ).exposure()
        assert result == expected
