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

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


@pytest.mark.skip(reason="development_only")
def test_orderbook_spy_xnas_itch_mbo_l3(benchmark) -> None:
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento" / "temp" / "spy-xnas-itch-20231127.mbo.dbn.zst"
    instrument = TestInstrumentProvider.equity(symbol="SPY", venue="XNAS")
    data = loader.from_dbn_file(path, instrument_id=instrument.id, as_legacy_cython=True)

    book = TestDataStubs.make_book(
        instrument=instrument,
        book_type=BookType.L3_MBO,
    )

    def _apply_deltas():
        for delta in data:
            if not isinstance(delta, OrderBookDelta):
                continue
            book.apply_delta(delta)

    benchmark(_apply_deltas)

    # Assert
    assert book.ts_last == 1701129555644234540
    assert book.sequence == 429411899
    assert book.count == 6197580
    assert len(book.bids()) == 52
    assert len(book.asks()) == 38
    assert book.best_bid_price() == Price.from_str("454.84")
    assert book.best_ask_price() == Price.from_str("454.90")
