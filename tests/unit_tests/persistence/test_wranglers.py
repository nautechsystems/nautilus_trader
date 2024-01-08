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

from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.persistence.loaders import BinanceOrderBookDeltaDataLoader
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from tests import TEST_DATA_DIR


def test_load_binance_deltas() -> None:
    # Arrange
    instrument = TestInstrumentProvider.btcusdt_binance()
    data_path = TEST_DATA_DIR / "binance" / "btcusdt-depth-snap.csv"
    df = BinanceOrderBookDeltaDataLoader.load(data_path)

    wrangler = OrderBookDeltaDataWrangler(instrument)

    # Act
    deltas = wrangler.process(df)

    # Assert
    assert len(deltas) == 100
    assert deltas[0].action == BookAction.ADD
    assert deltas[0].order.side == OrderSide.BUY
    assert deltas[0].flags == 42  # Snapshot


@pytest.mark.parametrize(
    ("timestamp_is_close", "interval_ms", "ts_event1", "ts_event2", "ts_event3", "ts_event4"),
    [
        [
            True,
            100,
            1359676799700000000,
            1359676799800000000,
            1359676799900000000,
            1359676800000000000,
        ],
        [
            False,
            50,
            1359676800000000000,
            1359676800049999872,
            1359676800100000000,
            1359676800150000128,
        ],
    ],
)
def test_bar_data_wrangler(
    timestamp_is_close: bool,
    interval_ms: int,
    ts_event1: int,
    ts_event2: int,
    ts_event3: int,
    ts_event4: int,
) -> None:
    # Arrange
    usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
    wrangler = QuoteTickDataWrangler(instrument=usdjpy)
    provider = TestDataProvider()

    # Act
    ticks = wrangler.process_bar_data(
        bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv"),
        ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv"),
        offset_interval_ms=interval_ms,
        timestamp_is_close=timestamp_is_close,
    )

    # Assert
    assert ticks[0].ts_event == ts_event1
    assert ticks[1].ts_event == ts_event2
    assert ticks[2].ts_event == ts_event3
    assert ticks[3].ts_event == ts_event4
