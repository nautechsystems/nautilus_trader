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

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.bybit.loaders import BybitOrderBookDeltaDataLoader
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_load_bybit_deltas() -> None:
    # Arrange
    instrument = TestInstrumentProvider.xrpusdt_linear_bybit()
    data_path = TEST_DATA_DIR / "bybit" / "xrpusdt-ob500.data.zip"
    df = BybitOrderBookDeltaDataLoader.load(data_path)

    wrangler = OrderBookDeltaDataWrangler(instrument)

    # Act
    deltas = wrangler.process(df)

    # Assert
    assert len(deltas) == 3968
    assert deltas[0].action == BookAction.CLEAR
    assert deltas[1].action == BookAction.ADD
    assert deltas[1].order.side == OrderSide.SELL
    assert deltas[1].flags == RecordFlag.F_SNAPSHOT
    assert deltas[1002].action == BookAction.UPDATE
    assert deltas[1235].order.side == OrderSide.SELL
