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

from collections import defaultdict
from datetime import datetime
from typing import Optional, Union

import pandas as pd
from betfair_parser.spec.betting.type_definitions import ClearedOrderSummary
from betfair_parser.spec.streaming.mcm import MarketChange
from betfair_parser.spec.streaming.mcm import MarketDefinition
from betfair_parser.spec.streaming.mcm import Runner
from betfair_parser.spec.streaming.mcm import RunnerChange
from betfair_parser.spec.streaming.mcm import RunnerStatus
from betfair_parser.spec.streaming.mcm import _PriceVolume

from nautilus_trader.adapters.betfair.constants import CLOSE_PRICE_LOSER
from nautilus_trader.adapters.betfair.constants import CLOSE_PRICE_WINNER
from nautilus_trader.adapters.betfair.constants import MARKET_STATUS_MAPPING
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDeltas
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id
from nautilus_trader.adapters.betfair.parsing.common import hash_market_trade
from nautilus_trader.adapters.betfair.parsing.requests import parse_handicap
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.data.book import NULL_ORDER
from nautilus_trader.model.data.book import BookOrder
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClose
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price


PARSE_TYPES = Union[
    InstrumentStatusUpdate,
    InstrumentClose,
    OrderBookDeltas,
    TradeTick,
    BetfairTicker,
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
    if mc.market_definition is not None:
        updates.extend(
            market_definition_to_instrument_status_updates(
                mc.market_definition,
                mc.id,
                ts_event,
                ts_init,
            ),
        )
        updates.extend(
            market_definition_to_instrument_closes(mc.market_definition, mc.id, ts_event, ts_init),
        )
        updates.extend(
            market_definition_to_betfair_starting_prices(
                mc.market_definition,
                mc.id,
                ts_event,
                ts_init,
            ),
        )

    # Handle market data updates
    book_updates: list[OrderBookDeltas] = []
    bsp_book_updates: list[BSPOrderBookDeltas] = []
    for rc in mc.rc:
        instrument_id = betfair_instrument_id(
            market_id=mc.id,
            selection_id=str(rc.id),
            selection_handicap=parse_handicap(rc.hc),
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
            selection_id=str(runner.runner_id),
            selection_handicap=parse_handicap(runner.handicap),
        )
        key: tuple[MarketStatus, bool] = (market_definition.status, market_definition.in_play)
        if runner.status == RunnerStatus.REMOVED:
            status = MarketStatus.CLOSED
        else:
            try:
                status = MARKET_STATUS_MAPPING[key]
            except KeyError:
                raise ValueError(
                    f"{runner.status=} {market_definition.status=} {market_definition.in_play=}",
                )
        status = InstrumentStatusUpdate(
            instrument_id,
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
    instrument_id: InstrumentId = betfair_instrument_id(
        market_id=market_id,
        selection_id=str(runner.runner_id),
        selection_handicap=parse_handicap(runner.handicap),
    )

    if runner.status in (RunnerStatus.LOSER, RunnerStatus.REMOVED):
        return InstrumentClose(
            instrument_id,
            close_price=CLOSE_PRICE_LOSER,
            close_type=InstrumentCloseType.CONTRACT_EXPIRED,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    elif runner.status in (RunnerStatus.WINNER, RunnerStatus.PLACED):
        return InstrumentClose(
            instrument_id,
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
            selection_id=str(runner.runner_id),
            selection_handicap=parse_handicap(runner.handicap),
        )
        return BetfairStartingPrice(
            instrument_id=make_bsp_instrument_id(instrument_id),
            bsp=runner.bsp,
            ts_event=ts_event,
            ts_init=ts_init,
        )
    else:
        return None


def _price_volume_to_book_order(pv: _PriceVolume, side: OrderSide) -> BookOrder:
    price = betfair_float_to_price(pv.price)
    order_id = int(price.as_double() * 10**price.precision)
    return BookOrder(
        side,
        price,
        betfair_float_to_quantity(pv.volume),
        order_id,
    )


def price_to_order_id(price: Price) -> int:
    return int(price.as_double() * 10**price.precision)


def runner_change_to_order_book_snapshot(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> OrderBookDeltas:
    """
    Convert a RunnerChange to a OrderBookDeltas snapshot.
    """
    # Check for incorrect data types
    assert not (
        rc.bdatb or rc.bdatl
    ), "Incorrect orderbook data found (best display), should only be `atb` and `atl`"
    assert not (
        rc.batb or rc.batl
    ), "Incorrect orderbook data found (best) should only be `atb` and `atl`"

    deltas: list[OrderBookDelta] = [
        OrderBookDelta(
            instrument_id,
            BookAction.CLEAR,
            NULL_ORDER,
            ts_event,
            ts_init,
        ),
    ]

    # Bids are available to back (atb)
    for bid in rc.atb:
        book_order = _price_volume_to_book_order(bid, OrderSide.BUY)
        delta = OrderBookDelta(
            instrument_id,
            BookAction.UPDATE if bid.volume > 0.0 else BookAction.DELETE,
            book_order,
            ts_event,
            ts_init,
        )
        deltas.append(delta)

    # Asks are available to back (atl)
    for ask in rc.atl:
        book_order = _price_volume_to_book_order(ask, OrderSide.SELL)
        delta = OrderBookDelta(
            instrument_id,
            BookAction.UPDATE if ask.volume > 0.0 else BookAction.DELETE,
            book_order,
            ts_event,
            ts_init,
        )
        deltas.append(delta)

    return OrderBookDeltas(instrument_id, deltas)


def runner_change_to_order_book_deltas(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> Optional[OrderBookDeltas]:
    """
    Convert a RunnerChange to a list of OrderBookDeltas.
    """
    assert not (
        rc.bdatb or rc.bdatl
    ), "Incorrect orderbook data found (best display), should only be `atb` and `atl`"
    assert not (
        rc.batb or rc.batl
    ), "Incorrect orderbook data found (best) should only be `atb` and `atl`"

    deltas: list[OrderBookDelta] = []

    # Bids are available to back (atb)
    for bid in rc.atb:
        book_order = _price_volume_to_book_order(bid, OrderSide.BUY)
        delta = OrderBookDelta(
            instrument_id,
            BookAction.UPDATE if bid.volume > 0.0 else BookAction.DELETE,
            book_order,
            ts_event,
            ts_init,
        )
        deltas.append(delta)

    # Asks are available to back (atl)
    for ask in rc.atl:
        book_order = _price_volume_to_book_order(ask, OrderSide.SELL)

        delta = OrderBookDelta(
            instrument_id,
            BookAction.UPDATE if ask.volume > 0.0 else BookAction.DELETE,
            book_order,
            ts_event,
            ts_init,
        )
        deltas.append(delta)

    if not deltas:
        return None

    return OrderBookDeltas(instrument_id, deltas)


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
            instrument_id,
            betfair_float_to_price(trd.price),
            betfair_float_to_quantity(trd.volume),
            AggressorSide.NO_AGGRESSOR,
            TradeId(trade_id),
            ts_event,
            ts_init,
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
        last_traded_price = runner.ltp
    if runner.tv:
        traded_volume = runner.tv
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


def runner_change_to_bsp_order_book_deltas(
    rc: RunnerChange,
    instrument_id: InstrumentId,
    ts_event: int,
    ts_init: int,
) -> Optional[BSPOrderBookDeltas]:
    if not (rc.spb or rc.spl):
        return None
    bsp_instrument_id = make_bsp_instrument_id(instrument_id)
    deltas: list[OrderBookDelta] = []

    for spb in rc.spb:
        book_order = _price_volume_to_book_order(spb, OrderSide.SELL)
        delta = OrderBookDelta(
            bsp_instrument_id,
            BookAction.DELETE if spb.volume == 0.0 else BookAction.UPDATE,
            book_order,
            ts_event,
            ts_init,
        )
        deltas.append(delta)

    for spl in rc.spl:
        book_order = _price_volume_to_book_order(spb, OrderSide.BUY)
        delta = OrderBookDelta(
            bsp_instrument_id,
            BookAction.DELETE if spl.volume == 0.0 else BookAction.UPDATE,
            book_order,
            ts_event,
            ts_init,
        )
        deltas.append(delta)

    return BSPOrderBookDeltas(bsp_instrument_id, deltas)


def _merge_order_book_deltas(all_deltas: list[OrderBookDeltas]):
    cls = type(all_deltas[0])
    per_instrument_deltas = defaultdict(list)

    for deltas in all_deltas:
        per_instrument_deltas[deltas.instrument_id].extend(deltas.deltas)
    return [
        cls(
            instrument_id=instrument_id,
            deltas=deltas,
        )
        for instrument_id, deltas in per_instrument_deltas.items()
    ]


async def generate_trades_list(
    self,
    venue_order_id: VenueOrderId,
    symbol: Symbol,
    since: Optional[datetime] = None,
) -> list[TradeReport]:
    filled: list[ClearedOrderSummary] = self.client().betting.list_cleared_orders(
        bet_ids=[venue_order_id],
    )
    if not filled:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled[0]
    ts_event = pd.Timestamp(fill.lastMatchedDate).value
    return [
        TradeReport(
            account_id=AccountId("BETFAIR"),
            instrument_id=betfair_instrument_id(
                fill.marketId,
                str(fill.selectionId),
                str(fill.handicap),
            ),
            order_side=OrderSide.NO_ORDER_SIDE,  # TODO: Needs this
            venue_order_id=VenueOrderId(fill.betId),
            venue_position_id=None,  # Can be None
            trade_id=TradeId(fill.lastMatchedDate),
            last_qty=betfair_float_to_quantity(fill.sizeSettled),
            last_px=betfair_float_to_price(fill.priceMatched),
            commission=None,  # Can be None
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            report_id=UUID4(),
            ts_event=ts_event,
            ts_init=ts_event,
        ),
    ]


def make_bsp_instrument_id(instrument_id: InstrumentId) -> InstrumentId:
    return InstrumentId(
        symbol=Symbol(instrument_id.symbol.value + "-BSP"),
        venue=instrument_id.venue,
    )
