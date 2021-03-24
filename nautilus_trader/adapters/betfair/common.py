import datetime
import itertools
import logging

from betfairlightweight.filters import cancel_instruction
from betfairlightweight.filters import limit_order
from betfairlightweight.filters import place_instruction
from betfairlightweight.filters import replace_instruction
import numpy as np

from nautilus_trader.core.datetime import from_unix_time_ms
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.c_enums.orderbook_op import OrderBookOperationType
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
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.limit import LimitOrder
from nautilus_trader.model.orderbook.book import OrderBookOperation
from nautilus_trader.model.orderbook.book import OrderBookOperations
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.tick import TradeTick


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


def build_market_snapshot_messages(self, raw):
    for market in raw.get("mc", []):
        market_definition = market.get("marketDefinition", {})

        # Market status events
        for selection in market_definition.get("runners", []):
            if market_definition["status"] == "CLOSED":
                # TODO Should yield an event here
                continue
            kw = dict(
                market_id=market["id"],
                selection_id=str(selection["id"]),
                handicap=str(selection.get("hc", "0.0")),
            )
            try:
                instrument = self.instrument_provider().get_betting_instrument(**kw)
                # TODO - Need to add a instrument status event here
                # msu = InstrumentStatusUpdate(
                #     status=MARKET_STATUS[market_definition["status"]],
                #     instrument_id=str(instrument.id),
                # )
            except KeyError:
                logging.error(f"Couldn't find instrument for market_id: {kw}")
                # TODO - Should probably raise? We may need to do an instrument re-pull
                continue

        # Orderbook snapshots
        if market.get("img") is True:
            market_id = market["id"]
            for (selection_id, handicap), selections in itertools.groupby(
                market.get("rc", []), lambda x: (x["id"], x.get("hc"))
            ):
                for selection in list(selections):
                    kw = dict(
                        market_id=market_id,
                        selection_id=str(selection_id),
                        handicap=str(handicap or "0.0"),
                    )
                    instrument = self.instrument_provider().get_betting_instrument(**kw)
                    # Check we only have one of [best bets / depth bets / all bets]
                    bid_keys = [k for k in B_BID_KINDS if k in selection] or ["atb"]
                    ask_keys = [k for k in B_ASK_KINDS if k in selection] or ["atb"]
                    assert len(bid_keys) <= 1
                    assert len(ask_keys) <= 1
                    snapshot = OrderBookSnapshot(
                        level=OrderBookLevel.L2,
                        instrument_id=instrument.id,
                        bids=[
                            Order(price=p, volume=q, side=OrderSide.BUY)
                            for _, p, q in selection.get((bid_keys or ["atb"])[0], [])
                        ],
                        asks=[
                            Order(price=p, volume=q, side=OrderSide.SELL)
                            for _, p, q in selection.get((bid_keys or ["atl"])[0], [])
                        ],
                        timestamp=from_unix_time_ms(raw["pt"]),
                    )
                    self._handle_data(snapshot)

                    # TODO - handle orderbook snapshot
                    assert snapshot


def build_market_update_messages(self, raw):
    for market in raw.get("mc", []):
        market_id = market["id"]
        for runner in market.get("rc", []):
            kw = dict(
                market_id=market_id,
                selection_id=str(runner["id"]),
                handicap=str(runner.get("hc") or "0.0"),
            )
            instrument = self.instrument_provider().get_betting_instrument(**kw)
            assert instrument
            operations = []
            assert operations
            for side in B_SIDE_KINDS:
                for price, volume in runner.get(side, []):
                    operations.append(
                        OrderBookOperation(
                            op_type=OrderBookOperationType.delete
                            if volume == 0
                            else OrderBookOperationType.update,
                            order=Order(
                                price=price,
                                volume=volume,
                                side=B2N_MARKET_STREAM_SIDE[side],
                            ),
                        )
                    )
            ob_update = OrderBookOperations(
                level=OrderBookLevel.L2,
                instrument_id=instrument.id,
                ops=operations,
                timestamp=datetime.datetime.utcfromtimestamp(market["pt"] / 1e3),
            )
            assert ob_update
            # TODO - emit orderbook updates

            for price, volume in runner.get("trd", []):
                trade_tick = TradeTick(
                    instrument_id=instrument.id,
                    price=Price(price),
                    quantity=Quantity(volume),
                    side=OrderSide.BUY,
                    # TradeMatchId(trade_match_id),
                    timestamp=from_unix_time_ms(market["pt"]),
                )
                assert trade_tick
                self.on_trade_tick(trade_tick)

        if market.get("marketDefinition", {}).get("status") == "CLOSED":
            for runner in market["marketDefinition"]["runners"]:
                kw = dict(
                    market_id=market_id,
                    selection_id=str(runner["id"]),
                    handicap=str(runner.get("hc") or "0.0"),
                )
                instrument = self.instrument_provider().get_betting_instrument(**kw)
                assert instrument
                # TODO - handle market closed
                # on_market_status()

                if runner["status"] == "LOSER":
                    # TODO - handle closing valuation = 0
                    pass
                elif runner["status"] == "WINNER":
                    # TODO handle closing valuation = 1
                    pass
        if (
            market.get("marketDefinition", {}).get("inPlay")
            and not market.get("marketDefinition", {}).get("status") == "CLOSED"
        ):
            for selection in market["marketDefinition"]["runners"]:
                kw = dict(
                    market_id=market_id,
                    spcelection_id=str(selection["id"]),
                    handicap=str(selection or "0.0"),
                )
                instrument = self.instrument_provider().get_betting_instrument(**kw)
                assert instrument
                # TODO - handle instrument status IN_PLAY


def on_market_update(self, raw):
    if raw.get("ct") == "HEARTBEAT":
        # TODO - Do we send out heartbeats
        return
    for mc in raw.get("mc", []):
        if mc.get("img"):
            return build_market_snapshot_messages(self, raw)
        else:
            return build_market_update_messages(self, raw)
