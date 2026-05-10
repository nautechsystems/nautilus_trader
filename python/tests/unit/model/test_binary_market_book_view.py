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

import pytest

from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import OrderType
from nautilus_trader.model import OwnBookOrder
from nautilus_trader.model import OwnOrderBook
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId


def _populate_book(book, bids=None, asks=None):
    for i, (price, size) in enumerate(bids or []):
        order = BookOrder(OrderSide.BUY, Price(price, 2), Quantity(size, 0), i)
        book.add(order, 0, 0, 0)
    for i, (price, size) in enumerate(asks or [], start=len(bids or [])):
        order = BookOrder(OrderSide.SELL, Price(price, 2), Quantity(size, 0), i)
        book.add(order, 0, 0, 0)


def _make_own_order(client_order_id, side, price, size):
    return OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId(client_order_id),
        side=side,
        price=Price(price, 2),
        size=Quantity(size, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )


def test_binary_market_filtered_view_with_opposite_own_book():
    yes_id = InstrumentId.from_str("YES.XNAS")
    no_id = InstrumentId.from_str("NO.XNAS")

    book = OrderBook(yes_id, BookType.L2_MBP)
    _populate_book(
        book,
        bids=[(0.40, 100), (0.39, 100)],
        asks=[(0.60, 100), (0.61, 200)],
    )

    own_book = OwnOrderBook(yes_id)
    own_book.add(_make_own_order("O-123", OrderSide.BUY, 0.40, 20))

    own_synthetic_book = OwnOrderBook(no_id)
    own_synthetic_book.add(_make_own_order("O-1", OrderSide.BUY, 0.40, 100))

    combined_own = own_book.combined_with_opposite(own_synthetic_book)
    filtered = book.filtered_view(combined_own)

    assert filtered.best_bid_size() == 80
    assert filtered.best_ask_size() == 200
    assert filtered.best_ask_price() == 0.61


def test_filtered_view_instrument_mismatch_raises():
    yes_id = InstrumentId.from_str("YES.XNAS")
    no_id = InstrumentId.from_str("NO.XNAS")

    book = OrderBook(yes_id, BookType.L2_MBP)
    own_book = OwnOrderBook(no_id)

    with pytest.raises(
        ValueError,
        match=r"Instrument ID mismatch: book=YES.XNAS, own_book=NO.XNAS",
    ):
        book.filtered_view(own_book)


def test_combined_with_opposite_same_instrument_raises():
    yes_id = InstrumentId.from_str("YES.XNAS")

    own_book = OwnOrderBook(yes_id)
    own_synthetic_book = OwnOrderBook(yes_id)

    with pytest.raises(
        ValueError,
        match=r"Opposite own book must have different instrument ID",
    ):
        own_book.combined_with_opposite(own_synthetic_book)


def test_filtered_view_without_own_book():
    yes_id = InstrumentId.from_str("YES.XNAS")

    book = OrderBook(yes_id, BookType.L2_MBP)
    _populate_book(
        book,
        bids=[(0.40, 100)],
        asks=[(0.60, 200)],
    )

    filtered = book.filtered_view()

    assert filtered.best_bid_size() == 100
    assert filtered.best_ask_size() == 200
