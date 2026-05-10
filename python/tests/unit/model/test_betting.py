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

from decimal import Decimal

from nautilus_trader.model import Bet
from nautilus_trader.model import BetPosition
from nautilus_trader.model import BetSide
from nautilus_trader.model import OrderSide
from nautilus_trader.model import calc_bets_pnl
from nautilus_trader.model import inverse_probability_to_bet
from nautilus_trader.model import probability_to_bet


def test_bet_properties_and_payoffs():
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


def test_bet_factories():
    back = Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK)
    lay = Bet.from_liability(Decimal("2.5"), Decimal(15), BetSide.LAY)

    assert back.stake == Decimal(10)
    assert back.side == BetSide.BACK
    assert lay.side == BetSide.LAY
    assert lay.stake == Decimal(10)


def test_bet_position_add_bets_and_reset():
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


def test_betting_helpers_create_expected_bets():
    probability_bet = probability_to_bet(Decimal("0.4"), Decimal(10), OrderSide.BUY)
    inverse_bet = inverse_probability_to_bet(Decimal("0.4"), Decimal(10), OrderSide.SELL)

    assert probability_bet.side == BetSide.BACK
    assert probability_bet.price == Decimal("2.5")
    assert probability_bet.stake == Decimal(4)
    assert inverse_bet.side == BetSide.BACK
    assert inverse_bet.price > Decimal(1)
    assert inverse_bet.stake > Decimal(0)


def test_calc_bets_pnl():
    bets = [
        Bet.from_stake(Decimal("2.0"), Decimal(10), BetSide.BACK),
        Bet.from_liability(Decimal("2.5"), Decimal(15), BetSide.LAY),
    ]

    assert calc_bets_pnl(bets) == Decimal("-5.0")
    assert calc_bets_pnl(bets) == sum((bet.outcome_win_payoff() for bet in bets), Decimal(0))


def test_bet_side_helpers():
    assert BetSide.from_str("BACK") == BetSide.BACK
    assert BetSide.from_order_side(OrderSide.BUY) == BetSide.BACK
    assert BetSide.BACK.opposite() == BetSide.LAY
