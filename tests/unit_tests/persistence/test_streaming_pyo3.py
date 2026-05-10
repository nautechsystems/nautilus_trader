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

import os
import sys

import pytest

from nautilus_trader.core.nautilus_pyo3 import StreamingFeatherWriter
from nautilus_trader.core.nautilus_pyo3.common import Cache
from nautilus_trader.core.nautilus_pyo3.common import Clock
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")


def _make_writer(path, **kwargs):
    """
    Create a StreamingFeatherWriter, ensuring the path directory exists.
    """
    os.makedirs(path, exist_ok=True)
    return StreamingFeatherWriter(
        path=path,
        cache=kwargs.pop("cache", Cache()),
        clock=kwargs.pop("clock", Clock.new_test()),
        **kwargs,
    )


def test_streaming_feather_writer_creation(catalog: ParquetDataCatalog):
    """
    Test creating a StreamingFeatherWriter instance.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")

    # Act
    writer = _make_writer(path)

    # Assert
    assert writer is not None


def test_streaming_feather_writer_write_quote_tick(catalog: ParquetDataCatalog):
    """
    Test writing a QuoteTick to StreamingFeatherWriter.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")
    writer = _make_writer(path)
    quote = TestDataProviderPyo3.quote_tick()

    # Act
    writer.write(quote)

    # Assert
    writer.flush()


def test_streaming_feather_writer_write_trade_tick(catalog: ParquetDataCatalog):
    """
    Test writing a TradeTick to StreamingFeatherWriter.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")
    writer = _make_writer(path)
    trade = TestDataProviderPyo3.trade_tick()

    # Act
    writer.write(trade)

    # Assert
    writer.flush()


def test_streaming_feather_writer_write_all_types(catalog: ParquetDataCatalog):
    """
    Test writing all supported data types to StreamingFeatherWriter.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")
    writer = _make_writer(path)
    quote = TestDataProviderPyo3.quote_tick()
    trade = TestDataProviderPyo3.trade_tick()

    # Act
    writer.write(quote)
    writer.write(trade)

    # Assert
    writer.flush()


def test_streaming_feather_writer_flush(catalog: ParquetDataCatalog):
    """
    Test flushing StreamingFeatherWriter buffers.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")
    writer = _make_writer(path)
    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)

    # Act
    writer.flush()


def test_streaming_feather_writer_close(catalog: ParquetDataCatalog):
    """
    Test closing StreamingFeatherWriter.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")
    writer = _make_writer(path)
    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)

    # Act
    writer.close()


def test_streaming_feather_writer_rotation_modes(catalog: ParquetDataCatalog):
    """
    Test creating StreamingFeatherWriter with different rotation modes.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")
    cache = Cache()
    clock = Clock.new_test()

    # Act
    writer1 = _make_writer(
        f"{path}_size",
        cache=cache,
        clock=clock,
        rotation_mode=0,  # SIZE
        max_file_size=1024 * 1024,
    )
    writer2 = _make_writer(
        f"{path}_interval",
        cache=cache,
        clock=clock,
        rotation_mode=1,  # INTERVAL
        rotation_interval_ns=3600_000_000_000,
    )
    writer3 = _make_writer(
        f"{path}_scheduled",
        cache=cache,
        clock=clock,
        rotation_mode=2,  # SCHEDULED_DATES
        rotation_interval_ns=86400_000_000_000,
        rotation_time_ns=0,
    )
    writer4 = _make_writer(
        f"{path}_no_rotation",
        cache=cache,
        clock=clock,
        rotation_mode=3,  # NO_ROTATION (default)
    )

    # Assert
    assert writer1 is not None
    assert writer2 is not None
    assert writer3 is not None
    assert writer4 is not None


def test_streaming_feather_writer_include_types(catalog: ParquetDataCatalog):
    """
    Test creating StreamingFeatherWriter with include_types filter.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")

    # Act
    writer = _make_writer(path, include_types=["quotes", "trades"])

    # Assert
    assert writer is not None


def test_streaming_feather_writer_flush_interval(catalog: ParquetDataCatalog):
    """
    Test creating StreamingFeatherWriter with flush_interval_ms.
    """
    # Arrange
    path = os.path.join(catalog.path, "streaming_test")
    writer = _make_writer(path, flush_interval_ms=500)
    quote = TestDataProviderPyo3.quote_tick()

    # Act
    writer.write(quote)

    # Assert
    assert writer is not None
    writer.flush()
