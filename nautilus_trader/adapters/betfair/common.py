import numpy as np

from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.identifiers import Venue


BETFAIR_VENUE = Venue("BETFAIR")

# -- MAPPINGS -------------------------------
"""
Mappings between nautilus and betfair.

Prefixes:
    N2B = {NAUTILUS: BETFAIR}
    B2N = {BETFAIR: NAUTILUS}

"""

N2B_SIDE = {
    # In nautilus, we map BUYS in probability space to a BACK (Back @ 3.0 is equivalent to BID/BUY @ 0.33
    OrderSide.BUY: "Back",
    OrderSide.SELL: "Lay",
}

N2B_TIME_IN_FORCE = {
    TimeInForce.GTC: None,
    TimeInForce.FOK: "FILL_OR_KILL",
}

B2N_MARKET_STREAM_SIDE = {
    "atb": OrderSide.BUY,  # Available to Back
    "batb": OrderSide.BUY,  # Best available to Back
    "bdatb": OrderSide.BUY,  # Best display to Back
    "atl": OrderSide.SELL,  # Available to Lay
    "batl": OrderSide.SELL,  # Best available to Lay
    "bdatl": OrderSide.SELL,  # Best display available to Lay
}

B_BID_KINDS = ("atb", "batb", "bdatb")
B_ASK_KINDS = ("atl", "batl", "bdatl")
B_SIDE_KINDS = B_BID_KINDS + B_ASK_KINDS

B2N_ORDER_STREAM_SIDE = {
    "B": OrderSide.BUY,
    "L": OrderSide.SELL,
}


def parse_price(p):
    return int(round(p * 100))


def parse_prob(p):
    return str(round(p, 5))


def invert_price(p):
    return parse_price(1 / (1 - (1 / p))) / 100


# -- A bunch of structures for dealing with prices and probabilites. \
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
    probabilities = map(
        parse_prob, (1 / prices)
    )  # Lowest precision to keep unique mapping
    price_probability_map.update(dict(zip(map(parse_price, prices), probabilities)))

probability_price_map = {v: k for k, v in price_probability_map.items()}
inverse_price_map = {p: invert_price(p / 100) for p in price_probability_map}
all_probabilities = np.asarray(sorted(map(float, probability_price_map)))

all_prices = np.asarray(np.asarray(list(price_probability_map)) / 100.0)


def round_price_to_betfair(price, side):
    """ If we have a probability in between two prices, round to the better price """
    idx = all_prices.searchsorted(price)
    if side == OrderSide.SELL:
        return all_prices[idx]
    elif side == OrderSide.BUY:
        return all_prices[idx - 1]
