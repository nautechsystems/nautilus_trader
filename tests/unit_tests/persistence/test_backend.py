# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

    deltas = []
    for chunk in result:
        deltas.extend(capsule_to_list(chunk))

    # Assert
    assert pd.read_parquet(data_path).shape[0] == 1_077
    assert len(deltas) == 1_077
    # TODO: deltas.parquet does not have non decreasing timestamps and so would fail the
    # is_ascending check (bad data file)


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

    quotes = []
    for chunk in result:
        quotes.extend(capsule_to_list(chunk))

    # TODO: Quote tick test data currently uses incorrectly scaled prices and sizes and needs repair
    # Assert
    assert len(quotes) == 9_500
    assert (
        str(quotes[-1]) == "EUR/USD.SIM,112.13000,112.13200,10000000,10000000,1577919652000000125"
    )
    is_ascending = all(quotes[i].ts_init <= quotes[i + 1].ts_init for i in range(len(quotes) - 1))
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

    trades = []
    for chunk in result:
        trades.extend(capsule_to_list(chunk))

    # Assert
    assert len(trades) == 100
    is_ascending = all(trades[i].ts_init <= trades[i + 1].ts_init for i in range(len(trades) - 1))
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
    is_ascending = all(bars[i].ts_init <= bars[i + 1].ts_init for i in range(len(bars) - 1))
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

    data = []
    for chunk in result:
        data.extend(capsule_to_list(chunk))

    # Assert
    assert len(data) == 9_600
    is_ascending = all(data[i].ts_init <= data[i + 1].ts_init for i in range(len(data) - 1))
    assert is_ascending


def test_backend_session_register_object_store_from_uri_local_file() -> None:
    """
    Test registering object store from local file URI.
    """
    # Arrange
    if HIGH_PRECISION:
        data_path = TEST_DATA_DIR / "nautilus" / "128-bit" / "trades.parquet"
    else:
        data_path = TEST_DATA_DIR / "nautilus" / "64-bit" / "trades.parquet"

    session = DataBackendSession()

    # Act - register object store from local file URI
    file_uri = f"file://{data_path.parent}"
    session.register_object_store_from_uri(file_uri)

    # Add file using the registered object store
    session.add_file(NautilusDataType.TradeTick, "trade_ticks", str(data_path))
    result = session.to_query_result()

    trades = []
    for chunk in result:
        trades.extend(capsule_to_list(chunk))

    # Assert
    assert len(trades) == 100
    is_ascending = all(trades[i].ts_init <= trades[i + 1].ts_init for i in range(len(trades) - 1))
    assert is_ascending


def test_backend_session_mixed_builtin_and_custom_types(tmp_path) -> None:
    """
    Test streaming a session containing both built-in Rust types and custom Python
    types. The Rust backend returns PyCapsules for built-in-only chunks and Python lists
    for chunks containing custom data. Callers must handle both.

    Regression test for
    https://github.com/nautechsystems/nautilus_trader/issues/3853

    """
    from nautilus_trader.core.data import Data
    from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
    from nautilus_trader.model.custom import customdataclass_pyo3
    from nautilus_trader.model.data import TradeTick
    from nautilus_trader.model.enums import AggressorSide
    from nautilus_trader.model.identifiers import TradeId
    from nautilus_trader.model.objects import Price
    from nautilus_trader.model.objects import Quantity
    from nautilus_trader.persistence.catalog import ParquetDataCatalog
    from nautilus_trader.test_kit.providers import TestInstrumentProvider

    @customdataclass_pyo3()
    class MixedTestSignal(Data):
        value: float = 0.0

    register_custom_data_class(MixedTestSignal)

    catalog = ParquetDataCatalog(str(tmp_path / "mixed_catalog"))
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    catalog.write_data([instrument])

    trades = [
        TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("0.67000"),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId(f"mixed_{i}"),
            ts_event=i * 1_000_000_000,
            ts_init=i * 1_000_000_000,
        )
        for i in range(1, 6)
    ]
    catalog.write_data(trades)

    signals = [
        MixedTestSignal(
            ts_event=i * 1_000_000_000 + 500_000_000,
            ts_init=i * 1_000_000_000 + 500_000_000,
            value=float(i),
        )
        for i in range(1, 6)
    ]
    catalog.write_data(signals)

    # Build a session containing both built-in and custom types
    session = DataBackendSession()
    session = catalog.backend_session(data_cls=TradeTick, session=session)
    session = catalog.backend_session(data_cls=MixedTestSignal, session=session)

    # Stream chunks: mixed chunks are Python lists, pure built-in are PyCapsules
    all_data = []

    for chunk in session.to_query_result():
        if isinstance(chunk, list):
            all_data.extend(chunk)
        else:
            all_data.extend(capsule_to_list(chunk))

    assert len(all_data) == 10
    is_ascending = all(
        all_data[i].ts_init <= all_data[i + 1].ts_init for i in range(len(all_data) - 1)
    )
    assert is_ascending

    # Verify both types are present in the merged stream
    type_names = {type(item).__name__ for item in all_data}
    assert "TradeTick" in type_names
    assert "CustomData" in type_names


def test_pyo3_list_to_data_list_unknown_type_raises() -> None:
    """
    Passing an unrecognized object type to pyo3_list_to_data_list raises RuntimeError so
    callers get a clear error instead of silent data loss.
    """
    from nautilus_trader.model.data import pyo3_list_to_data_list

    with pytest.raises(RuntimeError, match="Cannot convert PyO3 data type"):
        pyo3_list_to_data_list(["not a data object"])


def test_backend_session_register_object_store_from_uri_invalid_uri() -> None:
    """
    Test registering object store from invalid URI raises appropriate error.
    """
    # Arrange
    session = DataBackendSession()

    # Act & Assert - invalid URI should raise an error
    with pytest.raises(Exception):  # The specific exception type may vary
        session.register_object_store_from_uri("invalid://not-a-real-uri")


def test_backend_session_register_object_store_from_uri_nonexistent_path() -> None:
    """
    Test registering object store from non-existent path URI.
    """
    # Arrange
    session = DataBackendSession()

    # Act & Assert - non-existent path should raise an error
    with pytest.raises(Exception):  # The specific exception type may vary
        session.register_object_store_from_uri("file:///nonexistent/path")
