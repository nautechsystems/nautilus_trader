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
import pathlib

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core.nautilus_pyo3.persistence import DataBackendSession
from nautilus_trader.core.nautilus_pyo3.persistence import NautilusDataType
from nautilus_trader.persistence.wranglers import list_from_capsule


def test_python_catalog_data():
    trades_path = os.path.join(PACKAGE_ROOT, "tests/test_data/trade_tick_data.parquet")
    quotes_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    session = DataBackendSession()
    session.add_file("trade_ticks", trades_path, NautilusDataType.TradeTick)
    session.add_file("quote_ticks", quotes_path, NautilusDataType.QuoteTick)
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(list_from_capsule(chunk))

    assert len(ticks) == 9600
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_python_catalog_trades():
    trades_path = os.path.join(PACKAGE_ROOT, "tests/test_data/trade_tick_data.parquet")
    session = DataBackendSession()
    session.add_file("trade_ticks", trades_path, NautilusDataType.TradeTick)
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(list_from_capsule(chunk))

    assert len(ticks) == 100
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_python_catalog_quotes():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    session = DataBackendSession()
    session.add_file("quote_ticks", parquet_data_path, NautilusDataType.QuoteTick)
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(list_from_capsule(chunk))

    assert len(ticks) == 9500
    assert str(ticks[-1]) == "EUR/USD.SIM,1.12130,1.12132,0,0,1577919652000000125"
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending


def test_python_catalog_order_book(load_betfair_data):
    fn = "tests/unit_tests/persistence/data_catalog/data/order_book_delta/1.166564490-60424-0.0.BETFAIR/part-0.parquet"
    parquet_data_path = os.path.join(PACKAGE_ROOT, fn)
    assert pathlib.Path(parquet_data_path).exists()
    session = DataBackendSession()
    session.add_file("order_book_deltas", parquet_data_path, NautilusDataType.OrderBookDelta)
    result = session.to_query_result()

    ticks = []
    for chunk in result:
        ticks.extend(list_from_capsule(chunk))

    assert len(ticks) == 1077
    is_ascending = all(ticks[i].ts_init <= ticks[i].ts_init for i in range(len(ticks) - 1))
    assert is_ascending
