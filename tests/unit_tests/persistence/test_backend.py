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

import os

import pandas as pd

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core.nautilus_pyo3 import DataBackendSession
from nautilus_trader.core.nautilus_pyo3 import NautilusDataType
from nautilus_trader.model.data.base import capsule_to_list


def test_backend_session_order_book() -> None:
    # Arrange
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/order_book_deltas.parquet")
    assert pd.read_parquet(parquet_data_path).shape[0] == 1077
    session = DataBackendSession()
    session.add_file(NautilusDataType.OrderBookDelta, "order_book_deltas", parquet_data_path)

    # Act
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(capsule_to_list(chunk))

    # Assert
    assert len(ticks) == 1077
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_backend_session_quotes() -> None:
    # Arrange
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    session = DataBackendSession()
    session.add_file(NautilusDataType.QuoteTick, "quote_ticks", parquet_data_path)

    # Act
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(capsule_to_list(chunk))

    # Assert
    assert len(ticks) == 9500
    assert str(ticks[-1]) == "EUR/USD.SIM,1.12130,1.12132,0,0,1577919652000000125"
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_backend_session_trades() -> None:
    # Arrange
    trades_path = os.path.join(PACKAGE_ROOT, "tests/test_data/trade_tick_data.parquet")
    session = DataBackendSession()
    session.add_file(NautilusDataType.TradeTick, "trade_ticks", trades_path)

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
    trades_path = os.path.join(PACKAGE_ROOT, "tests/test_data/bar_data.parquet")
    session = DataBackendSession()
    session.add_file(NautilusDataType.Bar, "bars_01", trades_path)

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
    trades_path = os.path.join(PACKAGE_ROOT, "tests/test_data/trade_tick_data.parquet")
    quotes_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    session = DataBackendSession()
    session.add_file(NautilusDataType.TradeTick, "trades_01", trades_path)
    session.add_file(NautilusDataType.QuoteTick, "quotes_01", quotes_path)

    # Act
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(capsule_to_list(chunk))

    # Assert
    assert len(ticks) == 9600
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending
