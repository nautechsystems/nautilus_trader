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

import pytest

from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.enums import BookType
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests import TEST_DATA_DIR


def run_l3_test(book, feed):
    for m in feed:
        if m["op"] == "update":
            book.update(order=m["order"])
        elif m["op"] == "delete":
            book.delete(order=m["order"])
    return book


@pytest.mark.skip(reason="Takes too long")
def test_orderbook_updates(benchmark):
    # We only care about the actual updates here, so instantiate orderbook and
    # load updates outside of benchmark
    book = OrderBook(
        instrument_id=TestIdStubs.audusd_id(),
        book_type=BookType.L3_MBO,
    )
    filename = TEST_DATA_DIR / "L3_feed.json"
    feed = TestDataStubs.l3_feed(filename)
    assert len(feed) == 100048  # 100k updates

    # benchmark something
    # book = benchmark(run_l3_test, book=book, feed=feed)
    benchmark.pedantic(run_l3_test, args=(book, feed), rounds=10, iterations=10, warmup_rounds=5)
