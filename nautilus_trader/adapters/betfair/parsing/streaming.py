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
from typing import Optional, Union

import pandas as pd
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming.mcm import MCM
from betfair_parser.spec.streaming.mcm import BestAvailableToBack
from betfair_parser.spec.streaming.mcm import BestAvailableToLay
from betfair_parser.spec.streaming.mcm import MarketChange
from betfair_parser.spec.streaming.mcm import MarketDefinition
from betfair_parser.spec.streaming.mcm import Runner
from betfair_parser.spec.streaming.mcm import RunnerChange

from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B_ASK_KINDS
from nautilus_trader.adapters.betfair.common import B_BID_KINDS
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id
from nautilus_trader.adapters.betfair.parsing.requests import parse_handicap
from nautilus_trader.adapters.betfair.util import hash_market_trade
from nautilus_trader.adapters.betfair.util import one
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


def _handle_market_snapshot(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
):
    updates = []
    # Check we only have one of [best bets / depth bets / all bets]
    bid_keys = [k for k in B_BID_KINDS if getattr(rc, k)] or ["atb"]
    ask_keys = [k for k in B_ASK_KINDS if getattr(rc, k)] or ["atl"]
    if set(bid_keys) == {"batb", "atb"}:
        bid_keys = ["atb"]
    if set(ask_keys) == {"batl", "atl"}:
        ask_keys = ["atl"]

    bid_key = one(bid_keys)
    ask_key = one(ask_keys)

    # OrderBook Snapshot
    bids: list[BestAvailableToBack] = getattr(rc, bid_key)
    asks: list[BestAvailableToLay] = getattr(rc, ask_key)

    if bids or asks:
        bid_tuple = [
            (price_to_probability(str(order.price)), order.volume) for order in asks if order.price
        ]
        ask_tuple = [
            (price_to_probability(str(order.price)), order.volume) for order in bids if order.price
        ]

        snapshot = OrderBookSnapshot(
            book_type=BookType.L2_MBP,
            instrument_id=instrument_id,
            bids=bid_tuple,
            asks=ask_tuple,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        updates.append(snapshot)
    if rc.trd:
        updates.extend(
            _handle_market_trades(
                rc=rc,
                instrument_id=instrument_id,
                ts_event=ts_event,
                ts_init=ts_init,
            ),
        )
    if rc.spb or rc.spl:
        updates.extend(
            _handle_bsp_updates(
                rc=rc,
                instrument_id=instrument_id,
                ts_event=ts_event,
                ts_init=ts_init,
            ),
        )

    return updates


def _handle_market_trades(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
):
    trade_ticks = []
    for trd in rc.trd:
        if trd.volume == 0:
            continue
        # TODO - should we use clk here for ID instead of the hash?
        # Betfair doesn't publish trade ids, so we make our own
        trade_id = hash_market_trade(timestamp=ts_event, price=trd.price, volume=trd.volume)
        tick = TradeTick(
            instrument_id=instrument_id,
            price=price_to_probability(str(trd.price)),
            size=Quantity(trd.volume, precision=BETFAIR_QUANTITY_PRECISION),
            aggressor_side=AggressorSide.NONE,
            trade_id=TradeId(trade_id),
            ts_event=ts_event,
            ts_init=ts_init,
        )
        trade_ticks.append(tick)
    return trade_ticks


def _handle_bsp_updates(rc: RunnerChange, instrument_id: InstrumentId, ts_event, ts_init):
    updates = []
    for side, starting_prices in zip(("spb", "spl"), (rc.spb, rc.spl)):
        for sp in starting_prices:
            delta = BSPOrderBookDelta(
                instrument_id=instrument_id,
                book_type=BookType.L2_MBP,
                action=BookAction.DELETE if sp.volume == 0 else BookAction.UPDATE,
                order=BookOrder(
                    price=price_to_probability(str(sp.price)),
                    size=Quantity(sp.volume, precision=BETFAIR_QUANTITY_PRECISION),
                    side=B2N_MARKET_STREAM_SIDE[side],
                ),
                ts_event=ts_event,
                ts_init=ts_init,
            )
            updates.append(delta)
    return updates


def _handle_book_updates(runner: RunnerChange, instrument_id: InstrumentId, ts_event, ts_init):
    deltas = []
    for side in B_SIDE_KINDS:
        for upd in getattr(runner, side, []):
            # TODO(bm): Clean this up
            if len(upd) == 3:
                _, price, volume = upd
            else:
                price, volume = upd
            if price == 0.0:
                continue
            deltas.append(
                OrderBookDelta(
                    instrument_id=instrument_id,
                    book_type=BookType.L2_MBP,
                    action=BookAction.DELETE if volume == 0 else BookAction.UPDATE,
                    order=BookOrder(
                        price=price_to_probability(str(price)),
                        size=Quantity(volume, precision=BETFAIR_QUANTITY_PRECISION),
                        side=B2N_MARKET_STREAM_SIDE[side],
                    ),
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )
    if deltas:
        ob_update = OrderBookDeltas(
            book_type=BookType.L2_MBP,
            instrument_id=instrument_id,
            deltas=deltas,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        return [ob_update]
    else:
        return []


def _handle_market_close(runner: Runner, instrument_id: InstrumentId, ts_event, ts_init):
    if runner.status in ("LOSER", "REMOVED"):
        close_price = InstrumentClosePrice(
            instrument_id=instrument_id,
            close_price=Price(0.0, precision=4),
            close_type=InstrumentCloseType.EXPIRED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif runner.status in ("WINNER", "PLACED"):
        close_price = InstrumentClosePrice(
            instrument_id=instrument_id,
            close_price=Price(1.0, precision=4),
            close_type=InstrumentCloseType.EXPIRED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        raise ValueError(f"Unknown runner close status: {runner.status}")
    return [close_price]


def _handle_instrument_status(
    mc: MarketChange,
    runner: Runner,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
):
    market_def = mc.marketDefinition
    if not market_def.status:
        return []
    if runner.status == "REMOVED":
        status = InstrumentStatusUpdate(
            instrument_id=instrument_id,
            status=InstrumentStatus.CLOSED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def.status == "OPEN" and not market_def.inPlay:
        status = InstrumentStatusUpdate(
            instrument_id=instrument_id,
            status=InstrumentStatus.PRE_OPEN,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def.status == "OPEN" and market_def.inPlay:
        status = InstrumentStatusUpdate(
            instrument_id=instrument_id,
            status=InstrumentStatus.OPEN,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def.status == "SUSPENDED":
        status = InstrumentStatusUpdate(
            instrument_id=instrument_id,
            status=InstrumentStatus.PAUSE,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif market_def.status == "CLOSED":
        status = InstrumentStatusUpdate(
            instrument_id=instrument_id,
            status=InstrumentStatus.CLOSED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        raise ValueError("Unknown market status")
    return [status]


def _handle_market_runners_status(mc: MarketChange, ts_event: int, ts_init: int):
    updates = []
    for runner in mc.marketDefinition.runners:
        instrument_id = betfair_instrument_id(
            market_id=mc.id,
            selection_id=str(runner.id),
            selection_handicap=parse_handicap(runner.hc),
        )
        updates.extend(
            _handle_instrument_status(
                mc=mc,
                runner=runner,
                instrument_id=instrument_id,
                ts_event=ts_event,
                ts_init=ts_init,
            ),
        )
        if mc.marketDefinition.status == "CLOSED":
            updates.extend(
                _handle_market_close(
                    runner=runner,
                    instrument_id=instrument_id,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )
    return updates


def _handle_ticker(runner: RunnerChange, instrument_id: InstrumentId, ts_event, ts_init):
    last_traded_price, traded_volume = None, None
    if runner.ltp:
        last_traded_price = price_to_probability(str(runner.ltp))
    if runner.tv:
        traded_volume = Quantity(value=runner.tv, precision=BETFAIR_QUANTITY_PRECISION)
    return BetfairTicker(
        instrument_id=instrument_id,
        last_traded_price=last_traded_price,
        traded_volume=traded_volume,
        ts_init=ts_init,
        ts_event=ts_event,
    )


def build_market_snapshot_messages(
    mc: MarketChange,
    ts_event: int,
    ts_init: int,
) -> list[Union[OrderBookSnapshot, InstrumentStatusUpdate]]:
    updates = []
    # OrderBook snapshots
    if mc.img is True:
        for _, runners in itertools.groupby(mc.rc, lambda x: (x.id, x.hc)):
            runners: list[RunnerChange]  # type: ignore
            for rc in list(runners):
                instrument_id = betfair_instrument_id(
                    market_id=mc.id,
                    selection_id=str(rc.id),
                    selection_handicap=parse_handicap(rc.hc),
                )

                updates.extend(
                    _handle_market_snapshot(
                        rc=rc,
                        instrument_id=instrument_id,
                        ts_event=ts_event,
                        ts_init=ts_init,
                    ),
                )
    return updates


def _merge_order_book_deltas(all_deltas: list[OrderBookDeltas]):
    per_instrument_deltas = defaultdict(list)
    book_type = one({deltas.book_type for deltas in all_deltas})
    ts_event = one({deltas.ts_event for deltas in all_deltas})
    ts_init = one({deltas.ts_init for deltas in all_deltas})

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
    mc: MarketChange,
    ts_event: int,
    ts_init: int,
) -> list[Union[OrderBookDelta, TradeTick, InstrumentStatusUpdate, InstrumentClosePrice]]:
    updates = []
    book_updates = []

    for rc in mc.rc:
        instrument_id = betfair_instrument_id(
            market_id=mc.id,
            selection_id=str(rc.id),
            selection_handicap=parse_handicap(rc.hc),
        )

        # Delay appending book updates until we can merge at the end
        book_updates.extend(
            _handle_book_updates(
                runner=rc,
                instrument_id=instrument_id,
                ts_event=ts_event,
                ts_init=ts_init,
            ),
        )

        if rc.trd:
            updates.extend(
                _handle_market_trades(
                    rc=rc,
                    instrument_id=instrument_id,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )
        if rc.ltp or rc.tv:
            updates.append(
                _handle_ticker(
                    runner=rc,
                    instrument_id=instrument_id,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )

        if rc.spb or rc.spl:
            updates.extend(
                _handle_bsp_updates(
                    rc=rc,
                    instrument_id=instrument_id,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )
    if book_updates:
        updates.extend(_merge_order_book_deltas(book_updates))
    return updates


PARSE_TYPES = Union[
    InstrumentStatusUpdate,
    InstrumentClosePrice,
    OrderBookSnapshot,
    OrderBookDeltas,
    TradeTick,
    BetfairTicker,
    BSPOrderBookDelta,
]


class BetfairParser:
    """Stateful parser that keeps market definition"""

    def __init__(self):
        self.market_definitions: dict[str, MarketDefinition] = {}

    def parse(self, mcm: MCM, ts_init: Optional[int] = None) -> list[PARSE_TYPES]:
        if isinstance(mcm, (Status, Connection, OCM)):
            return []
        if mcm.is_heartbeat:
            return []
        updates = []
        ts_event = millis_to_nanos(mcm.pt)
        ts_init = ts_init or ts_event
        for mc in mcm.mc:
            if mc.marketDefinition is not None:
                self.market_definitions[mc.id] = mc.marketDefinition
                updates.extend(_handle_market_runners_status(mc, ts_event, ts_init))
            if mc.img:
                updates.extend(build_market_snapshot_messages(mc, ts_event, ts_init))
            else:
                upd = build_market_update_messages(mc, ts_event, ts_init)
                updates.extend(upd)
        return updates


async def generate_trades_list(
    self,
    venue_order_id: VenueOrderId,
    symbol: Symbol,
    since: datetime = None,  # type: ignore
) -> list[TradeReport]:
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
        ),
    ]
