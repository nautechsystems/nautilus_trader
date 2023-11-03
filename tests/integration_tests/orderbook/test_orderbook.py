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
import pytest

from nautilus_trader.model.enums import BookType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orderbook import OrderBook
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests import TEST_DATA_DIR


class TestOrderBook:
    def test_l1_orderbook(self):
        book = OrderBook(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L1_MBP,
        )
        i = 0
        for i, m in enumerate(TestDataStubs.l1_feed()):
            if m["op"] == "update":
                book.update(order=m["order"], ts_event=0)
            else:
                raise KeyError
            book.check_integrity()
        assert i == 1999

    def test_l2_feed(self):
        filename = TEST_DATA_DIR / "bitmex" / "L2_feed.json"

        book = OrderBook(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
        )

        # Duplicate delete messages
        skip = [
            (12152, "378a3caf-0262-4d8b-95b6-8df65312b9f3"),
            (28646, "8101452c-8a80-4ca9-b0d9-c472691cec28"),
            (68431, "8913f4bf-cc49-4e23-b05d-5eeed948a454"),
        ]
        i = 0
        for i, m in enumerate(TestDataStubs.l2_feed(filename)):
            if not m or m["op"] == "trade":
                pass
            elif (i, m["order"].order_id) in skip:
                continue
            elif m["op"] == "update":
                book.update(order=m["order"], ts_event=0)
            elif m["op"] == "delete":
                book.delete(order=m["order"], ts_event=0)
            book.check_integrity()
        assert i == 68462

    @pytest.mark.skip("segfault on check_integrity")
    def test_l3_feed(self):
        filename = TEST_DATA_DIR / "bitmex" / "L3_feed.json"

        book = OrderBook(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L3_MBO,
        )

        # Updates that cause the book to fail integrity checks will be deleted
        # immediately, however we may also delete later.
        skip_deletes = []
        i = 0
        for i, m in enumerate(TestDataStubs.l3_feed(filename)):
            if m["op"] == "update":
                book.update(order=m["order"], ts_event=0)
                try:
                    book.check_integrity()
                except RuntimeError:  # BookIntegrityError was removed
                    book.delete(order=m["order"], ts_event=0)
                    skip_deletes.append(m["order"].order_id)
            elif m["op"] == "delete" and m["order"].order_id not in skip_deletes:
                book.delete(order=m["order"], ts_event=0)
            book.check_integrity()
        assert i == 100_047
        assert book.best_ask_level().price == 61405.27923706
        assert book.best_ask_level().volume() == 0.12227
        assert book.best_bid_level().price == Price.from_int(61_391)
        assert book.best_bid_level().volume() == 1
