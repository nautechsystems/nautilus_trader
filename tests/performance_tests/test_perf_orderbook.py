# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.orderbook.book import L3OrderBook
from tests.test_kit.providers import TestDataProvider


def run_l3_test(ob, feed):
    for m in feed:
        if m["op"] == "update":
            ob.update(order=m["order"])
        elif m["op"] == "delete":
            ob.delete(order=m["order"])
    return ob


def test_orderbook_updates(benchmark):
    # We only care about the actual updates here, so instantiate orderbook and
    # load updates outside of benchmark
    ob = L3OrderBook()
    feed = TestDataProvider.l3_feed()
    assert len(feed) == 100048  # 100k updates

    # benchmark something
    ob = benchmark(run_l3_test, ob=ob, feed=feed)

    # Assertions from integration test
    assert ob.best_ask_level().price() == 61405.27923706
    assert ob.best_ask_level().volume() == 0.12227
    assert ob.best_bid_level().price() == 61391
    assert ob.best_bid_level().volume() == 1
