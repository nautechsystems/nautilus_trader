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

import orjson
import pytest

from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.orderbook.book import OrderBook
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


def _fix_ids(r):
    return (
        r.replace(b"1.133262888", b"1.180737206")
        .replace(b"2501003", b"19248890")
        .replace(b"1111884", b"38848248")
        .replace(b"1111887", b"10921178")
    )


@pytest.mark.skip
def test_betfair_orderbook(betfair_data_client, provider):
    provider.search_markets(market_filter={"market_id": "1.180737206"})

    book = OrderBook(
        instrument_id=BetfairTestStubs.instrument_id(), level=OrderBookLevel.L2
    )
    for raw in BetfairTestStubs.raw_orderbook_updates():
        update = orjson.loads(_fix_ids(raw.strip()))
        for operation in on_market_update(update, instrument_provider=provider):
            book.apply_operation(operation)
