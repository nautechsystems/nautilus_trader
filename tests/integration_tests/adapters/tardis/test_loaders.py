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

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests import TEST_DATA_DIR


def test_tardis_load_deltas():
    # Arrange
    filepath = (
        TEST_DATA_DIR
        / "large"
        / "tardis_deribit_incremental_book_L2_2020-04-01_BTC-PERPETUAL.csv.gz"
    )
    checksums = TEST_DATA_DIR / "large" / "checksums.json"
    url = (
        "https://datasets.tardis.dev/v1/deribit/incremental_book_L2/2020/04/01/BTC-PERPETUAL.csv.gz"
    )
    nautilus_pyo3.ensure_file_exists_or_download_http(
        str(filepath.resolve()),
        url,
        str(checksums.resolve()),
    )

    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    deltas = loader.load_deltas(filepath, limit=1_000)

    # Assert
    assert len(deltas) == 1_000
    assert deltas[0].instrument_id == InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
    assert deltas[0].action == BookAction.ADD
    assert deltas[0].order.side == OrderSide.SELL
    assert deltas[0].order.price == Price.from_str("6421.5")
    assert deltas[0].order.size == Quantity.from_str("18640")
    assert deltas[0].flags == 0
    assert deltas[0].sequence == 0
    assert deltas[0].ts_event == 1585699200245000000
    assert deltas[0].ts_init == 1585699200355684000


def test_tardis_load_trades():
    # Arrange
    filepath = TEST_DATA_DIR / "large" / "tardis_bitmex_trades_2020-03-01_XBTUSD.csv.gz"
    checksums = TEST_DATA_DIR / "large" / "checksums.json"
    url = "https://datasets.tardis.dev/v1/bitmex/trades/2020/03/01/XBTUSD.csv.gz"
    nautilus_pyo3.ensure_file_exists_or_download_http(
        str(filepath.resolve()),
        url,
        str(checksums.resolve()),
    )

    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    trades = loader.load_trades(filepath, limit=1_000)

    # Assert
    assert len(trades) == 1_000
    assert trades[0].instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
    assert trades[0].price == Price.from_str("8531.5")
    assert trades[0].size == Quantity.from_str("2152")
    assert trades[0].aggressor_side == AggressorSide.SELLER
    assert trades[0].trade_id == TradeId("ccc3c1fa-212c-e8b0-1706-9b9c4f3d5ecf")
    assert trades[0].ts_event == 1583020803145000000
    assert trades[0].ts_init == 1583020803307160000
