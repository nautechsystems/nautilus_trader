# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import uuid

import msgspec

from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
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
from nautilus_trader.model.identifiers import TradeId
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
    ) -> OrderBookDeltas:
        ts_event = millis_to_nanos(float(self.timestamp))

        deltas: list[OrderBookDelta] = []

        # Add initial clear
        clear = OrderBookDelta.clear(
            instrument_id=instrument.id,
            sequence=0,  # N/A
            ts_event=ts_event,
            ts_init=ts_init,
        )
        deltas.append(clear)

        bids_len = len(self.bids)
        asks_len = len(self.asks)

        for idx, bid in enumerate(self.bids):
            flags = 0
            if idx == bids_len - 1 and asks_len == 0:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

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
            flags = 0
            if idx == asks_len - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

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

    def parse_to_quote_tick(
        self,
        instrument: BinaryOption,
        ts_init: int,
    ) -> QuoteTick:
        top_bid = self.bids[-1]
        top_ask = self.asks[-1]
        return QuoteTick(
            instrument_id=instrument.id,
            bid_price=instrument.make_price(float(top_bid.price)),
            ask_price=instrument.make_price(float(top_ask.price)),
            bid_size=instrument.make_qty(float(top_bid.size)),
            ask_size=instrument.make_qty(float(top_ask.size)),
            ts_event=ts_init,  # Polymarket does not provide a timestamp
            ts_init=ts_init,
        )


class PolymarketQuote(msgspec.Struct, frozen=True):
    price: str
    side: PolymarketOrderSide
    size: str


class PolymarketQuotes(msgspec.Struct, tag="price_change", tag_field="event_type", frozen=True):
    market: str
    asset_id: str
    changes: list[PolymarketQuote]
    timestamp: str

    def parse_to_deltas(
        self,
        instrument: BinaryOption,
        ts_init: int,
    ) -> OrderBookDeltas:
        deltas: list[OrderBookDelta] = []
        for change in self.changes:
            order = BookOrder(
                side=OrderSide.BUY if change.side == PolymarketOrderSide.BUY else OrderSide.SELL,
                price=instrument.make_price(float(change.price)),
                size=instrument.make_qty(float(change.size)),
                order_id=0,  # N/A for L2 books
            )
            delta = OrderBookDelta(
                instrument_id=instrument.id,
                action=BookAction.UPDATE if order.size > 0 else BookAction.DELETE,
                order=order,
                flags=RecordFlag.F_LAST,
                sequence=0,  # N/A
                ts_event=millis_to_nanos(float(self.timestamp)),
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
        quotes: list[QuoteTick] = []
        for change in self.changes:
            if change.side == PolymarketOrderSide.BUY:
                ask_price = last_quote.ask_price
                ask_size = last_quote.ask_size
                bid_price = instrument.make_price(float(change.price))
                bid_size = instrument.make_qty(float(change.size))
            else:  # SELL
                ask_price = instrument.make_price(float(change.price))
                ask_size = instrument.make_qty(float(change.size))
                bid_price = last_quote.bid_price
                bid_size = last_quote.bid_size
            quote = QuoteTick(
                instrument_id=instrument.id,
                bid_price=bid_price,
                ask_price=ask_price,
                bid_size=bid_size,
                ask_size=ask_size,
                ts_event=millis_to_nanos(float(self.timestamp)),
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
        return TradeTick(
            instrument_id=instrument.id,
            price=instrument.make_price(float(self.price)),
            size=instrument.make_qty(float(self.size)),
            aggressor_side=aggressor_side,
            trade_id=TradeId(str(uuid.uuid4())),
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
