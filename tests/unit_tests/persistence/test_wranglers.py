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

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.binance.loaders import BinanceOrderBookDeltaDataLoader
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.objects import FIXED_SCALAR
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_load_deltas() -> None:
    # Arrange
    instrument = TestInstrumentProvider.btcusdt_binance()
    data_path = TEST_DATA_DIR / "binance" / "btcusdt-depth-snap.csv"
    df = BinanceOrderBookDeltaDataLoader.load(data_path)

    wrangler = OrderBookDeltaDataWrangler(instrument)

    # Act
    deltas = wrangler.process(df)

    # Assert
    assert len(deltas) == 101
    assert deltas[0].action == BookAction.CLEAR
    assert deltas[1].action == BookAction.ADD
    assert deltas[1].order.side == OrderSide.BUY
    assert deltas[1].flags == RecordFlag.F_SNAPSHOT


bar_timestamp_tests_params = (
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
            1359676800050000000,
            1359676800100000000,
            1359676800150000000,
        ],
    ],
)


@pytest.mark.parametrize(*bar_timestamp_tests_params)
def test_quote_bar_data_wrangler(
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


@pytest.mark.parametrize(*bar_timestamp_tests_params)
def test_trade_bar_data_wrangler(
    timestamp_is_close: bool,
    interval_ms: int,
    ts_event1: int,
    ts_event2: int,
    ts_event3: int,
    ts_event4: int,
) -> None:
    # Arrange
    usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
    wrangler = TradeTickDataWrangler(instrument=usdjpy)
    provider = TestDataProvider()
    data = provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv")
    data.loc[:, "volume"] = 100_0000
    expected_ticks_count = len(data) * 4

    # Act
    ticks = wrangler.process_bar_data(
        data=data,
        offset_interval_ms=interval_ms,
        timestamp_is_close=timestamp_is_close,
    )

    # Assert
    assert ticks[0].ts_event == ts_event1
    assert ticks[1].ts_event == ts_event2
    assert ticks[2].ts_event == ts_event3
    assert ticks[3].ts_event == ts_event4
    assert len(ticks) == expected_ticks_count


@pytest.mark.parametrize("is_raw", [False])
def test_trade_bar_data_wrangler_size_precision(is_raw: bool) -> None:
    # Arrange
    spy = TestInstrumentProvider.equity("SPY", "ARCA")
    factor = FIXED_SCALAR if is_raw else 1
    wrangler = TradeTickDataWrangler(instrument=spy)
    ts = pd.Timestamp("2024-01-05 21:00:00+0000", tz="UTC")
    data = pd.DataFrame(
        {
            "open": {ts: 468.01 * factor},
            "high": {ts: 468.08 * factor},
            "low": {ts: 467.81 * factor},
            "close": {ts: 467.96 * factor},
            "volume": {ts: 18735.0 * factor},
        },
    )

    # Calculate expected_size
    if is_raw:
        # For raw data, adjust precision by -9
        expected_size = round(data["volume"].iloc[0] / 4, spy.size_precision)
    else:
        # For non-raw data, apply standard precision and scale back up to compare with raw
        expected_size = round(data["volume"].iloc[0] / 4, spy.size_precision) * FIXED_SCALAR

    # Act
    ticks = wrangler.process_bar_data(
        data=data,
        offset_interval_ms=0,
        timestamp_is_close=True,
        is_raw=is_raw,
    )

    # Assert
    for tick in ticks:
        assert tick.size.raw == expected_size
