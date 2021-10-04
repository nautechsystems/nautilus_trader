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

from enum import Enum

import numpy as np

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price


BETFAIR_VENUE = Venue("BETFAIR")

# -- MAPPINGS -------------------------------
"""
Mappings between Nautilus and betfair.

Prefixes:
    N2B = {NAUTILUS: BETFAIR}
    B2N = {BETFAIR: NAUTILUS}

In Nautilus, we map BUYS in probability space to a BACK
(Back @ 3.0 is equivalent to BID/BUY @ 0.33)
"""

N2B_SIDE = {
    OrderSide.BUY: "BACK",
    OrderSide.SELL: "LAY",
}

N2B_TIME_IN_FORCE = {
    TimeInForce.FOK: "FILL_OR_KILL",
    TimeInForce.FAK: "FILL_OR_KILL",
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
}

# TODO - Clean this up with Price() objects?


def parse_price(p):
    return int(round(p * 100))


def parse_prob(p):
    return str(round(p, 5)).ljust(7, "0")


def invert_price(p):
    if p is None:
        return
    return parse_price(1 / (1 - (1 / p))) / 100


def invert_probability(p):
    return 1 - p


# -- A bunch of structures for dealing with prices and probabilities.
price_increments = [
    (1.01, 2, 0.01),
    (2, 3, 0.02),
    (3, 4, 0.05),
    (4, 6, 0.1),
    (6, 10, 0.2),
    (10, 20, 0.5),
    (20, 30, 1),
    (30, 50, 2),
    (50, 100, 5),
    (100, 1000, 10),
]
price_probability_map = {}
for start, end, step in price_increments:
    prices = np.append(np.arange(start, end, step), [end])
    probabilities = map(parse_prob, (1 / prices))  # Lowest precision to keep unique mapping
    price_probability_map.update(dict(zip(map(parse_price, prices), probabilities)))

probability_price_map = {v: k for k, v in price_probability_map.items()}
inverse_price_map = {p: invert_price(p / 100) for p in price_probability_map}
all_probabilities = np.asarray(sorted(map(float, probability_price_map)))

all_prices = np.asarray(np.asarray(list(price_probability_map)) / 100.0)

MAX_BET_PROB = Price(max(price_probability_map.values()), 5)
MIN_BET_PROB = Price(min(price_probability_map.values()), 5)


def round_probability(probability, side):
    """
    If we have a probability in between two prices, round to the better price
    """
    if probability in all_probabilities:
        return probability
    idx = all_probabilities.searchsorted(probability)
    if side == OrderSide.SELL:
        if idx == len(all_probabilities):
            return all_probabilities[-1]
        return all_probabilities[idx]
    elif side == OrderSide.BUY:
        if idx == 0:
            return all_probabilities[idx]
        return all_probabilities[idx - 1]


def round_price(price, side):
    """
    If we have a probability in between two prices, round to the better price
    """
    if price in all_prices:
        return price
    else:
        idx = all_prices.searchsorted(price)
        if side == OrderSide.BUY:
            return all_prices[idx]
        elif side == OrderSide.SELL:
            return all_prices[idx - 1]


def price_to_probability(price, side=None, force=False) -> Price:
    """
    Convert a bet price into a probability, rounded to the "better" probability
    (based on the side) if a the price is between the real ticks for betfair
    prices.
    """
    rounded = round(price * 100)
    if rounded not in price_probability_map:
        if force:
            prob = 1.0 / price
            return Price(prob, precision=5)
        if side is None:
            raise ValueError(
                f"If not passing a side, price ({price}) must exist in `price_probability_map`"
            )
        rounded = round(round_price(price=price, side=side) * 100)
    probability = float(price_probability_map[rounded])
    return Price(probability, precision=5)


def probability_to_price(probability, side=None) -> Price:
    """
    Convert a bet probability into a betting price, rounded to the "better"
    price (based on the side) if a the probability is between the real ticks
    for betfair prices.
    """
    parsed = parse_prob(probability)
    if parsed not in probability_price_map:
        if side is None:
            raise ValueError(
                f"If not passing a side, probability ({probability}) "
                f"must exist in `probability_price_map`"
            )
        parsed = parse_prob(round_probability(probability=probability, side=side))
    price = float(probability_price_map[parsed]) / 100.0
    return Price(price, precision=5)


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
    HORSE_RACING = "Horse Racing"
    SOCCER = "Soccer"
    TENNIS = "Tennis"
    CRICKET = "Cricket"
    GOLF = "Golf"
    GREYHOUND_RACING = "Greyhound Racing"
    OTHER_SPORTS = "Other Sports"
