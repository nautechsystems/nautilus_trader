# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from betfair_parser.spec.streaming.mcm import RunnerStatus

from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B_ASK_KINDS
from nautilus_trader.adapters.betfair.common import B_BID_KINDS
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDeltas
from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id
from nautilus_trader.adapters.betfair.parsing.constants import CLOSE_PRICE_LOSER
from nautilus_trader.adapters.betfair.parsing.constants import CLOSE_PRICE_WINNER
from nautilus_trader.adapters.betfair.parsing.constants import MARKET_STATUS_MAPPING
from nautilus_trader.adapters.betfair.parsing.requests import parse_handicap
from nautilus_trader.adapters.betfair.util import hash_market_trade
from nautilus_trader.adapters.betfair.util import one
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClose
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


PARSE_TYPES = Union[
    InstrumentStatusUpdate,
    InstrumentClose,
    OrderBookSnapshot,
    OrderBookDeltas,
    TradeTick,
    BetfairTicker,
    BSPOrderBookDelta,
    BSPOrderBookDeltas,
]


def market_change_to_updates(mc: MarketChange, ts_event: int, ts_init: int) -> list[PARSE_TYPES]:
    updates: list[PARSE_TYPES] = []
    if mc.marketDefinition is not None:
        updates.extend(
            market_definition_to_instrument_status_updates(
                mc.marketDefinition,
                mc.id,
                ts_event,
                ts_init,
            ),
        )
        updates.extend(
            market_definition_to_instrument_closes(mc.marketDefinition, mc.id, ts_event, ts_init),
        )
        updates.extend(
            market_definition_to_instrument_closes(mc.marketDefinition, mc.id, ts_event, ts_init),
        )
    return updates


def market_definition_to_instrument_status_updates(
    market_definition: MarketDefinition,
    market_id: str,
    ts_event: int,
    ts_init: int,
) -> list[InstrumentStatusUpdate]:
    updates = []
    for runner in market_definition.runners:
        instrument_id = betfair_instrument_id(
            market_id=market_id,
            runner_id=str(runner.runner_id),
            runner_handicap=parse_handicap(runner.handicap),
        )
        status = InstrumentStatusUpdate(
            instrument_id=instrument_id,
            status=MARKET_STATUS_MAPPING[(market_definition.status, market_definition.inPlay)],
            ts_event=ts_event,
            ts_init=ts_init,
        )
        updates.append(status)
    return updates


def market_definition_to_instrument_closes(
    market_definition: MarketDefinition,
    market_id: str,
    ts_event: int,
    ts_init: int,
) -> list[InstrumentClose]:
    updates = []
    for runner in market_definition.runners:
        updates.append(runner_to_instrument_close(runner, market_id, ts_event, ts_init))
    return updates


def runner_to_instrument_close(
    runner: Runner,
    market_id: str,
    ts_event: int,
    ts_init: int,
) -> InstrumentClose:
    instrument_id = betfair_instrument_id(
        market_id=market_id,
        runner_id=str(runner.runner_id),
        runner_handicap=parse_handicap(runner.handicap),
    )

    if runner.status in (RunnerStatus.LOSER, RunnerStatus.REMOVED):
        return InstrumentClose(
            instrument_id=instrument_id,
            close_price=CLOSE_PRICE_LOSER,
            close_type=InstrumentCloseType.CONTRACT_EXPIRED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif runner.status in (RunnerStatus.WINNER, RunnerStatus.PLACED):
        return InstrumentClose(
            instrument_id=instrument_id,
            close_price=CLOSE_PRICE_WINNER,
            close_type=InstrumentCloseType.CONTRACT_EXPIRED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        raise ValueError(f"Unknown runner close status: {runner.status}")


def market_definition_to_betfair_starting_prices(
    market_definition: MarketDefinition,
    market_id: str,
    ts_event: int,
    ts_init: int,
) -> list[BetfairStartingPrice]:
    updates: list[BetfairStartingPrice] = []
    for runner in market_definition.runners:
        sp = runner_to_betfair_starting_price(runner, market_id, ts_event, ts_init)
        if sp is not None:
            return updates
    return updates


def runner_to_betfair_starting_price(
    runner: Runner,
    market_id: str,
    ts_event: int,
    ts_init: int,
) -> Optional[BetfairStartingPrice]:
    if runner.bsp is not None:
        instrument_id = betfair_instrument_id(
            market_id=market_id,
            runner_id=str(runner.runner_id),
            runner_handicap=parse_handicap(runner.handicap),
        )
        return BetfairStartingPrice(
            instrument_id=make_bsp_instrument_id(instrument_id),
            bsp=runner.bsp,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        return None


def runner_change_to_order_book_snapshot(
    mc: MarketChange,
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
):
    pass


def _handle_orderbook_snapshot(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> list[OrderBookSnapshot]:
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
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId(trade_id),
            ts_event=ts_event,
            ts_init=ts_init,
        )
        trade_ticks.append(tick)
    return trade_ticks


def _handle_bsp_updates(rc: RunnerChange, instrument_id: InstrumentId, ts_event, ts_init):
    updates = []
    bsp_instrument_id = make_bsp_instrument_id(instrument_id)
    for side, starting_prices in zip(("spb", "spl"), (rc.spb, rc.spl)):
        deltas = []
        for sp in starting_prices:
            delta = BSPOrderBookDelta(
                instrument_id=bsp_instrument_id,
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
            deltas.append(delta)
        batch = BSPOrderBookDeltas(
            instrument_id=bsp_instrument_id,
            book_type=BookType.L2_MBP,
            deltas=deltas,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        updates.append(batch)
    return updates


def _handle_order_book_updates(
    runner: RunnerChange,
    instrument_id: InstrumentId,
    ts_event,
    ts_init,
) -> list[OrderBookData]:
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


def _handle_ticker(runner: RunnerChange, instrument_id: InstrumentId, ts_event, ts_init):
    last_traded_price, traded_volume, starting_price_far, starting_price_near = (
        None,
        None,
        None,
        None,
    )
    if runner.ltp:
        last_traded_price = price_to_probability(str(runner.ltp)).as_double()
    if runner.tv:
        traded_volume = Quantity(value=runner.tv, precision=BETFAIR_QUANTITY_PRECISION).as_double()
    if runner.spn and runner.spn not in ("NaN", "Infinity"):
        starting_price_near = runner.spn
    if runner.spf and runner.spf not in ("NaN", "Infinity"):
        starting_price_far = runner.spf
    return BetfairTicker(
        instrument_id=instrument_id,
        last_traded_price=last_traded_price,
        traded_volume=traded_volume,
        starting_price_far=starting_price_far,
        starting_price_near=starting_price_near,
        ts_init=ts_init,
        ts_event=ts_event,
    )


def _merge_order_book_deltas(all_deltas: list[OrderBookDeltas]):
    cls = type(all_deltas[0])
    per_instrument_deltas = defaultdict(list)
    book_type = one({deltas.book_type for deltas in all_deltas})
    ts_event = one({deltas.ts_event for deltas in all_deltas})
    ts_init = one({deltas.ts_init for deltas in all_deltas})

    for deltas in all_deltas:
        per_instrument_deltas[deltas.instrument_id].extend(deltas.deltas)
    return [
        cls(
            instrument_id=instrument_id,
            deltas=deltas,
            book_type=book_type,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        for instrument_id, deltas in per_instrument_deltas.items()
    ]


#
# def market_change_to_updates(mc: MarketChange, ts_event: int, ts_init: int) -> list[PARSE_TYPES]:
#     updates: list[PARSE_TYPES] = []
#     book_updates: list[OrderBookData] = []
#     bsp_book_updates: list[BSPOrderBookDeltas] = []
#
#     # Emit market change events first
#     updates.extend(_handle_market_runners_status(mc, ts_event, ts_init))
#
#     for rc in mc.rc:
#
#         instrument_id = betfair_instrument_id(
#             market_id=mc.id,
#             runner_id=str(rc.id),
#             runner_handicap=parse_handicap(rc.hc),
#         )
#
#         # OrderBook updates
#         # Delay appending book updates until we can merge at the end
#         if mc.img:
#             # Full snapshot
#             book_updates.extend(
#                 _handle_orderbook_snapshot(
#                     runner=rc,
#                     instrument_id=instrument_id,
#                     ts_event=ts_event,
#                     ts_init=ts_init,
#                 ),
#             )
#         else:
#             book_updates.extend(
#                 _handle_order_book_updates(
#                     runner=rc,
#                     instrument_id=instrument_id,
#                     ts_event=ts_event,
#                     ts_init=ts_init,
#                 ),
#             )
#
#         # TradeTicks
#         if rc.trd:
#             updates.extend(
#                 _handle_market_trades(
#                     rc=rc,
#                     instrument_id=instrument_id,
#                     ts_event=ts_event,
#                     ts_init=ts_init,
#                 ),
#             )
#
#         # Ticker
#         if rc.ltp or rc.tv or rc.spn or rc.spf:
#             updates.append(
#                 _handle_ticker(
#                     runner=rc,
#                     instrument_id=instrument_id,
#                     ts_event=ts_event,
#                     ts_init=ts_init,
#                 ),
#             )
#
#         # BSP Orderbook
#         if rc.spb or rc.spl:
#             bsp_book_updates.extend(
#                 _handle_bsp_updates(
#                     rc=rc,
#                     instrument_id=instrument_id,
#                     ts_event=ts_event,
#                     ts_init=ts_init,
#                 ),
#             )
#
#     return updates


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
            mc_updates = market_change_to_updates(mc, ts_event, ts_init)
            updates.extend(mc_updates)
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
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            ts_event=ts_event,
            ts_init=ts_event,
        ),
    ]


def make_bsp_instrument_id(instrument_id: InstrumentId) -> InstrumentId:
    return InstrumentId(
        symbol=Symbol(instrument_id.symbol.value + "-BSP"),
        venue=instrument_id.venue,
    )
