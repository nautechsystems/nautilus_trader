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

import datetime
import itertools
from collections import defaultdict
from functools import lru_cache
from typing import Dict, List, Union

import pandas as pd

from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B2N_ORDER_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B2N_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import B_ASK_KINDS
from nautilus_trader.adapters.betfair.common import B_BID_KINDS
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import MAX_BET_PROB
from nautilus_trader.adapters.betfair.common import MIN_BET_PROB
from nautilus_trader.adapters.betfair.common import N2B_SIDE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.util import hash_market_trade
from nautilus_trader.adapters.betfair.util import one
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.c_enums.order_type import OrderTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder


MILLIS_TO_NANOS = 1_000_000


def make_custom_order_ref(client_order_id: ClientOrderId, strategy_id: StrategyId) -> str:
    return client_order_id.value.rsplit("-" + strategy_id.get_tag(), maxsplit=1)[0]


def determine_order_price(order: Union[LimitOrder, MarketOrder]):
    """
    Determine the correct price to send for a given order. Betfair doesn't support market orders, so if this order is a
    MarketOrder, we generate a MIN/MAX price based on the side
    :param order:
    :return:
    """
    if isinstance(order, LimitOrder):
        return order.price
    elif isinstance(order, MarketOrder):
        if order.side == OrderSide.BUY:
            return MAX_BET_PROB
        else:
            return MIN_BET_PROB


def parse_betfair_timestamp(pt):
    return pt * MILLIS_TO_NANOS


def _probability_to_price(probability: Price, side: OrderSide):
    if side == OrderSide.BUY:
        tick_prob = BETFAIR_TICK_SCHEME.next_bid_price(value=probability)
    elif side == OrderSide.SELL:
        tick_prob = BETFAIR_TICK_SCHEME.next_ask_price(value=probability)
    else:
        raise RuntimeError(f"invalid OrderSide, was {side}")
    return probability_to_price(probability=tick_prob)


def _order_quantity_to_stake(quantity: Quantity) -> str:
    """
    Convert quantities from nautilus into liabilities in Betfair.
    """
    return str(quantity.as_double())


def _make_limit_order(order: Union[LimitOrder, MarketOrder]):
    price = str(float(_probability_to_price(probability=order.price, side=order.side)))
    size = _order_quantity_to_stake(quantity=order.quantity)

    if order.time_in_force == TimeInForce.AT_THE_CLOSE:
        return {
            "orderType": "LIMIT_ON_CLOSE",
            "limitOnCloseOrder": {"price": price, "liability": size},
        }
    else:
        parsed = {
            "orderType": "LIMIT",
            "limitOrder": {"price": price, "size": size, "persistenceType": "PERSIST"},
        }
        if order.time_in_force in N2B_TIME_IN_FORCE:
            parsed["limitOrder"]["timeInForce"] = N2B_TIME_IN_FORCE[order.time_in_force]  # type: ignore
            parsed["limitOrder"]["persistenceType"] = "LAPSE"  # type: ignore
        return parsed


def _make_market_order(order: Union[LimitOrder, MarketOrder]):
    if order.time_in_force == TimeInForce.AT_THE_CLOSE:
        return {
            "orderType": "MARKET_ON_CLOSE",
            "marketOnCloseOrder": {
                "liability": str(order.quantity.as_double()),
            },
        }
    else:
        # Betfair doesn't really support market orders, return a limit order with min/max price
        limit_order = LimitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            order_side=order.side,
            quantity=order.quantity,
            price=MAX_BET_PROB if order.side == OrderSide.BUY else MIN_BET_PROB,
            time_in_force=TimeInForce.FOK,
            init_id=order.init_id,
            ts_init=order.ts_init,
        )
        limit_order = _make_limit_order(order=limit_order)
        # We transform the size of a limit order inside `_make_limit_order` but for a market order we want to just use
        # the size as is.
        limit_order["limitOrder"]["size"] = str(order.quantity.as_double())
        return limit_order


def make_order(order: Union[LimitOrder, MarketOrder]):
    if isinstance(order, LimitOrder):
        return _make_limit_order(order=order)
    elif isinstance(order, MarketOrder):
        return _make_market_order(order=order)
    else:
        raise TypeError(f"Unknown order type: {type(order)}")


def order_submit_to_betfair(command: SubmitOrder, instrument: BettingInstrument) -> Dict:
    """
    Convert a SubmitOrder command into the data required by BetfairClient.
    """
    order = make_order(command.order)

    place_order = {
        "market_id": instrument.market_id,
        # Used to de-dupe orders on betfair server side
        "customer_ref": command.id.value.replace("-", ""),
        "customer_strategy_ref": command.strategy_id.value[:15],
        "instructions": [
            {
                **order,
                "selectionId": instrument.selection_id,
                "side": N2B_SIDE[command.order.side],
                "handicap": instrument.selection_handicap,
                # Remove the strategy name from customer_order_ref; it has a limited size and we don't control what
                # length the strategy might be or what characters users might append
                "customerOrderRef": make_custom_order_ref(
                    client_order_id=command.order.client_order_id,
                    strategy_id=command.strategy_id,
                ),
            }
        ],
    }
    return place_order


def order_update_to_betfair(
    command: ModifyOrder,
    venue_order_id: VenueOrderId,
    side: OrderSide,
    instrument: BettingInstrument,
):
    """
    Convert an ModifyOrder command into the data required by BetfairClient.
    """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [
            {
                "betId": venue_order_id.value,
                "newPrice": float(_probability_to_price(probability=command.price, side=side)),
            }
        ],
    }


def order_cancel_to_betfair(command: CancelOrder, instrument: BettingInstrument):
    """
    Convert a CancelOrder command into the data required by BetfairClient.
    """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [{"betId": command.venue_order_id.value}],
    }


def order_cancel_all_to_betfair(instrument: BettingInstrument):
    """
    Convert a CancelAllOrders command into the data required by BetfairClient.
    """
    return {
        "market_id": instrument.market_id,
    }


def betfair_account_to_account_state(
    account_detail,
    account_funds,
    event_id,
    ts_event,
    ts_init,
    account_id="001",
) -> AccountState:
    currency = Currency.from_str(account_detail["currencyCode"])
    balance = float(account_funds["availableToBetBalance"])
    locked = -float(account_funds["exposure"]) if account_funds["exposure"] else 0.0
    free = balance - locked
    return AccountState(
        account_id=AccountId(f"{BETFAIR_VENUE.value}-{account_id}"),
        account_type=AccountType.BETTING,
        base_currency=currency,
        reported=False,
        balances=[
            AccountBalance(
                total=Money(balance, currency),
                locked=Money(locked, currency),
                free=Money(free, currency),
            ),
        ],
        margins=[],
        info={"funds": account_funds, "detail": account_detail},
        event_id=event_id,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def _handle_market_snapshot(selection, instrument, ts_event, ts_init):
    updates = []
    # Check we only have one of [best bets / depth bets / all bets]
    bid_keys = [k for k in B_BID_KINDS if k in selection] or ["atb"]
    ask_keys = [k for k in B_ASK_KINDS if k in selection] or ["atl"]
    if set(bid_keys) == {"batb", "atb"}:
        bid_keys = ["atb"]
    if set(ask_keys) == {"batl", "atl"}:
        ask_keys = ["atl"]

    assert len(bid_keys) <= 1
    assert len(ask_keys) <= 1

    # OrderBook Snapshot
    # TODO(bm): Clean this up
    if bid_keys[0] == "atb":
        bids = selection.get("atb", [])
    else:
        bids = [(p, v) for _, p, v in selection.get(bid_keys[0], [])]
    if ask_keys[0] == "atl":
        asks = selection.get("atl", [])
    else:
        asks = [(p, v) for _, p, v in selection.get(ask_keys[0], [])]
    if bids or asks:
        snapshot = OrderBookSnapshot(
            book_type=BookType.L2_MBP,
            instrument_id=instrument.id,
            bids=[(price_to_probability(str(p)), v) for p, v in asks if p],
            asks=[(price_to_probability(str(p)), v) for p, v in bids if p],
            ts_event=ts_event,
            ts_init=ts_init,
        )
        updates.append(snapshot)
    if "trd" in selection:
        updates.extend(
            _handle_market_trades(
                runner=selection,
                instrument=instrument,
                ts_event=ts_event,
                ts_init=ts_init,
            )
        )
    if "spb" in selection or "spl" in selection:
        updates.extend(
            _handle_bsp_updates(
                runner=selection,
                instrument=instrument,
                ts_event=ts_event,
                ts_init=ts_init,
            )
        )

    return updates


def _handle_market_trades(
    runner,
    instrument,
    ts_event,
    ts_init,
):
    trade_ticks = []
    for price, volume in runner.get("trd", []):
        if volume == 0:
            continue
        # TODO - should we use clk here for ID instead of the hash?
        # Betfair doesn't publish trade ids, so we make our own
        trade_id = hash_market_trade(timestamp=ts_event, price=price, volume=volume)
        tick = TradeTick(
            instrument_id=instrument.id,
            price=price_to_probability(str(price)),
            size=Quantity(volume, precision=BETFAIR_QUANTITY_PRECISION),
            aggressor_side=AggressorSide.UNKNOWN,
            trade_id=TradeId(trade_id),
            ts_event=ts_event,
            ts_init=ts_init,
        )
        trade_ticks.append(tick)
    return trade_ticks


def _handle_bsp_updates(runner, instrument, ts_event, ts_init):
    updates = []
    for side in ("spb", "spl"):
        for upd in runner.get(side, []):
            price, volume = upd
            delta = BSPOrderBookDelta(
                instrument_id=instrument.id,
                book_type=BookType.L2_MBP,
                action=BookAction.DELETE if volume == 0 else BookAction.UPDATE,
                order=Order(
                    price=price_to_probability(str(price)),
                    size=Quantity(volume, precision=BETFAIR_QUANTITY_PRECISION),
                    side=B2N_MARKET_STREAM_SIDE[side],
                ),
                ts_event=ts_event,
                ts_init=ts_init,
            )
            updates.append(delta)
    return updates


def _handle_book_updates(runner, instrument, ts_event, ts_init):
    deltas = []
    for side in B_SIDE_KINDS:
        for upd in runner.get(side, []):
            # TODO(bm): Clean this up
            if len(upd) == 3:
                _, price, volume = upd
            else:
                price, volume = upd
            if price == 0.0:
                continue
            deltas.append(
                OrderBookDelta(
                    instrument_id=instrument.id,
                    book_type=BookType.L2_MBP,
                    action=BookAction.DELETE if volume == 0 else BookAction.UPDATE,
                    order=Order(
                        price=price_to_probability(str(price)),
                        size=Quantity(volume, precision=BETFAIR_QUANTITY_PRECISION),
                        side=B2N_MARKET_STREAM_SIDE[side],
                    ),
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
            )
    if deltas:
        ob_update = OrderBookDeltas(
            book_type=BookType.L2_MBP,
            instrument_id=instrument.id,
            deltas=deltas,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        return [ob_update]
    else:
        return []


def _handle_market_close(runner, instrument, ts_event, ts_init):
    if runner["status"] in ("LOSER", "REMOVED"):
        close_price = InstrumentClosePrice(
            instrument_id=instrument.id,
            close_price=Price(0.0, precision=4),
            close_type=InstrumentCloseType.EXPIRED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif runner["status"] in ("WINNER", "PLACED"):
        close_price = InstrumentClosePrice(
            instrument_id=instrument.id,
            close_price=Price(1.0, precision=4),
            close_type=InstrumentCloseType.EXPIRED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        raise ValueError(f"Unknown runner close status: {runner['status']}")
    return [close_price]


def _handle_instrument_status(market, runner, instrument, ts_event, ts_init):
    market_def = market.get("marketDefinition", {})
    if "status" not in market_def:
        return []
    if runner.get("status") == "REMOVED":
        status = InstrumentStatusUpdate(
            instrument_id=instrument.id,
            status=InstrumentStatus.CLOSED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def["status"] == "OPEN" and not market_def["inPlay"]:
        status = InstrumentStatusUpdate(
            instrument_id=instrument.id,
            status=InstrumentStatus.PRE_OPEN,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def["status"] == "OPEN" and market_def["inPlay"]:
        status = InstrumentStatusUpdate(
            instrument_id=instrument.id,
            status=InstrumentStatus.OPEN,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def["status"] == "SUSPENDED":
        status = InstrumentStatusUpdate(
            instrument_id=instrument.id,
            status=InstrumentStatus.PAUSE,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def["status"] == "CLOSED":
        status = InstrumentStatusUpdate(
            instrument_id=instrument.id,
            status=InstrumentStatus.CLOSED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        raise ValueError("Unknown market status")
    return [status]


def _handle_market_runners_status(instrument_provider, market, ts_event, ts_init):
    updates = []

    for runner in market.get("marketDefinition", {}).get("runners", []):
        kw = dict(
            market_id=market["id"],
            selection_id=str(runner["id"]),
            handicap=parse_handicap(runner.get("hc")),
        )
        instrument = instrument_provider.get_betting_instrument(**kw)
        if instrument is None:
            continue
        updates.extend(
            _handle_instrument_status(
                market=market,
                runner=runner,
                instrument=instrument,
                ts_event=ts_event,
                ts_init=ts_init,
            )
        )
        if market["marketDefinition"].get("status") == "CLOSED":
            updates.extend(
                _handle_market_close(
                    runner=runner,
                    instrument=instrument,
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
            )
    return updates


def _handle_ticker(runner: dict, instrument: BettingInstrument, ts_event, ts_init):
    last_traded_price, traded_volume = None, None
    if "ltp" in runner:
        last_traded_price = price_to_probability(str(runner["ltp"]))
    if "tv" in runner:
        traded_volume = Quantity(value=runner.get("tv"), precision=BETFAIR_QUANTITY_PRECISION)
    return BetfairTicker(
        instrument_id=instrument.id,
        last_traded_price=last_traded_price,
        traded_volume=traded_volume,
        ts_init=ts_init,
        ts_event=ts_event,
    )


def build_market_snapshot_messages(
    instrument_provider, raw
) -> List[Union[OrderBookSnapshot, InstrumentStatusUpdate]]:
    updates = []
    ts_event = parse_betfair_timestamp(raw["pt"])

    for market in raw.get("mc", []):
        # Instrument Status
        updates.extend(
            _handle_market_runners_status(
                instrument_provider=instrument_provider,
                market=market,
                ts_event=ts_event,
                ts_init=ts_event,
            )
        )

        # OrderBook snapshots
        if market.get("img") is True:
            market_id = market["id"]
            for (selection_id, handicap), selections in itertools.groupby(
                market.get("rc", []), lambda x: (x["id"], x.get("hc"))
            ):
                for selection in list(selections):
                    kw = dict(
                        market_id=market_id,
                        selection_id=str(selection_id),
                        handicap=parse_handicap(handicap),
                    )
                    instrument = instrument_provider.get_betting_instrument(**kw)
                    if instrument is None:
                        continue
                    updates.extend(
                        _handle_market_snapshot(
                            selection=selection,
                            instrument=instrument,
                            ts_event=ts_event,
                            ts_init=ts_event,
                        )
                    )
    return updates


def _merge_order_book_deltas(all_deltas: List[OrderBookDeltas]):
    per_instrument_deltas = defaultdict(list)
    book_type = one(set(deltas.book_type for deltas in all_deltas))
    ts_event = one(set(deltas.ts_event for deltas in all_deltas))
    ts_init = one(set(deltas.ts_init for deltas in all_deltas))

    for deltas in all_deltas:
        per_instrument_deltas[deltas.instrument_id].extend(deltas.deltas)
    return [
        OrderBookDeltas(
            instrument_id=instrument_id,
            deltas=deltas,
            book_type=book_type,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        for instrument_id, deltas in per_instrument_deltas.items()
    ]


def build_market_update_messages(
    instrument_provider, raw
) -> List[Union[OrderBookDelta, TradeTick, InstrumentStatusUpdate, InstrumentClosePrice]]:
    updates = []
    book_updates = []
    ts_event = parse_betfair_timestamp(raw["pt"])

    for market in raw.get("mc", []):
        updates.extend(
            _handle_market_runners_status(
                instrument_provider=instrument_provider,
                market=market,
                ts_event=ts_event,
                ts_init=ts_event,
            )
        )
        for runner in market.get("rc", []):
            kw = dict(
                market_id=market["id"],
                selection_id=str(runner["id"]),
                handicap=parse_handicap(runner.get("hc")),
            )
            instrument = instrument_provider.get_betting_instrument(**kw)
            if instrument is None:
                continue
            # Delay appending book updates until we can merge at the end
            book_updates.extend(
                _handle_book_updates(
                    runner=runner,
                    instrument=instrument,
                    ts_event=ts_event,
                    ts_init=ts_event,
                )
            )

            if "trd" in runner:
                updates.extend(
                    _handle_market_trades(
                        runner=runner,
                        instrument=instrument,
                        ts_event=ts_event,
                        ts_init=ts_event,
                    )
                )
            if "ltp" in runner or "tv" in runner:
                updates.append(
                    _handle_ticker(
                        runner=runner,
                        instrument=instrument,
                        ts_event=ts_event,
                        ts_init=ts_event,
                    )
                )

            if "spb" in runner or "spl" in runner:
                updates.extend(
                    _handle_bsp_updates(
                        runner=runner,
                        instrument=instrument,
                        ts_event=ts_event,
                        ts_init=ts_event,
                    )
                )
    if book_updates:
        updates.extend(_merge_order_book_deltas(book_updates))
    return updates


def on_market_update(instrument_provider, update: dict):
    if update.get("ct") == "HEARTBEAT":
        # TODO - Should we send out heartbeats
        return []
    for mc in update.get("mc", []):
        if mc.get("img"):
            return build_market_snapshot_messages(instrument_provider, update)
        else:
            return build_market_update_messages(instrument_provider, update)
    return []


async def generate_trades_list(
    self, venue_order_id: VenueOrderId, symbol: Symbol, since: datetime = None  # type: ignore
) -> List[TradeReport]:
    filled = self.client().betting.list_cleared_orders(
        bet_ids=[venue_order_id],
    )
    if not filled["clearedOrders"]:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled["clearedOrders"][0]
    ts_event = int(pd.Timestamp(fill["lastMatchedDate"]).to_datetime64())
    return [
        TradeReport(
            client_order_id=self.venue_order_id_to_client_order_id[venue_order_id],
            venue_order_id=VenueOrderId(fill["betId"]),
            venue_position_id=None,  # Can be None
            trade_id=TradeId(fill["lastMatchedDate"]),
            last_qty=Quantity.from_str(str(fill["sizeSettled"])),  # TODO: Incorrect precision?
            last_px=Price.from_str(str(fill["priceMatched"])),  # TODO: Incorrect precision?
            commission=None,  # Can be None
            liquidity_side=LiquiditySide.NONE,
            ts_event=ts_event,
            ts_init=ts_event,
        )
    ]


@lru_cache(None)
def parse_handicap(x) -> str:
    """
    Ensure consistent parsing of the various handicap sources we get.
    """
    if x in (None, ""):
        return "0.0"
    if isinstance(x, (int, str)):
        return str(float(x))
    elif isinstance(x, float):
        return str(x)
    else:
        raise TypeError(f"Unexpected type ({type(x)}) for handicap: {x}")


def bet_to_order_status_report(
    order,
    account_id: AccountId,
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    client_order_id: ClientOrderId,
    ts_init,
    report_id,
) -> OrderStatusReport:
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        venue_order_id=venue_order_id,
        client_order_id=client_order_id,
        order_side=B2N_ORDER_STREAM_SIDE[order["side"]],
        order_type=OrderTypeParser.from_str_py(order["orderType"]),
        contingency_type=ContingencyType.NONE,
        time_in_force=B2N_TIME_IN_FORCE[order["persistenceType"]],
        order_status=determine_order_status(order),
        price=price_to_probability(str(order["priceSize"]["price"])),
        quantity=Quantity(order["priceSize"]["size"], BETFAIR_QUANTITY_PRECISION),
        filled_qty=Quantity(order["sizeMatched"], BETFAIR_QUANTITY_PRECISION),
        report_id=report_id,
        ts_accepted=dt_to_unix_nanos(pd.Timestamp(order["placedDate"])),
        ts_triggered=0,
        ts_last=dt_to_unix_nanos(pd.Timestamp(order["matchedDate"]))
        if "matchedDate" in order
        else 0,
        ts_init=ts_init,
    )


def determine_order_status(order: Dict) -> OrderStatus:
    order_size = order["priceSize"]["size"]
    if order["status"] == "EXECUTION_COMPLETE":
        if order_size == order["sizeMatched"]:
            return OrderStatus.FILLED
        elif order["sizeCancelled"] > 0.0:
            return OrderStatus.CANCELED
        else:
            return OrderStatus.PARTIALLY_FILLED
    elif order["status"] == "EXECUTABLE":
        if order["sizeMatched"] == 0.0:
            return OrderStatus.ACCEPTED
        elif order["sizeMatched"] > 0.0:
            return OrderStatus.PARTIALLY_FILLED
