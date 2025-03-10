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

from nautilus_trader.core.nautilus_pyo3 import Bet
from nautilus_trader.core.nautilus_pyo3 import BetPosition
from nautilus_trader.core.nautilus_pyo3 import BetSide
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import calc_bets_pnl
from nautilus_trader.core.nautilus_pyo3 import inverse_probability_to_bet
from nautilus_trader.core.nautilus_pyo3 import probability_to_bet


def test_bet_exposure():
    # For a BACK bet, exposure should be price * stake
    bet_back = Bet(Decimal("2.0"), Decimal("100.0"), BetSide.BACK)
    assert bet_back.exposure() == Decimal("200.0")

    # For a LAY bet, exposure is negative
    bet_lay = Bet(Decimal("2.0"), Decimal("100.0"), BetSide.LAY)
    assert bet_lay.exposure() == Decimal("-200.0")


def test_bet_profit_and_payoffs():
    # BACK bet profit: stake * (price - 1) = 100 * 1 = 100
    bet_back = Bet(Decimal("2.0"), Decimal("100.0"), BetSide.BACK)
    assert bet_back.profit() == Decimal("100.0")
    assert bet_back.outcome_win_payoff() == Decimal("100.0")
    assert bet_back.outcome_lose_payoff() == Decimal("-100.0")

    # LAY bet profit: equals stake = 100
    bet_lay = Bet(Decimal("2.0"), Decimal("100.0"), BetSide.LAY)
    assert bet_lay.profit() == Decimal("100.0")
    assert bet_lay.outcome_win_payoff() == Decimal("-100.0")
    assert bet_lay.outcome_lose_payoff() == Decimal("100.0")


def test_hedging_functions():
    # For a BACK bet, the hedging stake should be (price / new_price) * stake
    bet = Bet(Decimal("2.0"), Decimal("100.0"), BetSide.BACK)
    hedging_stake = bet.hedging_stake(Decimal("1.5"))
    expected = (Decimal("2.0") / Decimal("1.5")) * Decimal("100.0")
    precision = 8  # Arbitrary precision for testing
    assert round(hedging_stake, precision) == round(expected, precision)

    # The hedging bet should have a new price and the opposite side
    hedge_bet = bet.hedging_bet(Decimal("1.5"))
    assert hedge_bet.price == Decimal("1.5")
    # For a BACK bet, the hedge should be a LAY bet
    assert hedge_bet.side == BetSide.LAY


def test_betposition_methods():
    pos = BetPosition()
    # Initially, the position should have no side
    assert pos.side is None

    # Add a BACK bet
    bet1 = Bet(Decimal("2.0"), Decimal("100.0"), BetSide.BACK)
    pos.add_bet(bet1)
    assert pos.side == BetSide.BACK

    # Add a LAY bet that partially offsets the BACK bet
    bet2 = Bet(Decimal("2.0"), Decimal("150.0"), BetSide.LAY)
    pos.add_bet(bet2)
    # Depending on the internal logic, the overall exposure may flip negative
    assert pos.exposure < Decimal("0")

    # If the net exposure is non-zero, as_bet() should return a Bet
    assert pos.as_bet() is not None


def dec(val):
    return Decimal(str(val))


# --- Bet tests -----------------------------------------------------------------------------------


def test_bet_creation():
    # Arrange
    price = dec(2.0)
    stake = dec(100)
    side = BetSide.BACK

    # Act
    bet = Bet(price, stake, side)

    # Assert
    assert bet.price == price
    assert bet.stake == stake
    assert bet.side == side


def test_bet_exposure_back():
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    exposure = bet.exposure()
    assert exposure == pytest.approx(dec(200))


def test_bet_exposure_lay():
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    exposure = bet.exposure()
    assert exposure == pytest.approx(dec(-200))


def test_bet_liability_back():
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    liability = bet.liability()
    assert liability == pytest.approx(dec(100))


def test_bet_liability_lay():
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    liability = bet.liability()
    assert liability == pytest.approx(dec(100))


def test_bet_profit_back():
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    profit = bet.profit()
    assert profit == pytest.approx(dec(100))


def test_bet_profit_lay():
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    profit = bet.profit()
    assert profit == pytest.approx(dec(100))


def test_outcome_win_payoff_back():
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    win_payoff = bet.outcome_win_payoff()
    assert win_payoff == pytest.approx(dec(100))


def test_outcome_win_payoff_lay():
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    win_payoff = bet.outcome_win_payoff()
    assert win_payoff == pytest.approx(dec(-100))


def test_outcome_lose_payoff_back():
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    lose_payoff = bet.outcome_lose_payoff()
    assert lose_payoff == pytest.approx(dec(-100))


def test_outcome_lose_payoff_lay():
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    lose_payoff = bet.outcome_lose_payoff()
    assert lose_payoff == pytest.approx(dec(100))


def test_hedging_stake_back():
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    hedging_stake = bet.hedging_stake(dec(1.5))
    expected = (dec(2.0) / dec(1.5)) * dec(100)
    precision = 8  # Arbitrary precision for testing
    assert round(hedging_stake, precision) == round(expected, precision)


def test_hedging_bet_lay():
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    hedge_bet = bet.hedging_bet(dec(1.5))

    assert hedge_bet.side == BetSide.BACK
    assert hedge_bet.price == dec(1.5)
    precision = 8  # Arbitrary precision for testing
    expected_stake = dec("133.3333333333333333333333333")
    assert round(hedge_bet.stake, precision) == round(expected_stake, precision)


# --- BetPosition tests ---------------------------------------------------------------------------


def test_bet_position_initialization():
    position = BetPosition()
    assert position.price == Decimal(0)
    assert position.exposure == Decimal(0)
    assert position.realized_pnl == Decimal(0)


def test_bet_position_side_none():
    position = BetPosition()
    assert position.side is None


def test_bet_position_side_back():
    position = BetPosition()
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    position.add_bet(bet)
    assert position.side == BetSide.BACK


def test_bet_position_side_lay():
    position = BetPosition()
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    position.add_bet(bet)
    assert position.side == BetSide.LAY


def test_position_increase_back():
    position = BetPosition()
    bet1 = Bet(dec(2.0), dec(100), BetSide.BACK)
    bet2 = Bet(dec(2.0), dec(50), BetSide.BACK)
    position.add_bet(bet1)
    position.add_bet(bet2)
    # exposure = 200 + 100 = 300
    assert position.exposure == pytest.approx(dec(300))


def test_position_increase_lay():
    position = BetPosition()
    bet1 = Bet(dec(2.0), dec(100), BetSide.LAY)
    bet2 = Bet(dec(2.0), dec(50), BetSide.LAY)
    position.add_bet(bet1)
    position.add_bet(bet2)
    # exposure = -200 + (-100) = -300
    assert position.exposure == pytest.approx(dec(-300))


def test_position_flip():
    position = BetPosition()
    back_bet = Bet(dec(2.0), dec(100), BetSide.BACK)  # exposure +200
    lay_bet = Bet(dec(2.0), dec(150), BetSide.LAY)  # exposure -300
    position.add_bet(back_bet)
    position.add_bet(lay_bet)
    # Net exposure: 200 + (-300) = -100 → side becomes LAY.
    assert position.side == BetSide.LAY
    assert position.exposure == dec(-100)


def test_position_flat():
    position = BetPosition()
    back_bet = Bet(dec(2.0), dec(100), BetSide.BACK)  # exposure +200
    lay_bet = Bet(dec(2.0), dec(100), BetSide.LAY)  # exposure -200
    position.add_bet(back_bet)
    position.add_bet(lay_bet)
    assert position.side is None
    assert position.exposure == pytest.approx(dec(0))


def test_unrealized_pnl_negative():
    position = BetPosition()
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    position.add_bet(bet)
    unrealized_pnl = position.unrealized_pnl(dec(2.5))
    assert unrealized_pnl == dec(-20)


def test_total_pnl():
    position = BetPosition()
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    position.add_bet(bet)
    total_pnl = position.total_pnl(dec(2.5))
    assert total_pnl == pytest.approx(dec(-20))


def test_flattening_bet_back_profit():
    position = BetPosition()
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    position.add_bet(bet)
    flattening_bet = position.flattening_bet(dec("1.6"))
    assert flattening_bet is not None
    assert flattening_bet.side == BetSide.LAY
    assert flattening_bet.stake == dec("125")


def test_flattening_bet_back_hack():
    position = BetPosition()
    bet = Bet(dec(2.0), dec(100), BetSide.BACK)
    position.add_bet(bet)
    flattening_bet = position.flattening_bet(dec(2.5))
    assert flattening_bet is not None
    assert flattening_bet.side == BetSide.LAY
    assert float(flattening_bet.stake) == pytest.approx(80, rel=1e-2)


def test_flattening_bet_lay():
    position = BetPosition()
    bet = Bet(dec(2.0), dec(100), BetSide.LAY)
    position.add_bet(bet)
    flattening_bet = position.flattening_bet(dec(1.5))
    assert flattening_bet is not None
    assert flattening_bet.side == BetSide.BACK
    assert round(flattening_bet.stake, 8) == dec("133.33333333")


def test_calc_bets_pnl_single_back_bet():
    bet = Bet(dec(5.0), dec(100.0), BetSide.BACK)
    pnl = calc_bets_pnl([bet])
    # Profit = 5.0 - 1 = 4 * 100 = 400
    assert pnl == pytest.approx(dec(400.0))


def test_calc_bets_pnl_single_lay_bet():
    bet = Bet(dec(4.0), dec(100.0), BetSide.LAY)
    pnl = calc_bets_pnl([bet])
    # For a lay bet, outcome win payoff = -liability, here -300.
    assert pnl == pytest.approx(dec(-300.0))


def test_calc_bets_pnl_multiple_bets():
    back_bet = Bet(dec(5.0), dec(100.0), BetSide.BACK)
    lay_bet = Bet(dec(4.0), dec(100.0), BetSide.LAY)
    pnl = calc_bets_pnl([back_bet, lay_bet])
    expected = dec(400.0) + dec(-300.0)
    assert pnl == pytest.approx(expected)


def test_calc_bets_pnl_mixed_bets():
    back_bet1 = Bet(dec(5.0), dec(100.0), BetSide.BACK)
    back_bet2 = Bet(dec(2.0), dec(50.0), BetSide.BACK)
    lay_bet1 = Bet(dec(3.0), dec(75.0), BetSide.LAY)
    pnl = calc_bets_pnl([back_bet1, back_bet2, lay_bet1])
    expected = dec(400.0) + dec(50.0) + dec(-150.0)
    assert pnl == pytest.approx(expected)


def test_calc_bets_pnl_no_bets():
    bets = []
    pnl = calc_bets_pnl(bets)
    assert pnl == Decimal(0)


def test_calc_bets_pnl_zero_outcome():
    back_bet = Bet(dec(5.0), dec(100.0), BetSide.BACK)
    lay_bet = Bet(dec(5.0), dec(100.0), BetSide.LAY)
    pnl = calc_bets_pnl([back_bet, lay_bet])
    assert pnl == Decimal(0)


def test_probability_to_bet_back_simple():
    bet = probability_to_bet(probability=dec(0.50), volume=dec(50.0), side=OrderSide.BUY)
    expected = Bet(dec(2.0), dec(25.0), BetSide.BACK)
    assert bet == expected
    assert bet.outcome_win_payoff() == dec(25)
    assert bet.outcome_lose_payoff() == dec(-25)


def test_probability_to_bet_back_high_prob():
    bet = probability_to_bet(probability=dec(0.64), volume=dec(50.0), side=OrderSide.BUY)
    expected = Bet(dec(1.5625), dec(32.0), BetSide.BACK)
    assert bet == expected
    assert bet.outcome_win_payoff() == dec(18)
    assert bet.outcome_lose_payoff() == dec(-32)


def test_probability_to_bet_back_low_prob():
    bet = probability_to_bet(probability=dec(0.40), volume=dec(50.0), side=OrderSide.BUY)
    expected = Bet(dec(2.5), dec(20.0), BetSide.BACK)
    assert bet == expected
    assert bet.outcome_win_payoff() == dec(30)
    assert bet.outcome_lose_payoff() == dec(-20)


def test_probability_to_bet_sell():
    bet = probability_to_bet(probability=dec(0.80), volume=dec(50.0), side=OrderSide.SELL)
    expected = Bet(Decimal("1.25"), Decimal("40"), BetSide.LAY)
    assert bet == expected
    assert bet.outcome_win_payoff() == Decimal("-10")
    assert bet.outcome_lose_payoff() == Decimal("40")


def test_inverse_probability_to_bet():
    original_bet = probability_to_bet(probability=dec(0.80), volume=dec(100), side=OrderSide.SELL)
    reverse_bet = probability_to_bet(probability=dec(0.20), volume=dec(100), side=OrderSide.BUY)
    inverse_bet = inverse_probability_to_bet(
        probability=dec(0.80),
        volume=dec(100),
        side=OrderSide.SELL,
    )
    assert original_bet.outcome_win_payoff() == pytest.approx(reverse_bet.outcome_lose_payoff())
    assert original_bet.outcome_win_payoff() == pytest.approx(inverse_bet.outcome_lose_payoff())
    assert original_bet.outcome_lose_payoff() == pytest.approx(reverse_bet.outcome_win_payoff())
    assert original_bet.outcome_lose_payoff() == pytest.approx(inverse_bet.outcome_win_payoff())


def test_inverse_probability_to_bet_example2():
    original_bet = probability_to_bet(probability=dec(0.64), volume=dec(50), side=OrderSide.SELL)
    inverse_bet = inverse_probability_to_bet(
        probability=dec(0.64),
        volume=dec(50),
        side=OrderSide.SELL,
    )
    # Original bet checks
    assert float(original_bet.stake) == pytest.approx(32)
    assert float(original_bet.outcome_win_payoff()) == pytest.approx(-18)
    assert float(original_bet.outcome_lose_payoff()) == 32
    # Inverse bet checks
    assert float(inverse_bet.stake) == pytest.approx(18.0)
    assert float(inverse_bet.outcome_win_payoff()) == pytest.approx(32.0)
    assert float(inverse_bet.outcome_lose_payoff()) == -18


def test_bet_position_back_and_lay():
    bet_position = BetPosition()
    back_bet = Bet(Decimal("2.00"), Decimal("100000"), BetSide.BACK)
    lay_bet = Bet(Decimal("3.00"), Decimal("10000"), BetSide.LAY)
    bet_position.add_bet(back_bet)
    bet_position.add_bet(lay_bet)

    # Assuming exposure is stake * odds for back and -stake * odds for lay
    # 200,000 - 30,000 = 170,000
    expected_exposure = Decimal("100000") * Decimal("2.00") - Decimal("10000") * Decimal("3.00")
    # Note: Portfolio test expects -170,000, suggesting a sign convention
    assert expected_exposure == Decimal("170000")
    assert bet_position.exposure == expected_exposure  # Adjust based on actual implementation

    # Check unrealized PnL at mark price 3.00
    mark_price = Decimal("3.00")
    unrealized_pnl = bet_position.unrealized_pnl(mark_price)
    assert unrealized_pnl == Decimal("-28333.33333333333333333333333")
