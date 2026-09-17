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
"""
Parquet catalog regression tests.
"""

import pyarrow as pa
import pyarrow.parquet as pq
import pytest
from tests.stubs import TestDataProviderPyo3

from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.model import NautilusDataType
from nautilus_trader.persistence import CatalogBackend
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import StreamingFeatherWriter
from nautilus_trader.persistence.catalog_to_df import CatalogOutput
from nautilus_trader.persistence.catalog_to_df import query_catalog


def test_parquet_roundtrip_and_dataframe_preserve_nanoseconds(tmp_path) -> None:
    """
    Verify parquet roundtrip and dataframe preserve nanoseconds.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    timestamps = [1_700_000_000_000_000_001, 1_700_000_000_000_000_123]
    quotes = [TestDataProviderPyo3.quote_tick(ts_event=ts - 1, ts_init=ts) for ts in timestamps]
    catalog.write_quote_ticks(quotes)

    actual = catalog.query_quote_ticks()
    table = query_catalog(catalog, NautilusDataType.QuoteTick, output=CatalogOutput.ARROW)
    physical = pq.read_table(next(tmp_path.rglob("*.parquet")))

    assert actual == quotes
    assert table.schema.field("ts_init").type == pa.timestamp("ns", tz="UTC")
    assert table.column("ts_init").cast(pa.int64()).to_pylist() == timestamps
    assert table.column("ts_event").cast(pa.int64()).to_pylist() == [ts - 1 for ts in timestamps]
    assert physical.schema.field("ts_init").type == pa.timestamp("ns", tz="UTC")
    assert physical.column("ts_init").cast(pa.int64()).to_pylist() == timestamps


def test_parquet_rejects_snapshot_queries(tmp_path) -> None:
    """
    Verify parquet rejects snapshot queries.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    with pytest.raises(ValueError, match="as_of is not supported by ParquetDataCatalog"):
        query_catalog(catalog, NautilusDataType.QuoteTick, as_of=1)


def test_parquet_converts_current_feather_stream(tmp_path) -> None:
    """
    Verify parquet converts current feather stream.
    """
    catalog = ParquetDataCatalog(str(tmp_path))
    staging = tmp_path / "backtest" / "run"
    staging.mkdir(parents=True)
    writer = StreamingFeatherWriter(str(staging), cache=Cache(), clock=Clock.new_test())
    quote = TestDataProviderPyo3.quote_tick(ts_event=123_456_788, ts_init=123_456_789)
    writer.write(quote)
    writer.close()

    catalog.convert_stream_to_data("run", "quotes")

    assert catalog.query_quote_ticks() == [quote]


def test_parquet_backend_is_builtin() -> None:
    """
    Verify parquet backend is builtin.
    """
    assert CatalogBackend.from_str("parquet") == CatalogBackend.Parquet
    assert CatalogBackend.Parquet.name == "Parquet"
    assert CatalogBackend.Parquet.value == "Parquet"
