# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MIN_PRICE
from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.parsing import determine_trade_id
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.instruments import BinaryOption


class PolymarketBookLevel(msgspec.Struct, frozen=True):
    price: str
    size: str


class PolymarketBookSnapshot(msgspec.Struct, tag="book", tag_field="event_type", frozen=True):
    market: str
    asset_id: str
    bids: list[PolymarketBookLevel]
    asks: list[PolymarketBookLevel]
    timestamp: str

    def parse_to_snapshot(
        self,
        instrument: BinaryOption,
        ts_init: int,
    ) -> OrderBookDeltas | None:
        ts_event = millis_to_nanos(float(self.timestamp))

        bids_len = len(self.bids)
        asks_len = len(self.asks)

        # Skip if both bids and asks are empty (can occur near market resolution)
        if bids_len == 0 and asks_len == 0:
            return None

        # Downstream consumers (data engine, wranglers) rely on F_SNAPSHOT to
        # distinguish the opening CLEAR + ADDs of a snapshot rebuild from an
        # incremental book reset. Flag every snapshot delta; the last also
        # gets F_LAST to close the batch.
        deltas: list[OrderBookDelta] = []

        clear = OrderBookDelta(
            instrument_id=instrument.id,
            action=BookAction.CLEAR,
            order=None,
            flags=RecordFlag.F_SNAPSHOT,
            sequence=0,  # N/A
            ts_event=ts_event,
            ts_init=ts_init,
        )
        deltas.append(clear)

        for idx, bid in enumerate(self.bids):
            flags = RecordFlag.F_SNAPSHOT
            if idx == bids_len - 1 and asks_len == 0:
                flags |= RecordFlag.F_LAST

            order = BookOrder(
                side=OrderSide.BUY,
                price=instrument.make_price(float(bid.price)),
                size=instrument.make_qty(float(bid.size)),
                order_id=0,  # N/A
            )
            delta = OrderBookDelta(
                instrument_id=instrument.id,
                action=BookAction.ADD,
                order=order,
                flags=flags,
                sequence=0,  # N/A
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        for idx, ask in enumerate(self.asks):
            flags = RecordFlag.F_SNAPSHOT
            if idx == asks_len - 1:
                flags |= RecordFlag.F_LAST

            order = BookOrder(
                side=OrderSide.SELL,
                price=instrument.make_price(float(ask.price)),
                size=instrument.make_qty(float(ask.size)),
                order_id=0,  # N/A
            )
            delta = OrderBookDelta(
                instrument_id=instrument.id,
                action=BookAction.ADD,
                order=order,
                flags=flags,
                sequence=0,  # N/A
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument.id, deltas=deltas)

    def parse_to_quote(
        self,
        instrument: BinaryOption,
        ts_init: int,
        drop_quotes_missing_side: bool = True,
    ) -> QuoteTick | None:
        # Handle missing bid/ask prices (can occur near market resolution)
        if not self.bids or not self.asks:
            if drop_quotes_missing_side:
                return None

            # Use boundary prices with zero volume for missing sides
            # POLYMARKET_MIN_PRICE = 0.001, POLYMARKET_MAX_PRICE = 0.999
            if self.bids:
                top_bid = self.bids[-1]
                top_bid_price = float(top_bid.price)
                top_bid_size = float(top_bid.size)
            else:
                top_bid_price = POLYMARKET_MIN_PRICE
                top_bid_size = 0.0

            if self.asks:
                top_ask = self.asks[-1]
                top_ask_price = float(top_ask.price)
                top_ask_size = float(top_ask.size)
            else:
                top_ask_price = POLYMARKET_MAX_PRICE
                top_ask_size = 0.0
        else:
            top_bid = self.bids[-1]
            top_bid_price = float(top_bid.price)
            top_bid_size = float(top_bid.size)

            top_ask = self.asks[-1]
            top_ask_price = float(top_ask.price)
            top_ask_size = float(top_ask.size)

        return QuoteTick(
            instrument_id=instrument.id,
            bid_price=instrument.make_price(top_bid_price),
            ask_price=instrument.make_price(top_ask_price),
            bid_size=instrument.make_qty(top_bid_size),
            ask_size=instrument.make_qty(top_ask_size),
            ts_event=millis_to_nanos(float(self.timestamp)),
            ts_init=ts_init,
        )


class PolymarketQuote(msgspec.Struct, frozen=True):
    asset_id: str
    price: str
    side: PolymarketOrderSide
    size: str
    hash: str
    best_bid: str | None = None
    best_ask: str | None = None


class PolymarketQuotes(msgspec.Struct, tag="price_change", tag_field="event_type", frozen=True):
    market: str
    price_changes: list[PolymarketQuote]
    timestamp: str

    def parse_to_deltas(
        self,
        instrument: BinaryOption,
        ts_init: int,
    ) -> OrderBookDeltas:
        ts_event = millis_to_nanos(float(self.timestamp))
        count = len(self.price_changes)
        deltas: list[OrderBookDelta] = []

        for idx, change in enumerate(self.price_changes):
            order = BookOrder(
                side=OrderSide.BUY if change.side == PolymarketOrderSide.BUY else OrderSide.SELL,
                price=instrument.make_price(float(change.price)),
                size=instrument.make_qty(float(change.size)),
                order_id=0,  # N/A for L2 books
            )
            flags = RecordFlag.F_LAST if idx == count - 1 else 0
            delta = OrderBookDelta(
                instrument_id=instrument.id,
                action=BookAction.UPDATE if order.size > 0 else BookAction.DELETE,
                order=order,
                flags=flags,
                sequence=0,  # N/A
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument.id, deltas)

    def parse_to_quote_ticks(
        self,
        instrument: BinaryOption,
        last_quote: QuoteTick,
        ts_init: int,
    ) -> list[QuoteTick]:
        # `change.price` is the level that changed, not the new top. The payload
        # carries the authoritative post-change top in `best_bid`/`best_ask` and
        # only reports size at the changed level, so carry the top size from
        # `last_quote` unless the changed level is itself the new top.
        ts_event = millis_to_nanos(float(self.timestamp))
        quotes: list[QuoteTick] = []

        for change in self.price_changes:
            if change.best_bid is None or change.best_ask is None:
                continue
            try:
                bid_price_f = float(change.best_bid)
                ask_price_f = float(change.best_ask)
            except ValueError:
                continue

            if bid_price_f <= 0.0 or ask_price_f <= 0.0 or bid_price_f >= ask_price_f:
                continue

            bid_price = instrument.make_price(bid_price_f)
            ask_price = instrument.make_price(ask_price_f)
            changed_price = instrument.make_price(float(change.price))
            changed_size = instrument.make_qty(float(change.size))

            if change.side == PolymarketOrderSide.BUY:
                bid_size = changed_size if changed_price == bid_price else last_quote.bid_size
                ask_size = last_quote.ask_size
            else:
                ask_size = changed_size if changed_price == ask_price else last_quote.ask_size
                bid_size = last_quote.bid_size

            quote = QuoteTick(
                instrument_id=instrument.id,
                bid_price=bid_price,
                ask_price=ask_price,
                bid_size=bid_size,
                ask_size=ask_size,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            quotes.append(quote)
            last_quote = quote

        return quotes


class PolymarketTrade(msgspec.Struct, tag="last_trade_price", tag_field="event_type", frozen=True):
    market: str
    asset_id: str
    fee_rate_bps: str
    price: str
    side: PolymarketOrderSide
    size: str
    timestamp: str

    def parse_to_trade_tick(
        self,
        instrument: BinaryOption,
        ts_init: int,
    ) -> TradeTick:
        aggressor_side = (
            AggressorSide.BUYER if self.side == PolymarketOrderSide.BUY else AggressorSide.SELLER
        )
        trade_id = determine_trade_id(
            asset_id=self.asset_id,
            side=self.side,
            price=self.price,
            size=self.size,
            timestamp=self.timestamp,
        )

        return TradeTick(
            instrument_id=instrument.id,
            price=instrument.make_price(float(self.price)),
            size=instrument.make_qty(float(self.size)),
            aggressor_side=aggressor_side,
            trade_id=trade_id,
            ts_event=millis_to_nanos(float(self.timestamp)),
            ts_init=ts_init,
        )


class PolymarketTickSizeChange(
    msgspec.Struct,
    tag="tick_size_change",
    tag_field="event_type",
    frozen=True,
):
    market: str
    asset_id: str
    new_tick_size: str
    old_tick_size: str
    timestamp: str
