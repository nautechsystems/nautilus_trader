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

from nautilus_trader.model.book import BookLevel
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Price


def compute_effective_deltas(
    book_old: OrderBook,
    book_new: OrderBook,
    instrument: BinaryOption,
) -> OrderBookDeltas | None:
    """
    Compare the old and new order book states and generate a list of effective deltas.

    Parameters
    ----------
    book_old : OrderBook
        The previous state of the order book.
    book_new : OrderBook
        The new state of the order book after applying deltas.
    instrument : BinaryOption
        The instrument associated with the order book.

    Returns
    -------
    OrderBookDeltas or ``None``
        A collection of deltas representing the changes between the old and new book states.
        If no change between book states, then `None` is returned.

    """
    deltas: list[OrderBookDelta] = []
    instrument_id = instrument.id
    assert instrument_id == book_old.instrument_id
    assert instrument_id == book_new.instrument_id
    ts_event = book_new.ts_event
    ts_init = book_new.ts_init

    old_bids: dict[Price, BookLevel] = {level.price: level for level in book_old.bids()}
    old_asks: dict[Price, BookLevel] = {level.price: level for level in book_old.asks()}

    new_bids = book_new.bids()
    for bid in new_bids:
        price = bid.price
        size = instrument.make_qty(bid.size())

        if bid.price not in old_bids:
            # New bid (ADD)
            order = BookOrder(
                side=OrderSide.BUY,
                price=price,
                size=size,
                order_id=0,  # Not applicable for L2 data
            )
            deltas.append(
                OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.ADD,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )
        elif instrument.make_qty(old_bids[bid.price].size()) != size:
            # Updated bid (UPDATE)
            order = BookOrder(
                side=OrderSide.BUY,
                price=price,
                size=size,
                order_id=0,  # Not applicable for L2 data
            )
            deltas.append(
                OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.UPDATE,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )

        old_bids.pop(bid.price, None)

    new_asks = book_new.asks()
    for ask in new_asks:
        price = ask.price
        size = instrument.make_qty(ask.size())

        if ask.price not in old_asks:
            # New ask (ADD)
            order = BookOrder(
                side=OrderSide.SELL,
                price=price,
                size=size,
                order_id=0,  # Not applicable for L2 data
            )
            deltas.append(
                OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.ADD,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )
        elif instrument.make_qty(old_asks[ask.price].size()) != size:
            # Updated ask (UPDATE)
            order = BookOrder(
                side=OrderSide.SELL,
                price=price,
                size=size,
                order_id=0,  # Not applicable for L2 data
            )
            deltas.append(
                OrderBookDelta(
                    instrument_id=instrument_id,
                    action=BookAction.UPDATE,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                ),
            )
        old_asks.pop(ask.price, None)

    # Process remaining old bids as removals
    for old_price, old_level in old_bids.items():
        order = BookOrder(
            side=OrderSide.BUY,
            price=old_price,
            size=instrument.make_qty(old_level.size()),
            order_id=0,  # Not applicable for L2 data
        )
        deltas.append(
            OrderBookDelta(
                instrument_id=instrument_id,
                action=BookAction.DELETE,
                order=order,
                flags=0,
                sequence=0,
                ts_event=ts_event,
                ts_init=ts_init,
            ),
        )

    # Process remaining old asks as removals
    for old_price, old_level in old_asks.items():
        order = BookOrder(
            side=OrderSide.SELL,
            price=old_price,
            size=instrument.make_qty(old_level.size()),
            order_id=0,  # Not applicable for L2 data
        )
        deltas.append(
            OrderBookDelta(
                instrument_id=instrument_id,
                action=BookAction.DELETE,
                order=order,
                flags=0,
                sequence=0,
                ts_event=ts_event,
                ts_init=ts_init,
            ),
        )

    # Return None if there are no deltas
    if not deltas:
        return None

    # Mark the last delta in the batch with the F_LAST flag
    last_delta = deltas[-1]
    last_delta = OrderBookDelta(
        instrument_id=last_delta.instrument_id,
        action=last_delta.action,
        order=last_delta.order,
        flags=RecordFlag.F_LAST,
        sequence=last_delta.sequence,
        ts_event=last_delta.ts_event,
        ts_init=last_delta.ts_init,
    )

    deltas[-1] = last_delta

    return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)
