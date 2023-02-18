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
from enum import Enum
from typing import Literal, Optional, Union

import pandas as pd
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming.mcm import MCM
from betfair_parser.spec.streaming.mcm import MarketChange
from betfair_parser.spec.streaming.mcm import MarketDefinition
from betfair_parser.spec.streaming.mcm import Runner
from betfair_parser.spec.streaming.mcm import RunnerChange
from betfair_parser.spec.streaming.mcm import RunnerStatus

from nautilus_trader.adapters.betfair.client.spec import ClearedOrder
from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import BETFAIR_PRICE_PRECISION
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
from nautilus_trader.adapters.betfair.parsing.constants import STRICT_MARKET_DATA_HANDLING
from nautilus_trader.adapters.betfair.parsing.requests import parse_handicap
from nautilus_trader.adapters.betfair.util import hash_market_trade
from nautilus_trader.common.functions import one
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
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.identifiers import AccountId
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


PARSE_TYPES = Union[
    InstrumentStatusUpdate,
    InstrumentClose,
    OrderBookSnapshot,
    OrderBookDeltas,
    TradeTick,
    BetfairTicker,
    BSPOrderBookDelta,
    BSPOrderBookDeltas,
    BetfairStartingPrice,
]


def market_change_to_updates(  # noqa: C901
    mc: MarketChange,
    ts_event: int,
    ts_init: int,
) -> list[PARSE_TYPES]:
    updates: list[PARSE_TYPES] = []

    # Handle instrument status and close updates first
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
            market_definition_to_betfair_starting_prices(
                mc.marketDefinition,
                mc.id,
                ts_event,
                ts_init,
            ),
        )

    # Handle market data updates
    book_updates: list[Union[OrderBookSnapshot, OrderBookDeltas]] = []
    bsp_book_updates: list[Union[BSPOrderBookDeltas]] = []
    for rc in mc.rc:
        instrument_id = betfair_instrument_id(
            market_id=mc.id,
            runner_id=str(rc.id),
            runner_handicap=parse_handicap(rc.hc),
        )

        # Order book data
        if mc.img:
            # Full snapshot, replace order book
            snapshot = runner_change_to_order_book_snapshot(
                rc,
                instrument_id,
                ts_event,
                ts_init,
            )
            if snapshot is not None:
                updates.append(snapshot)
        else:
            # Delta update
            deltas = runner_change_to_order_book_deltas(rc, instrument_id, ts_event, ts_init)
            if deltas is not None:
                book_updates.append(deltas)

        # Trade ticks
        if rc.trd:
            updates.extend(
                runner_change_to_trade_ticks(rc, instrument_id, ts_event, ts_init),
            )

        # BetfairTicker
        if any((rc.ltp, rc.tv, rc.spn, rc.spf)):
            updates.append(
                runner_change_to_betfair_ticker(rc, instrument_id, ts_event, ts_init),
            )

        # BSP order book deltas
        bsp_deltas = runner_change_to_bsp_order_book_deltas(rc, instrument_id, ts_event, ts_init)
        if bsp_deltas is not None:
            bsp_book_updates.append(bsp_deltas)

    # Finally, merge book_updates and bsp_book_updates as they can be split over multiple rc's
    if book_updates and not mc.img:
        updates.extend(_merge_order_book_deltas(book_updates))
    if bsp_book_updates:
        updates.extend(_merge_order_book_deltas(bsp_book_updates))

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
        key: tuple[MarketStatus, bool] = (market_definition.status, market_definition.inPlay)
        if runner.status == RunnerStatus.REMOVED:
            status = MarketStatus.CLOSED
        else:
            try:
                status = MARKET_STATUS_MAPPING[key]
            except KeyError:
                raise ValueError(
                    f"{runner.status=} {market_definition.status=} {market_definition.inPlay=}",
                )
        status = InstrumentStatusUpdate(
            instrument_id=instrument_id,
            status=status,
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
        close = runner_to_instrument_close(runner, market_id, ts_event, ts_init)
        if close is not None:
            updates.append(close)
    return updates


def runner_to_instrument_close(
    runner: Runner,
    market_id: str,
    ts_event: int,
    ts_init: int,
) -> Optional[InstrumentClose]:
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
    elif runner.status == RunnerStatus.ACTIVE:
        return None
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
            updates.append(sp)
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


class MarketDataKind(Enum):
    """MarketDataKind"""

    ALL = "ALL"
    BEST = "BEST"
    DISPLAY = "DISPLAY"


def runner_change_to_market_data_kind(rc: RunnerChange) -> MarketDataKind:
    if rc.atb or rc.atl:
        if STRICT_MARKET_DATA_HANDLING:
            assert not any((rc.batb, rc.batl, rc.bdatb, rc.bdatl)), "Mixed market data kinds"
        return MarketDataKind.ALL
    elif rc.batl or rc.batb:
        if STRICT_MARKET_DATA_HANDLING:
            assert not any((rc.atb, rc.atl, rc.bdatb, rc.bdatl)), "Mixed market data kinds"
        return MarketDataKind.BEST
    elif rc.bdatb or rc.bdatl:
        if STRICT_MARKET_DATA_HANDLING:
            assert not any((rc.atb, rc.atl, rc.batb, rc.batl)), "Mixed market data kinds"
        return MarketDataKind.DISPLAY
    else:
        raise ValueError("rc contains no valid market data")


def runner_change_to_order_book_snapshot(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> Optional[OrderBookSnapshot]:
    try:
        market_data_kind = runner_change_to_market_data_kind(rc)
    except ValueError:
        return None
    if market_data_kind == MarketDataKind.ALL:
        return runner_change_all_depth_to_order_book_snapshot(rc, instrument_id, ts_event, ts_init)
    elif market_data_kind == MarketDataKind.BEST:
        return runner_change_best_depth_to_order_book_snapshot(rc, instrument_id, ts_event, ts_init)
    elif market_data_kind == MarketDataKind.DISPLAY:
        return runner_change_display_depth_to_order_book_snapshot(
            rc,
            instrument_id,
            ts_event,
            ts_init,
        )
    else:
        raise ValueError("Unknown market data kind")


def runner_change_all_depth_to_order_book_snapshot(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> Optional[OrderBookSnapshot]:
    # Bids are available to lay (atl)
    if rc.atl:
        bids = [
            (price_to_probability(str(order.price)), order.volume)
            for order in rc.atl
            if order.price
        ]
    else:
        bids = []

    # Asks are available to back (atb)
    if rc.atb:
        asks: list = [
            (price_to_probability(str(order.price)), order.volume)
            for order in rc.atb
            if order.price
        ]
    else:
        asks = []

    return OrderBookSnapshot(
        book_type=BookType.L2_MBP,
        instrument_id=instrument_id,
        bids=bids,
        asks=asks,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def runner_change_best_depth_to_order_book_snapshot(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> Optional[OrderBookSnapshot]:
    # Bids are best available to lay (batl)
    if rc.batl:
        bids = [
            (price_to_probability(str(order.price)), order.volume)
            for order in rc.batl
            if order.price
        ]
    else:
        bids = []

    # Asks are best available to back (batb)
    if rc.batb:
        asks: list = [
            (price_to_probability(str(order.price)), order.volume)
            for order in rc.batb
            if order.price
        ]
    else:
        asks = []

    return OrderBookSnapshot(
        book_type=BookType.L2_MBP,
        instrument_id=instrument_id,
        bids=bids,
        asks=asks,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def runner_change_display_depth_to_order_book_snapshot(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> Optional[OrderBookSnapshot]:
    # Bids are best display available to lay (bdatl)
    if rc.bdatl:
        bids = [
            (price_to_probability(str(order.price)), order.volume)
            for order in rc.bdatl
            if order.price
        ]
    else:
        bids = []

    # Asks are best display available to back (bdatb)
    if rc.bdatb:
        asks: list = [
            (price_to_probability(str(order.price)), order.volume)
            for order in rc.bdatb
            if order.price
        ]
    else:
        asks = []

    return OrderBookSnapshot(
        book_type=BookType.L2_MBP,
        instrument_id=instrument_id,
        bids=bids,
        asks=asks,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def runner_change_to_order_book_deltas(
    runner: RunnerChange,
    instrument_id: InstrumentId,
    ts_event,
    ts_init,
) -> Optional[OrderBookDeltas]:
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
        return ob_update
    else:
        return None


def runner_change_to_trade_ticks(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> list[TradeTick]:
    trade_ticks: list[TradeTick] = []
    for trd in rc.trd:
        if trd.volume == 0:
            continue
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


def runner_change_to_betfair_ticker(
    runner: RunnerChange,
    instrument_id: InstrumentId,
    ts_event,
    ts_init,
) -> BetfairTicker:
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


def _create_bsp_order_book_delta(
    bsp_instrument_id: InstrumentId,
    side: Literal["spb", "spl"],
    price: float,
    volume: float,
    ts_event: int,
    ts_init: int,
) -> BSPOrderBookDelta:
    return BSPOrderBookDelta(
        instrument_id=bsp_instrument_id,
        book_type=BookType.L2_MBP,
        action=BookAction.DELETE if volume == 0 else BookAction.UPDATE,
        order=BookOrder(
            price=price_to_probability(str(price)),
            size=Quantity(volume, precision=BETFAIR_QUANTITY_PRECISION),
            side=B2N_MARKET_STREAM_SIDE[side],
        ),
        ts_event=ts_event,
        ts_init=ts_init,
    )


def runner_change_to_bsp_order_book_deltas(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> Optional[BSPOrderBookDeltas]:
    if not (rc.spb or rc.spl):
        return None
    bsp_instrument_id = make_bsp_instrument_id(instrument_id)
    deltas: list[BSPOrderBookDelta] = []
    for price, volume in rc.spb:
        deltas.append(
            _create_bsp_order_book_delta(
                bsp_instrument_id,
                "spb",
                price,
                volume,
                ts_event,
                ts_init,
            ),
        )
    for price, volume in rc.spl:
        deltas.append(
            _create_bsp_order_book_delta(
                bsp_instrument_id,
                "spl",
                price,
                volume,
                ts_event,
                ts_init,
            ),
        )

    return BSPOrderBookDeltas(
        instrument_id=bsp_instrument_id,
        book_type=BookType.L2_MBP,
        deltas=deltas,
        ts_event=ts_event,
        ts_init=ts_init,
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
    filled: list[ClearedOrder] = self.client().betting.list_cleared_orders(bet_ids=[venue_order_id])
    if not filled:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled[0]
    ts_event = int(pd.Timestamp(fill.lastMatchedDate).to_datetime64())
    return [
        TradeReport(
            account_id=AccountId("BETFAIR"),
            instrument_id=betfair_instrument_id(
                fill.marketId,
                str(fill.selectionId),
                str(fill.handicap),
            ),
            venue_order_id=VenueOrderId(fill.betId),
            venue_position_id=None,  # Can be None
            trade_id=TradeId(fill.lastMatchedDate),
            last_qty=Quantity(fill.sizeSettled, BETFAIR_QUANTITY_PRECISION),
            last_px=Price(fill.priceMatched, BETFAIR_PRICE_PRECISION),
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
