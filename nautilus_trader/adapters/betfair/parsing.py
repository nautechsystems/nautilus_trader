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

import datetime
import hashlib
import itertools
from collections import defaultdict
from functools import lru_cache
from typing import List, Optional, Union

import orjson
import pandas as pd

from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B_ASK_KINDS
from nautilus_trader.adapters.betfair.common import B_BID_KINDS
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import MAX_BET_PROB
from nautilus_trader.adapters.betfair.common import MIN_BET_PROB
from nautilus_trader.adapters.betfair.common import N2B_SIDE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.util import hash_json
from nautilus_trader.adapters.betfair.util import one
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import ModifyOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import Symbol
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


uuid_factory = UUIDFactory()
MILLIS_TO_NANOS = 1_000_000
SECS_TO_NANOS = 1_000_000_000


def make_custom_order_ref(client_order_id, strategy_id):
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


def _make_limit_order(order: Union[LimitOrder, MarketOrder]):
    price = determine_order_price(order)
    price = str(float(probability_to_price(probability=price, side=order.side)))
    size = str(float(order.quantity))
    if order.time_in_force == TimeInForce.OC:
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
            parsed["limitOrder"]["timeInForce"] = N2B_TIME_IN_FORCE[  # type: ignore
                order.time_in_force
            ]
            parsed["limitOrder"]["persistenceType"] = "LAPSE"  # type: ignore
        return parsed


def _make_market_order(order: Union[LimitOrder, MarketOrder]):
    if order.time_in_force == TimeInForce.OC:
        return {
            "orderType": "MARKET_ON_CLOSE",
            "marketOnCloseOrder": {"liability": str(float(order.quantity))},
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
            expire_time=None,
            init_id=order.init_id,
            ts_init=order.ts_init,
        )
        return _make_limit_order(order=limit_order)


def make_order(order: Union[LimitOrder, MarketOrder]):
    if isinstance(order, LimitOrder):
        return _make_limit_order(order=order)
    elif isinstance(order, MarketOrder):
        return _make_market_order(order=order)
    else:
        raise TypeError(f"Unknown order type: {type(order)}")


def order_submit_to_betfair(command: SubmitOrder, instrument: BettingInstrument):
    """
    Convert a SubmitOrder command into the data required by BetfairClient
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
    Convert an ModifyOrder command into the data required by BetfairClient
    """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [
            {
                "betId": venue_order_id.value,
                "newPrice": float(probability_to_price(probability=command.price, side=side)),
            }
        ],
    }


def order_cancel_to_betfair(command: CancelOrder, instrument: BettingInstrument):
    """
    Convert a SubmitOrder command into the data required by BetfairClient
    """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [{"betId": command.venue_order_id.value}],
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
        account_id=AccountId(issuer=BETFAIR_VENUE.value, number=account_id),
        account_type=AccountType.CASH,
        base_currency=currency,
        reported=False,
        balances=[
            AccountBalance(
                currency=currency,
                total=Money(balance, currency),
                locked=Money(locked, currency),
                free=Money(free, currency),
            ),
        ],
        info={"funds": account_funds, "detail": account_detail},
        event_id=event_id,
        ts_event=ts_event,
        ts_init=ts_init,
    )


EXECUTION_ID_KEYS = ("id", "p", "s", "side", "pt", "ot", "pd", "md", "avp", "sm")  # noqa:


def betfair_execution_id(uo) -> ExecutionId:
    data = orjson.dumps({k: uo[k] for k in EXECUTION_ID_KEYS if uo.get(k)})
    hsh = hashlib.sha1(data).hexdigest()  # noqa: S303
    return ExecutionId(hsh)


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
            level=BookLevel.L2,
            instrument_id=instrument.id,
            bids=[(price_to_probability(p, OrderSide.BUY), v) for p, v in asks],
            asks=[(price_to_probability(p, OrderSide.SELL), v) for p, v in bids],
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
        # Betfair doesn't publish trade ids, so we make our own
        # TODO - should we use clk here for ID instead of the hash?
        trade_id = hash_json(data=(ts_event, price, volume))
        tick = TradeTick(
            instrument_id=instrument.id,
            price=price_to_probability(price, force=True),  # Already wrapping in Price
            size=Quantity(volume, precision=4),
            aggressor_side=AggressorSide.UNKNOWN,
            match_id=trade_id,
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
                level=BookLevel.L2,
                action=BookAction.DELETE if volume == 0 else BookAction.UPDATE,
                order=Order(
                    price=price_to_probability(price, side=B2N_MARKET_STREAM_SIDE[side]),
                    size=Quantity(volume, precision=8),
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
            deltas.append(
                OrderBookDelta(
                    instrument_id=instrument.id,
                    level=BookLevel.L2,
                    action=BookAction.DELETE if volume == 0 else BookAction.UPDATE,
                    order=Order(
                        price=price_to_probability(price, side=B2N_MARKET_STREAM_SIDE[side]),
                        size=Quantity(volume, precision=8),
                        side=B2N_MARKET_STREAM_SIDE[side],
                    ),
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
            )
    if deltas:
        ob_update = OrderBookDeltas(
            level=BookLevel.L2,
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
        last_traded_price = price_to_probability(runner["ltp"], side=B2N_MARKET_STREAM_SIDE["atb"])
    if "tv" in runner:
        traded_volume = Quantity(value=runner.get("tv"), precision=instrument.size_precision)
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
    level = one(set(deltas.level for deltas in all_deltas))
    ts_event = one(set(deltas.ts_event for deltas in all_deltas))
    ts_init = one(set(deltas.ts_init for deltas in all_deltas))

    for deltas in all_deltas:
        per_instrument_deltas[deltas.instrument_id].extend(deltas.deltas)
    return [
        OrderBookDeltas(
            instrument_id=instrument_id,
            deltas=deltas,
            level=level,
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


async def generate_order_status_report(self, order) -> Optional[OrderStatusReport]:
    return [
        OrderStatusReport(
            client_order_id=ClientOrderId(),
            venue_order_id=VenueOrderId(),
            order_status=OrderStatus(),
            filled_qty=Quantity.zero(),
            ts_init=SECS_TO_NANOS * pd.Timestamp(order["timestamp"]).timestamp(),
        )
        for order in self.client().betting.list_current_orders()["currentOrders"]
    ]


async def generate_trades_list(
    self, venue_order_id: VenueOrderId, symbol: Symbol, since: datetime = None  # type: ignore
) -> List[ExecutionReport]:
    filled = self.client().betting.list_cleared_orders(
        bet_ids=[venue_order_id],
    )
    if not filled["clearedOrders"]:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled["clearedOrders"][0]
    ts_event = SECS_TO_NANOS * pd.Timestamp(fill["lastMatchedDate"]).timestamp()
    return [
        ExecutionReport(
            client_order_id=self.venue_order_id_to_client_order_id[venue_order_id],
            venue_order_id=VenueOrderId(fill["betId"]),
            venue_position_id=None,  # Can be None
            execution_id=ExecutionId(fill["lastMatchedDate"]),
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
    Ensure consistent parsing of the various handicap sources we get
    """
    if x in (None, ""):
        return "0.0"
    if isinstance(x, (int, str)):
        return str(float(x))
    elif isinstance(x, float):
        return str(x)
    else:
        raise TypeError(f"Unexpected type ({type(x)}) for handicap: {x}")
