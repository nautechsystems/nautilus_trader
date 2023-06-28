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

from betfair_parser.spec.betting.enums import PersistenceType
from betfair_parser.spec.betting.enums import Side
from betfair_parser.spec.betting.enums import TimeInForce as BetfairTimeInForce
from betfair_parser.spec.common import OrderType

from nautilus_trader.core.rust.model import OrderType as NautilusOrderType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.tick_scheme import register_tick_scheme
from nautilus_trader.model.tick_scheme.implementations.tiered import TieredTickScheme


# ------------------------------- MAPPINGS ------------------------------- #
# Mappings between Nautilus and betfair - prefixes:
#     N2B = {NAUTILUS: BETFAIR}
#     B2N = {BETFAIR: NAUTILUS}

N2B_SIDE = {
    OrderSide.BUY: Side.BACK,
    OrderSide.SELL: Side.LAY,
}

N2B_TIME_IN_FORCE = {
    TimeInForce.FOK: BetfairTimeInForce.FILL_OR_KILL,
}

N2B_PERSISTENCE = {
    TimeInForce.GTC: PersistenceType.PERSIST,
    TimeInForce.DAY: PersistenceType.MARKET_ON_CLOSE,
}

B2N_MARKET_SIDE = {
    "atb": OrderSide.SELL,  # Available to Back / Sell order
    "batb": OrderSide.SELL,  # Best available to Back / Sell order
    "bdatb": OrderSide.SELL,  # Best display to Back / Sell order
    "atl": OrderSide.BUY,  # Available to Lay / Buy order
    "batl": OrderSide.BUY,  # Best available to Lay / Buy order
    "bdatl": OrderSide.BUY,  # Best display available to Lay / Buy order
    "spb": OrderSide.SELL,  # Starting Price Back
    "spl": OrderSide.BUY,  # Starting Price LAY
}

B2N_ORDER_SIDE = {
    Side.BACK: OrderSide.BUY,
    Side.LAY: OrderSide.SELL,
    "B": OrderSide.BUY,
    "L": OrderSide.BUY,
}

B2N_TIME_IN_FORCE = {
    PersistenceType.LAPSE: TimeInForce.DAY,
    PersistenceType.PERSIST: TimeInForce.GTC,
}

B2N_ORDER_TYPE = {
    OrderType.LIMIT: NautilusOrderType.LIMIT,
    OrderType.LIMIT_ON_CLOSE: NautilusOrderType.LIMIT,
    OrderType.MARKET_ON_CLOSE: NautilusOrderType.MARKET,
}

BETFAIR_PRICE_TIERS = [
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

BETFAIR_TICK_SCHEME = TieredTickScheme("BETFAIR", BETFAIR_PRICE_TIERS)
BETFAIR_FLOAT_TO_PRICE = {price.as_double(): price for price in BETFAIR_TICK_SCHEME.ticks}
MAX_BET_PRICE = max(BETFAIR_TICK_SCHEME.ticks)
MIN_BET_PRICE = min(BETFAIR_TICK_SCHEME.ticks)
register_tick_scheme(BETFAIR_TICK_SCHEME)
