import datetime
import itertools
from typing import List, Optional, Union

from betfairlightweight.filters import cancel_instruction
from betfairlightweight.filters import limit_order
from betfairlightweight.filters import place_instruction
from betfairlightweight.filters import replace_instruction

from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B_ASK_KINDS
from nautilus_trader.adapters.betfair.common import B_BID_KINDS
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import N2B_SIDE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.core.datetime import from_unix_time_ms
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.c_enums.orderbook_op import OrderBookOperationType
from nautilus_trader.model.commands import AmendOrder
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import Symbol
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
                    price=float(
                        probability_to_price(probability=order.price, side=order.side)
                    ),
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


def build_market_snapshot_messages(
    raw, instrument_provider: BetfairInstrumentProvider
) -> List[OrderBookSnapshot]:
    updates = []
    for market in raw.get("mc", []):
        # Market status events
        # market_definition = market.get("marketDefinition", {})
        # TODO - Need to handle instrument status = CLOSED here

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
                    instrument = instrument_provider.get_betting_instrument(**kw)
                    # Check we only have one of [best bets / depth bets / all bets]
                    bid_keys = [k for k in B_BID_KINDS if k in selection] or ["atb"]
                    ask_keys = [k for k in B_ASK_KINDS if k in selection] or ["atb"]
                    assert len(bid_keys) <= 1
                    assert len(ask_keys) <= 1
                    # TODO Clean this crap up
                    if bid_keys[0] == "atb":
                        bids = selection.get("atb", [])
                    else:
                        bids = [(p, v) for _, p, v in selection.get(bid_keys[0], [])]
                    if ask_keys[0] == "atl":
                        asks = selection.get("atl", [])
                    else:
                        asks = [(p, v) for _, p, v in selection.get(ask_keys[0], [])]
                    snapshot = OrderBookSnapshot(
                        level=OrderBookLevel.L2,
                        instrument_id=instrument.id,
                        bids=[
                            (price_to_probability(p, OrderSide.BUY), v) for p, v in bids
                        ],
                        asks=[
                            (price_to_probability(p, OrderSide.SELL), v)
                            for p, v in asks
                        ],
                        timestamp=from_unix_time_ms(raw["pt"]),
                    )
                    updates.append(snapshot)
    return updates


def build_market_update_messages(
    raw, instrument_provider: BetfairInstrumentProvider
) -> List[Union[OrderBookOperation, TradeTick]]:
    updates = []
    for market in raw.get("mc", []):
        market_id = market["id"]
        for runner in market.get("rc", []):
            kw = dict(
                market_id=market_id,
                selection_id=str(runner["id"]),
                handicap=str(runner.get("hc") or "0.0"),
            )
            instrument = instrument_provider.get_betting_instrument(**kw)
            assert instrument
            operations = []
            for side in B_SIDE_KINDS:
                for level, price, volume in runner.get(side, []):
                    operations.append(
                        OrderBookOperation(
                            op_type=OrderBookOperationType.DELETE
                            if volume == 0
                            else OrderBookOperationType.UPDATE,
                            order=Order(
                                price=price_to_probability(
                                    price, side=B2N_MARKET_STREAM_SIDE[side]
                                ),
                                volume=volume,
                                side=B2N_MARKET_STREAM_SIDE[side],
                            ),
                        )
                    )
            ob_update = OrderBookOperations(
                level=OrderBookLevel.L2,
                instrument_id=instrument.id,
                ops=operations,
                timestamp=datetime.datetime.utcfromtimestamp(raw["pt"] / 1e3),
            )
            updates.append(ob_update)

            for price, volume in runner.get("trd", []):
                trade_tick = TradeTick(
                    instrument_id=instrument.id,
                    price=Price(price_to_probability(price)),
                    quantity=Quantity(volume),
                    side=OrderSide.BUY,
                    # TradeMatchId(trade_match_id),
                    timestamp=from_unix_time_ms(raw["pt"]),
                )
                updates.append(trade_tick)

        if market.get("marketDefinition", {}).get("status") == "CLOSED":
            for runner in market["marketDefinition"]["runners"]:
                kw = dict(
                    market_id=market_id,
                    selection_id=str(runner["id"]),
                    handicap=str(runner.get("hc") or "0.0"),
                )
                instrument = instrument_provider.get_betting_instrument(**kw)
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
                    selection_id=str(selection["id"]),
                    handicap=str(selection.get("hc") or "0.0"),
                )
                instrument = instrument_provider.get_betting_instrument(**kw)
                assert instrument
                # TODO - handle instrument status IN_PLAY
    return updates


def on_market_update(update: dict, instrument_provider: BetfairInstrumentProvider):
    if update.get("ct") == "HEARTBEAT":
        # TODO - Do we send out heartbeats
        return []
    for mc in update.get("mc", []):
        if mc.get("img"):
            return build_market_snapshot_messages(
                update, instrument_provider=instrument_provider
            )
        else:
            return build_market_update_messages(
                update, instrument_provider=instrument_provider
            )
    return []


# TODO - Need to handle pagination > 1000 orders
async def generate_order_status_report(self) -> Optional[OrderStatusReport]:
    return [
        # OrderStatusReport(
        #     cl_ord_id=ClientOrderId(),
        #     order_id=OrderId(),
        #     order_stat=OrderState(),
        #     filled_qty=Quantity(),
        #     timestamp=from_unix_time_ms(),
        # )
        # for order in self.client().betting.list_current_orders()["currentOrders"]
    ]


async def generate_trades_list(
    self, order_id: OrderId, symbol: Symbol, since: datetime = None
) -> List[ExecutionReport]:
    # filled = self.client().betting.list_cleared_orders()
    # return [ExecutionReport()]
    return []
