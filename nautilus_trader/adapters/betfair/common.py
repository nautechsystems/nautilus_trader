# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from enum import Enum

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick_scheme import register_tick_scheme
from nautilus_trader.model.tick_scheme.implementations.tiered import TieredTickScheme


BETFAIR_VENUE = Venue("BETFAIR")
BETFAIR_PRICE_PRECISION = 7
BETFAIR_QUANTITY_PRECISION = 4


# ------------------------------- MAPPINGS ------------------------------- #

# Mappings between Nautilus and betfair.
#
# Prefixes:
#     N2B = {NAUTILUS: BETFAIR}
#     B2N = {BETFAIR: NAUTILUS}
#
# In Nautilus, we map BUYS in probability space to a BACK
# (Back @ 3.0 is equivalent to BID/BUY @ 0.33)


N2B_SIDE = {
    OrderSide.BUY: "BACK",
    OrderSide.SELL: "LAY",
}

N2B_TIME_IN_FORCE = {
    TimeInForce.FOK: "FILL_OR_KILL",
}

B2N_MARKET_STREAM_SIDE = {
    "atb": OrderSide.SELL,  # Available to Back / Sell order
    "batb": OrderSide.SELL,  # Best available to Back / Sell order
    "bdatb": OrderSide.SELL,  # Best display to Back / Sell order
    "atl": OrderSide.BUY,  # Available to Lay / Buy order
    "batl": OrderSide.BUY,  # Best available to Lay / Buy order
    "bdatl": OrderSide.BUY,  # Best display available to Lay / Buy order
    "spb": OrderSide.SELL,  # Starting Price Back
    "spl": OrderSide.BUY,  # Starting Price LAY
}

B_BID_KINDS = ("atb", "batb", "bdatb")
B_ASK_KINDS = ("atl", "batl", "bdatl")
B_SIDE_KINDS = B_BID_KINDS + B_ASK_KINDS

B2N_ORDER_STREAM_SIDE = {
    "B": OrderSide.BUY,
    "L": OrderSide.SELL,
    "BACK": OrderSide.BUY,
    "LAY": OrderSide.SELL,
}

B2N_TIME_IN_FORCE = {
    "LAPSE": TimeInForce.DAY,
    "PERSIST": TimeInForce.GTC,
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
BETFAIR_PRICES = list(
    reversed(TieredTickScheme(name="betfair_prob", tiers=BETFAIR_PRICE_TIERS).ticks)
)
BETFAIR_PROBABILITIES = [
    Price(Price.from_int(1) / tick, precision=BETFAIR_PRICE_PRECISION) for tick in BETFAIR_PRICES
]
BETFAIR_PRICE_TO_PROBABILITY_MAP = {
    price: prob for price, prob in zip(BETFAIR_PRICES, BETFAIR_PROBABILITIES)
}
BETFAIR_PROBABILITY_TO_PRICE_MAP = {
    price: prob for price, prob in zip(BETFAIR_PROBABILITIES, BETFAIR_PRICES)
}
MAX_BET_PROB = max(BETFAIR_PROBABILITY_TO_PRICE_MAP)
MIN_BET_PROB = min(BETFAIR_PROBABILITY_TO_PRICE_MAP)

BETFAIR_TICK_SCHEME = TieredTickScheme(
    name="BETFAIR",
    tiers=[
        (start, stop, stop - start)
        for start, stop in zip(
            BETFAIR_PROBABILITIES, BETFAIR_PROBABILITIES[1:] + [Price.from_int(1)]
        )
    ],
)
register_tick_scheme(BETFAIR_TICK_SCHEME)


def price_to_probability(price_str: str) -> Price:
    PyCondition.type(price_str, str, "price", "str")
    price = Price.from_str(f"{float(price_str):.2f}")
    assert price > 0.0
    if price in BETFAIR_PRICE_TO_PROBABILITY_MAP:
        return BETFAIR_PRICE_TO_PROBABILITY_MAP[price]
    else:
        # This is likely a trade tick that has been currency adjusted, simply return the nearest price
        value = Price.from_int(1) / price
        bid = BETFAIR_TICK_SCHEME.next_bid_price(value=value)
        ask = BETFAIR_TICK_SCHEME.next_ask_price(value=value)
        if abs(bid - value) < abs(ask - value):
            return bid
        else:
            return ask


def probability_to_price(probability: Price):
    return BETFAIR_PROBABILITY_TO_PRICE_MAP[probability]


# ------------------------------- BETFAIR CONSTANTS ------------------------------- #


EVENT_TYPE_TO_NAME = {
    "1": "Soccer",
    "2": "Tennis",
    "3": "Golf",
    "4": "Cricket",
    "5": "Rugby Union",
    "1477": "Rugby League",
    "6": "Boxing",
    "7": "Horse Racing",
    "8": "Motor Sport",
    "27454571": "Esports",
    "10": "Special Bets",
    "998917": "Volleyball",
    "11": "Cycling",
    "2152880": "Gaelic Games",
    "3988": "Athletics",
    "6422": "Snooker",
    "7511": "Baseball",
    "6231": "Financial Bets",
    "6423": "American Football",
    "7522": "Basketball",
    "7524": "Ice Hockey",
    "61420": "Australian Rules",
    "468328": "Handball",
    "3503": "Darts",
    "26420387": "Mixed Martial Arts",
    "4339": "Greyhound Racing",
    "2378961": "Politics",
}


class HistoricalSportType(Enum):
    """
    Represents a `Betfair` historical sport type.
    """

    HORSE_RACING = "Horse Racing"
    SOCCER = "Soccer"
    TENNIS = "Tennis"
    CRICKET = "Cricket"
    GOLF = "Golf"
    GREYHOUND_RACING = "Greyhound Racing"
    OTHER_SPORTS = "Other Sports"
