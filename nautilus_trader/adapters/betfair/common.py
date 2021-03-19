import datetime

from betfairlightweight.filters import cancel_instruction
from betfairlightweight.filters import limit_order
from betfairlightweight.filters import place_instruction
from betfairlightweight.filters import replace_instruction
import numpy as np

from model.instrument import BettingInstrument
from model.objects import Money
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.commands import AmendOrder
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.order.base import Order


BETFAIR_VENUE = Venue("betfair")

SIDE_MAPPING = {
    # In nautilus, we map BUYS in probability space to a BACK (Back @ 3.0 is equivalent to BID/BUY @ 0.33
    OrderSide.BUY: "Back",
    OrderSide.SELL: "Back",
}

TIME_IN_FORCE_MAP = {
    TimeInForce.GTC: None,
    TimeInForce.FOK: "FILL_OR_KILL",
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


def order_submit_to_betfair(command: SubmitOrder, instrument: BettingInstrument):
    """ Convert a SubmitOrder command into the data required by betfairlightweight """
    # TODO - Investigate more order types

    order = command.order  # type: Order
    return {
        "market_id": instrument.market_id,
        # Used to de-dupe orders on betfair server side
        "customer_ref": command.id.value,
        "customer_strategy_ref": order.cl_ord_id.value,
        "async": True,  # Order updates will be sent via stream API
        "instructions": [
            place_instruction(
                order_type="LIMIT",
                selection_id=int(instrument.selection_id),
                side=SIDE_MAPPING[order.side],
                handicap=instrument.selection_handicap or None,
                limit_order=limit_order(
                    size=float(order.quantity),
                    price=round_price_to_betfair(price=order.price, side=order.side),
                    persistence_type="PERSIST",
                    time_in_force=TIME_IN_FORCE_MAP[order.time_in_force],
                    min_fill_size=0,
                ),
                customer_order_ref=order.cl_ord_id.value,
            )
        ],
    }


def order_amend_to_betfair(command: AmendOrder, instrument: BettingInstrument):
    """ Convert an AmendOrder command into the data required by betfairlightweight """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value,
        "async": True,  # Order updates will be sent via stream API
        "instructions": [
            replace_instruction(
                bet_id=command.cl_ord_id.value, new_price=float(command.price)
            )
        ],
    }


def order_cancel_to_betfair(command: CancelOrder, instrument: BettingInstrument):
    """ Convert a SubmitOrder command into the data required by betfairlightweight """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value,
        "instructions": [cancel_instruction(bet_id=command.cl_ord_id.value)],
    }


def betfair_account_to_account_state(
    account_detail, account_funds, event_id
) -> AccountState:
    account_id = f"{account_detail['firstName']}-{account_detail['lastName']}"
    currency = Currency.from_str(account_detail["currencyCode"])
    balance = float(account_funds["availableToBetBalance"])
    balance_locked = -float(account_funds["exposure"])
    balance_free = balance - balance_locked
    return AccountState(
        AccountId(issuer="betfair", identifier=account_id),
        [Money(value=balance, currency=currency)],
        [Money(value=balance_free, currency=currency)],
        [Money(value=balance_locked, currency=currency)],
        {"funds": account_funds, "detail": account_detail},
        event_id,
        datetime.datetime.now(),
    )
