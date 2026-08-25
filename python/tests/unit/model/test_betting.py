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
"""
Test betting behavior.
"""

from decimal import Decimal

import pytest

from nautilus_trader.model import Bet
from nautilus_trader.model import BetPosition
from nautilus_trader.model import BetSide
from nautilus_trader.model import OrderSide
from nautilus_trader.model import calc_bets_pnl
from nautilus_trader.model import inverse_probability_to_bet
from nautilus_trader.model import probability_to_bet


def test_bet_properties_and_payoffs() -> None:
    """
    Test bet properties and payoffs.
    """
    bet = Bet(Decimal("2.5"), Decimal(10), BetSide.BACK)

    assert bet.price == Decimal("2.5")
    assert bet.stake == Decimal(10)
    assert bet.side == BetSide.BACK
    assert bet.exposure() == Decimal("25.0")
    assert bet.liability() == Decimal(10)
    assert bet.profit() == Decimal("15.0")
    assert bet.outcome_win_payoff() == Decimal("15.0")
    assert bet.outcome_lose_payoff() == Decimal(-10)
    assert bet.hedging_bet(Decimal("1.5")).side == BetSide.LAY


def test_bet_factories() -> None:
    """
    Test bet factories.
    """
    back = Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK)
    lay = Bet.from_liability(Decimal("2.5"), Decimal(15), BetSide.LAY)

    assert back.stake == Decimal(10)
    assert back.side == BetSide.BACK
    assert lay.side == BetSide.LAY
    assert lay.stake == Decimal(10)


def test_bet_position_add_bets_and_reset() -> None:
    """
    Test bet position add bets and reset.
    """
    position = BetPosition()
    position.add_bet(Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK))
    position.add_bet(Bet.from_stake(Decimal("3.0"), Decimal(5), BetSide.LAY))

    assert position.side == BetSide.BACK
    assert position.as_bet() is not None
    assert position.flattening_bet(Decimal("2.2")) is not None
    assert position.total_pnl(Decimal("2.2")) == Decimal("-2.7272727272727272727272727272")

    position.reset()

    assert position.side is None
    assert position.exposure == Decimal(0)


def test_betting_helpers_create_expected_bets() -> None:
    """
    Test betting helpers create expected bets.
    """
    probability_bet = probability_to_bet(Decimal("0.4"), Decimal(10), OrderSide.BUY)
    inverse_bet = inverse_probability_to_bet(Decimal("0.4"), Decimal(10), OrderSide.SELL)

    assert probability_bet.side == BetSide.BACK
    assert probability_bet.price == Decimal("2.5")
    assert probability_bet.stake == Decimal(4)
    assert inverse_bet.side == BetSide.BACK
    assert inverse_bet.price > Decimal(1)
    assert inverse_bet.stake > Decimal(0)


def test_calc_bets_pnl() -> None:
    """
    Test calc bets pnl.
    """
    bets = [
        Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK),
        Bet.from_liability(Decimal("2.5"), Decimal(15), BetSide.LAY),
    ]

    assert calc_bets_pnl(bets) == Decimal("-5.0")
    assert calc_bets_pnl(bets) == sum((bet.outcome_win_payoff() for bet in bets), Decimal(0))


def test_bet_side_helpers() -> None:
    """
    Test bet side helpers.
    """
    assert BetSide.from_str("BACK") == BetSide.BACK
    assert BetSide.from_order_side(OrderSide.BUY) == BetSide.BACK
    assert BetSide.BACK.opposite() == BetSide.LAY


def test_from_liability_rejects_back_side() -> None:
    """
    Test from liability rejects back side.
    """
    with pytest.raises(ValueError, match="Liability-based betting is only applicable for Lay side"):
        Bet.from_liability(Decimal("2.0"), Decimal(100), BetSide.BACK)


@pytest.mark.parametrize("price", [Decimal(1), Decimal(0), Decimal(-1)])
def test_from_liability_rejects_odds_at_or_below_one(price: Decimal) -> None:
    """
    Test from liability rejects odds at or below one.
    """
    with pytest.raises(ValueError, match=r"Price must be greater than 1\.0"):
        Bet.from_liability(price, Decimal(100), BetSide.LAY)


def test_from_stake_or_liability_rejects_lay_odds_at_one() -> None:
    """
    Test from stake or liability rejects lay odds at one.
    """
    with pytest.raises(ValueError, match=r"Price must be greater than 1\.0"):
        Bet.from_stake_or_liability(Decimal(1), Decimal(100), BetSide.LAY)


def test_from_stake_or_liability_allows_back_odds_at_one() -> None:
    """
    Test from stake or liability allows back odds at one.
    """
    bet = Bet.from_stake_or_liability(Decimal(1), Decimal(10), BetSide.BACK)

    assert bet.price == Decimal(1)
    assert bet.stake == Decimal(10)
    assert bet.side == BetSide.BACK
    assert bet.exposure() == Decimal(10)
    assert bet.profit() == Decimal(0)


def test_hedging_rejects_zero_price() -> None:
    """
    Test hedging rejects zero price.
    """
    bet = Bet(Decimal("2.0"), Decimal(10), BetSide.BACK)

    with pytest.raises(ValueError, match="must be non-zero"):
        bet.hedging_stake(Decimal(0))
    with pytest.raises(ValueError, match="must be non-zero"):
        bet.hedging_bet(Decimal(0))


def test_exposure_rejects_decimal_overflow() -> None:
    """
    Test exposure rejects decimal overflow.
    """
    bet = Bet(Decimal(79228162514264337593543950335), Decimal(2), BetSide.BACK)

    with pytest.raises(ValueError, match="Decimal overflow"):
        bet.exposure()


def test_flattening_bet_rejects_zero_price() -> None:
    """
    Test flattening bet rejects zero price.
    """
    position = BetPosition()
    position.add_bet(Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK))

    with pytest.raises(ValueError, match="must be non-zero"):
        position.flattening_bet(Decimal(0))


def test_mark_to_market_rejects_zero_price() -> None:
    """
    Test mark to market rejects zero price.
    """
    position = BetPosition()
    position.add_bet(Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK))

    with pytest.raises(ValueError, match="must be non-zero"):
        position.unrealized_pnl(Decimal(0))
    with pytest.raises(ValueError, match="must be non-zero"):
        position.total_pnl(Decimal(0))


def test_add_bet_rejects_overflow_and_leaves_position_unchanged() -> None:
    """
    Test add bet rejects overflow and leaves position unchanged.
    """
    position = BetPosition()
    position.add_bet(Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK))
    before_price = position.price
    before_exposure = position.exposure

    with pytest.raises(ValueError, match="Decimal overflow"):
        position.add_bet(Bet(Decimal(79228162514264337593543950335), Decimal(2), BetSide.BACK))

    assert position.price == before_price
    assert position.exposure == before_exposure


def test_probability_conversion_rejects_unspecified_side() -> None:
    """
    Test probability conversion rejects unspecified side.
    """
    with pytest.raises(ValueError, match="must be Buy or Sell"):
        probability_to_bet(Decimal("0.4"), Decimal(10), OrderSide.NO_ORDER_SIDE)
    with pytest.raises(ValueError, match="must be Buy or Sell"):
        inverse_probability_to_bet(Decimal("0.4"), Decimal(10), OrderSide.NO_ORDER_SIDE)
