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

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.core.nautilus_pyo3 import DataBackendSession
from nautilus_trader.core.nautilus_pyo3 import NautilusDataType
from nautilus_trader.model.data import capsule_to_list
from nautilus_trader.model.objects import HIGH_PRECISION


def test_backend_session_order_book_deltas() -> None:
    # Arrange
    if HIGH_PRECISION:
        data_path = TEST_DATA_DIR / "nautilus" / "128-bit" / "deltas.parquet"
    else:
        data_path = TEST_DATA_DIR / "nautilus" / "64-bit" / "deltas.parquet"

    session = DataBackendSession()
    session.add_file(NautilusDataType.OrderBookDelta, "order_book_deltas", str(data_path))

    # Act
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(capsule_to_list(chunk))

    # Assert
    assert pd.read_parquet(data_path).shape[0] == 1077
    assert len(ticks) == 1077
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_backend_session_quotes() -> None:
    # Arrange
    if HIGH_PRECISION:
        data_path = TEST_DATA_DIR / "nautilus" / "128-bit" / "quotes.parquet"
    else:
        data_path = TEST_DATA_DIR / "nautilus" / "64-bit" / "quotes.parquet"

    session = DataBackendSession()
    session.add_file(NautilusDataType.QuoteTick, "quote_ticks", str(data_path))

    # Act
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(capsule_to_list(chunk))

    # TODO: Quote tick test data currently uses incorrectly scaled prices and sizes and needs repair
    # Assert
    assert len(ticks) == 9_500
    assert str(ticks[-1]) == "EUR/USD.SIM,112.13000,112.13200,10000000,10000000,1577919652000000125"
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_backend_session_trades() -> None:
    # Arrange
    if HIGH_PRECISION:
        data_path = TEST_DATA_DIR / "nautilus" / "128-bit" / "trades.parquet"
    else:
        data_path = TEST_DATA_DIR / "nautilus" / "64-bit" / "trades.parquet"

    session = DataBackendSession()
    session.add_file(NautilusDataType.TradeTick, "trade_ticks", str(data_path))

    # Act
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(capsule_to_list(chunk))

    # Assert
    assert len(ticks) == 100
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_backend_session_bars() -> None:
    # Arrange
    if HIGH_PRECISION:
        data_path = TEST_DATA_DIR / "nautilus" / "128-bit" / "bars.parquet"
    else:
        data_path = TEST_DATA_DIR / "nautilus" / "64-bit" / "bars.parquet"

    session = DataBackendSession()
    session.add_file(NautilusDataType.Bar, "bars_01", str(data_path))

    # Act
    result = session.to_query_result()

    bars = []
    for chunk in result:
        bars.extend(capsule_to_list(chunk))

    # Assert
    assert len(bars) == 10
    is_ascending = all(bars[i].ts_init <= bars[i].ts_init for i in range(len(bars) - 1))
    assert is_ascending


def test_backend_session_multiple_types() -> None:
    # Arrange
    if HIGH_PRECISION:
        trades_path = TEST_DATA_DIR / "nautilus" / "128-bit" / "trades.parquet"
        quotes_path = TEST_DATA_DIR / "nautilus" / "128-bit" / "quotes.parquet"
    else:
        trades_path = TEST_DATA_DIR / "nautilus" / "64-bit" / "trades.parquet"
        quotes_path = TEST_DATA_DIR / "nautilus" / "64-bit" / "quotes.parquet"

    session = DataBackendSession()
    session.add_file(NautilusDataType.TradeTick, "trades_01", str(trades_path))
    session.add_file(NautilusDataType.QuoteTick, "quotes_01", str(quotes_path))

    # Act
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(capsule_to_list(chunk))

    # Assert
    assert len(ticks) == 9_600
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending
