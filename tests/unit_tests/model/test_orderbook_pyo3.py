# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pytest

from nautilus_trader.core import nautilus_pyo3


@pytest.fixture(name="book")
def fixture_book() -> nautilus_pyo3.OrderBook:
    book_type = nautilus_pyo3.BookType.L2_MBP
    instrument_id = nautilus_pyo3.InstrumentId.from_str("AAPL.XNAS")
    book = nautilus_pyo3.OrderBook(instrument_id, book_type)
    return book


def populate_book(
    book: nautilus_pyo3.OrderBook,
    price_precision: int = 2,
    size_precision: int = 0,
    bids: list[tuple] | None = None,
    asks: list[tuple] | None = None,
) -> None:
    bids_counter: int = 0
    asks_counter: int = 0

    for price, size in bids or []:
        order = nautilus_pyo3.BookOrder(
            side=nautilus_pyo3.OrderSide.BUY,
            price=nautilus_pyo3.Price(price, price_precision),
            size=nautilus_pyo3.Quantity(size, size_precision),
            order_id=bids_counter,
        )
        book.add(order, 0, 0, 0)
        bids_counter += 1
    for price, size in asks or []:
        order = nautilus_pyo3.BookOrder(
            side=nautilus_pyo3.OrderSide.SELL,
            price=nautilus_pyo3.Price(price, price_precision),
            size=nautilus_pyo3.Quantity(size, size_precision),
            order_id=asks_counter,
        )
        book.add(order, 0, 0, 0)
        asks_counter += 1


def test_order_book(book: nautilus_pyo3.OrderBook) -> None:
    populate_book(
        book,
        bids=[
            (100.00, 100),
            (99.00, 200),
        ],
        asks=[
            (101.00, 100),
            (102.00, 200),
        ],
    )

    stub_qty = nautilus_pyo3.Quantity.from_int(150)

    assert book.instrument_id == nautilus_pyo3.InstrumentId.from_str("AAPL.XNAS")
    assert book.book_type == nautilus_pyo3.BookType.L2_MBP
    assert book.count == 4
    assert len(book.bids()) == 2
    assert len(book.asks()) == 2
    assert book.best_bid_price() == 100
    assert book.best_ask_price() == 101
    assert book.best_bid_size() == 100
    assert book.best_ask_size() == 100
    assert book.bids_to_dict() == {Decimal(100): Decimal(100), Decimal(99): Decimal(200)}
    assert book.asks_to_dict() == {Decimal(101): Decimal(100), Decimal(102): Decimal(200)}
    assert book.get_avg_px_for_quantity(stub_qty, nautilus_pyo3.OrderSide.BUY) == 101.33333333333333
    assert book.get_avg_px_for_quantity(stub_qty, nautilus_pyo3.OrderSide.SELL) == 99.66666666666667
