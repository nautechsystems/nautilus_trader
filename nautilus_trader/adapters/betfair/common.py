import datetime

from betfairlightweight.filters import cancel_instruction
from betfairlightweight.filters import limit_order
from betfairlightweight.filters import place_instruction
from betfairlightweight.filters import replace_instruction
import numpy as np

from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.time_in_force import TimeInForce
from nautilus_trader.model.commands import AmendOrder
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import BettingInstrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.order.limit import LimitOrder


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


# TODO - Investigate more order types
def order_submit_to_betfair(command: SubmitOrder, instrument: BettingInstrument):
    """ Convert a SubmitOrder command into the data required by betfairlightweight """

    order = command.order  # type: LimitOrder
    return {
        "market_id": instrument.market_id,
        # Used to de-dupe orders on betfair server side
        "customer_ref": order.cl_ord_id.value,
        "customer_strategy_ref": f"{command.account_id}-{command.strategy_id}",
        "async": True,  # Order updates will be sent via stream API
        "instructions": [
            place_instruction(
                order_type="LIMIT",
                selection_id=int(instrument.selection_id),
                side=N2B_SIDE[order.side],
                handicap=instrument.selection_handicap or None,
                limit_order=limit_order(
                    size=float(order.quantity),
                    price=round_price_to_betfair(price=order.price, side=order.side),
                    persistence_type="PERSIST",
                    time_in_force=N2B_TIME_IN_FORCE[order.time_in_force],
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
        "customer_ref": command.cl_ord_id.value,
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


# def create_market(update):
#     """
#     From a market image snippet, return the orderbook per-instrument
#
#     :param update:
#     :return tuple: (market_id, selection_id), Orderbook
#     """
#     if update.get("img") is True:
#         market_id = update["id"]
#         if update.get("rc") is None:
#             return []
#         for grp, runner_entries in itertools.groupby(
#             update.get("rc", []), lambda x: (x["id"], x.get("hc"))
#         ):
#             selection_id, handicap = grp
#             ob_kwargs = {}
#             runner_entries = list(runner_entries)
#             for runner in runner_entries:
#                 instrument = fetch_instrument(
#                     market_id=market_id,
#                     selection_id=selection_id,
#                     handicap=parse_handicap(handicap),
#                 )
#                 if instrument is None:
#                     continue
#                 ob_kwargs.setdefault(instrument.id, {"instrument": instrument})
#                 for side in ("atl", "atb"):
#                     if runner.get(side, []):
#                         bet_side = BET_SIDE[side]
#                         side_name = {OrderSide.BID: "bids", OrderSide.ASK: "asks"}[
#                             bet_side
#                         ]
#                         ob_kwargs[instrument.id][side_name] = Ladder.from_orders(
#                             orders=[
#                                 Order(price=p, volume=v, side=bet_side)
#                                 for p, v in runner[side]
#                             ]
#                         )
#             for ins, kw in ob_kwargs.items():
#                 instrument = kw.pop("instrument")
#                 book = BettingOrderbook(**kw)
#                 yield instrument, book
#
#
# def build_market_snapshot_messages(self: "BetfairDataClient", raw):
#     for market in raw.get("mc", []):
#         market_definition = market.get("marketDefinition", {})
#         for selection in market_definition.get("runners", []):
#             if market_definition["status"] == "CLOSED":
#                 # TODO Should yield an event here
#                 continue
#             try:
#                 kw = dict(
#                     market_id=market["id"],
#                     selection_id=str(selection["id"]),
#                     handicap=str(selection.get("hc", "0.0")),
#                 )
#                 instrument = self._instrument_provider.get_betting_instrument(kw)
#             except KeyError:
#                 logging.error(f"Couldn't find instrument for market_id: {kw}")
#                 # TODO - Should probably raise? We may need to do an instrument re-pull
#                 continue
#
#             msu = BettingInstrumentStatusUpdate(
#                 source=SOURCE,
#                 status=MARKET_STATUS[market_definition["status"]],
#                 instrument_id=str(instrument.id),
#                 extra_headers={"channel_suffix": instrument_to_channel(instrument)},
#             )
#             if msu is not None:
#                 msu.remote_timestamp = remote_timestamp
#                 return msu
#
#         for instrument, book in create_market(market):
#             mbu = messages.MarketBookUpdate(
#                 source=SOURCE,
#                 orderbook=book,
#                 instrument_id=str(instrument.id),
#                 extra_headers={"channel_suffix": instrument_to_channel(instrument)},
#             )
#             mbu.remote_timestamp = remote_timestamp
#             return mbu
#
#
# def build_market_update_messages(self: "BetfairDataClient", raw):
#     for market in raw.get("mc", []):
#         market_id = market["id"]
#         for runner in market.get("rc", []):
#             instrument = fetch_instrument(
#                 market_id=market_id,
#                 selection_id=runner["id"],
#                 handicap=parse_handicap(runner.get("hc")),
#             )
#             for side in ("atb", "atl"):
#                 for price, size in runner.get(side, []):
#                     if size == 0:
#                         msg = messages.MarketLevelDelete(
#                             source=SOURCE,
#                             level=Level(
#                                 orders=[
#                                     Order(side=BET_SIDE[side], price=price, volume=size)
#                                 ]
#                             ),
#                             instrument_id=str(instrument.id),
#                             extra_headers={
#                                 "channel_suffix": instrument_to_channel(instrument)
#                             },
#                         )
#                     else:
#                         msg = messages.MarketLevelUpdate(
#                             source=SOURCE,
#                             level=Level(
#                                 orders=[
#                                     Order(side=BET_SIDE[side], price=price, volume=size)
#                                 ]
#                             ),
#                             instrument_id=str(instrument.id),
#                             extra_headers={
#                                 "channel_suffix": instrument_to_channel(instrument)
#                             },
#                         )
#
#                     msg.remote_timestamp = parse_betfair_publish_time(raw["pt"])
#                     return msg
#
#             for price, size in runner.get("trd", []):
#                 if size == 0:
#                     continue
#                 msg = messages.MarketTradeUpdate(
#                     source=SOURCE,
#                     trade=BetTrade(price=price, volume=size, side=OrderSide.BID),
#                     instrument_id=str(instrument.id),
#                     extra_headers={"channel_suffix": instrument_to_channel(instrument)},
#                 )
#                 msg.remote_timestamp = parse_betfair_publish_time(raw["pt"])
#                 return msg
#
#         if market.get("marketDefinition", {}).get("status") == "CLOSED":
#             remote_timestamp = parse_betfair_publish_time(raw["pt"])
#             for runner in market["marketDefinition"]["runners"]:
#                 instrument = fetch_instrument(
#                     market_id=market_id,
#                     selection_id=runner["id"],
#                     handicap=parse_handicap(runner.get("hc")),
#                 )
#                 if instrument is None:
#                     continue
#                 msg = BettingInstrumentStatusUpdate(
#                     source=SOURCE,
#                     instrument_id=str(instrument.id),
#                     status=BettingInstrumentStatus.CLOSED,
#                     extra_headers={"channel_suffix": instrument_to_channel(instrument)},
#                 )
#                 msg.remote_timestamp = remote_timestamp
#                 return msg
#                 if runner["status"] == "LOSER":
#                     msg = messages.InstrumentCloseValuation(
#                         source=SOURCE,
#                         instrument_id=str(instrument.id),
#                         close_price=0,
#                         extra_headers={
#                             "channel_suffix": instrument_to_channel(instrument)
#                         },
#                     )
#                     msg.remote_timestamp = remote_timestamp
#                     return msg
#                 elif runner["status"] == "WINNER":
#                     msg = messages.InstrumentCloseValuation(
#                         source=SOURCE,
#                         instrument_id=str(instrument.id),
#                         close_price=1,
#                         extra_headers={
#                             "channel_suffix": instrument_to_channel(instrument)
#                         },
#                     )
#                     msg.remote_timestamp = remote_timestamp
#                     return msg
#
#         if (
#             market.get("marketDefinition", {}).get("inPlay")
#             and not market.get("marketDefinition", {}).get("status") == "CLOSED"
#         ):
#             remote_timestamp = parse_betfair_publish_time(raw["pt"])
#             for runner in market["marketDefinition"]["runners"]:
#                 instrument = fetch_instrument(
#                     market_id=market_id,
#                     selection_id=runner["id"],
#                     handicap=parse_handicap(runner.get("hc")),
#                 )
#                 if instrument is None:
#                     continue
#                 msg = BettingInstrumentStatusUpdate(
#                     source=SOURCE,
#                     instrument_id=str(instrument.id),
#                     status=BettingInstrumentStatus.IN_PLAY,
#                     extra_headers={"channel_suffix": instrument_to_channel(instrument)},
#                 )
#                 msg.remote_timestamp = remote_timestamp
#                 return msg
#
#
# def on_market_update(self: "BetfairDataClient", raw):
#     if raw.get("ct") == "HEARTBEAT":
#         # TODO - Do we send out heartbeats
#         return
#     for mc in raw.get("mc", []):
#         if mc.get("img"):
#             return build_market_snapshot_messages(self, raw)
#         else:
#             return build_market_update_messages(self, raw)
