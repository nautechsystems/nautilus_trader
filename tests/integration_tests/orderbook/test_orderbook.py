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

from nautilus_trader.model.objects import Price
from nautilus_trader.model.orderbook.book import L1OrderBook
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.book import L3OrderBook
from nautilus_trader.model.orderbook.error import BookIntegrityError
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def test_l3_feed():
    book = L3OrderBook(
        instrument_id=TestIdStubs.audusd_id(),
        price_precision=5,
        size_precision=0,
    )
    # Updates that cause the book to fail integrity checks will be deleted
    # immediately, however we may also delete later.
    skip_deletes = []
    i = 0
    for i, m in enumerate(TestDataStubs.l3_feed()):  # noqa (B007)
        if m["op"] == "update":
            book.update(order=m["order"])
            try:
                book.check_integrity()
            except BookIntegrityError:
                book.delete(order=m["order"])
                skip_deletes.append(m["order"].id)
        elif m["op"] == "delete" and m["order"].id not in skip_deletes:
            book.delete(order=m["order"])
        book.check_integrity()
    assert i == 100_047
    assert book.best_ask_level().price == 61405.27923706
    assert book.best_ask_level().volume() == 0.12227
    assert book.best_bid_level().price == Price.from_int(61391)
    assert book.best_bid_level().volume() == 1


def test_l2_feed():
    book = L2OrderBook(
        instrument_id=TestIdStubs.audusd_id(),
        price_precision=5,
        size_precision=0,
    )

    # Duplicate delete messages
    skip = [
        (12152, "378a3caf-0262-4d8b-95b6-8df65312b9f3"),
        (28646, "8101452c-8a80-4ca9-b0d9-c472691cec28"),
        (68431, "8913f4bf-cc49-4e23-b05d-5eeed948a454"),
    ]
    i = 0
    for i, m in enumerate(TestDataStubs.l2_feed()):
        if not m or m["op"] == "trade":
            pass
        elif (i, m["order"].id) in skip:
            continue
        elif m["op"] == "update":
            book.update(order=m["order"])
        elif m["op"] == "delete":
            book.delete(order=m["order"])
        book.check_integrity()
    assert i == 68462


def test_l1_orderbook():
    book = L1OrderBook(
        instrument_id=TestIdStubs.audusd_id(),
        price_precision=5,
        size_precision=0,
    )
    for i, m in enumerate(TestDataStubs.l1_feed()):  # noqa (B007)
        # print(f"[{i}]", "\n", m, "\n", repr(ob), "\n")
        # print("")
        if m["op"] == "update":
            book.update(order=m["order"])
        else:
            raise KeyError
        book.check_integrity()
    assert i == 1999
