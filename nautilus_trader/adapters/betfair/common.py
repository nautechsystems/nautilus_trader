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

from typing import Final

from betfair_parser.spec.betting.enums import PersistenceType
from betfair_parser.spec.betting.enums import Side
from betfair_parser.spec.betting.enums import TimeInForce as BetfairTimeInForce
from betfair_parser.spec.common import OrderType

from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.core.rust.model import OrderType as NautilusOrderType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.tick_scheme import register_tick_scheme
from nautilus_trader.model.tick_scheme.implementations.tiered import TieredTickScheme


# ------------------------------- MAPPINGS ------------------------------- #
# Mappings between Nautilus and betfair - prefixes:
#     N2B = {NAUTILUS: BETFAIR}
#     B2N = {BETFAIR: NAUTILUS}


class OrderSideParser:
    BACKS = (Side.BACK, "B")
    LAYS = (Side.LAY, "L")

    @classmethod
    def to_nautilus(cls, side: Side | str) -> OrderSide:
        if side in cls.BACKS:
            return OrderSide.SELL
        elif side in cls.LAYS:
            return OrderSide.BUY
        else:
            raise ValueError(f"Unknown side: {side}")

    @staticmethod
    def to_betfair(order_side: OrderSide) -> Side:
        if order_side == OrderSide.BUY:
            return Side.LAY
        elif order_side == OrderSide.SELL:
            return Side.BACK
        else:
            raise ValueError(f"Unknown order_side: {order_side}")


N2B_TIME_IN_FORCE: Final[dict[TimeInForce, BetfairTimeInForce]] = {
    TimeInForce.FOK: BetfairTimeInForce.FILL_OR_KILL,
    TimeInForce.IOC: BetfairTimeInForce.FILL_OR_KILL,  # min_fill_size 0 also needed
}

N2B_PERSISTENCE: Final[dict[TimeInForce, PersistenceType]] = {
    TimeInForce.GTC: PersistenceType.PERSIST,
    TimeInForce.DAY: PersistenceType.LAPSE,
}

B2N_MARKET_SIDE: Final[dict[str, OrderSide]] = {
    "atb": OrderSide.SELL,  # Available to Back / Sell order
    "batb": OrderSide.SELL,  # Best available to Back / Sell order
    "bdatb": OrderSide.SELL,  # Best display to Back / Sell order
    "atl": OrderSide.BUY,  # Available to Lay / Buy order
    "batl": OrderSide.BUY,  # Best available to Lay / Buy order
    "bdatl": OrderSide.BUY,  # Best display available to Lay / Buy order
    "spb": OrderSide.SELL,  # Starting Price Back
    "spl": OrderSide.BUY,  # Starting Price LAY
}


B2N_TIME_IN_FORCE: Final[dict[PersistenceType, TimeInForce]] = {
    PersistenceType.LAPSE: TimeInForce.DAY,
    PersistenceType.PERSIST: TimeInForce.GTC,
}

B2N_ORDER_TYPE: Final[dict[OrderType, NautilusOrderType]] = {
    OrderType.LIMIT: NautilusOrderType.LIMIT,
    OrderType.LIMIT_ON_CLOSE: NautilusOrderType.LIMIT,
    OrderType.MARKET_ON_CLOSE: NautilusOrderType.MARKET,
}

BETFAIR_PRICE_TIERS: Final[list[tuple[float, ...]]] = [
    (1.01, 2, 0.01),
    (2, 3, 0.02),
    (3, 4, 0.05),
    (4, 6, 0.1),
    (6, 10, 0.2),
    (10, 20, 0.5),
    (20, 30, 1),
    (30, 50, 2),
    (50, 100, 5),
    (100, 1010, 10),
]

BETFAIR_TICK_SCHEME = TieredTickScheme(
    "BETFAIR",
    BETFAIR_PRICE_TIERS,
    price_precision=BETFAIR_PRICE_PRECISION,
)
BETFAIR_FLOAT_TO_PRICE = {price.as_double(): price for price in BETFAIR_TICK_SCHEME.ticks}
MAX_BET_PRICE = max(BETFAIR_TICK_SCHEME.ticks)
MIN_BET_PRICE = min(BETFAIR_TICK_SCHEME.ticks)
register_tick_scheme(BETFAIR_TICK_SCHEME)
