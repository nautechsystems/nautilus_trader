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

from nautilus_trader.core.nautilus_pyo3 import StreamingFeatherWriterV2
from nautilus_trader.core.nautilus_pyo3.common import Cache as PyCache
from nautilus_trader.core.nautilus_pyo3.common import Clock as PyClock
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")


def test_streaming_feather_writer_v2_creation(catalog: ParquetDataCatalog):
    """
    Test creating a StreamingFeatherWriterV2 instance.

    Note: PyClock and PyCache need to be created from Rust types.
    For now, these tests are skipped as they require proper actor setup to get PyClock/PyCache.

    """
    pytest.skip("PyClock and PyCache require actor setup - test via integration tests")

    path = os.path.join(catalog.path, "streaming_test")

    # Act
    # These variables are not defined because the test is skipped
    # They would be created from actor setup in a real test
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,  # type: ignore[name-defined]  # noqa: F821
        clock=py_clock,  # type: ignore[name-defined]  # noqa: F821
    )

    # Assert - should create without error
    assert writer is not None


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_write_quote_tick(catalog: ParquetDataCatalog):
    """
    Test writing a QuoteTick to StreamingFeatherWriterV2.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,
        clock=py_clock,
    )

    quote = TestDataProviderPyo3.quote_tick()

    # Act
    writer.write(quote)

    # Assert - should write without error
    writer.flush()


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_write_trade_tick(catalog: ParquetDataCatalog):
    """
    Test writing a TradeTick to StreamingFeatherWriterV2.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,
        clock=py_clock,
    )

    trade = TestDataProviderPyo3.trade_tick()

    # Act
    writer.write(trade)

    # Assert - should write without error
    writer.flush()


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_write_all_types(catalog: ParquetDataCatalog):
    """
    Test writing all supported data types to StreamingFeatherWriterV2.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,
        clock=py_clock,
    )

    # Act - Write different data types
    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)

    trade = TestDataProviderPyo3.trade_tick()
    writer.write(trade)

    # Assert - should write without error
    writer.flush()


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_flush(catalog: ParquetDataCatalog):
    """
    Test flushing StreamingFeatherWriterV2 buffers.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,
        clock=py_clock,
    )

    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)

    # Act
    writer.flush()

    # Assert - should flush without error


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_close(catalog: ParquetDataCatalog):
    """
    Test closing StreamingFeatherWriterV2.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,
        clock=py_clock,
    )

    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)

    # Act
    writer.close()

    # Assert - should close without error


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_rotation_modes(catalog: ParquetDataCatalog):
    """
    Test creating StreamingFeatherWriterV2 with different rotation modes.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")

    # Act & Assert - test all rotation modes
    # 0 = SIZE
    writer1 = StreamingFeatherWriterV2(
        path=f"{path}_size",
        cache=py_cache,
        clock=py_clock,
        rotation_mode=0,
        max_file_size=1024 * 1024,  # 1MB
    )
    assert writer1 is not None

    # 1 = INTERVAL
    writer2 = StreamingFeatherWriterV2(
        path=f"{path}_interval",
        cache=py_cache,
        clock=py_clock,
        rotation_mode=1,
        rotation_interval_ns=3600_000_000_000,  # 1 hour
    )
    assert writer2 is not None

    # 2 = SCHEDULED_DATES
    writer3 = StreamingFeatherWriterV2(
        path=f"{path}_scheduled",
        cache=py_cache,
        clock=py_clock,
        rotation_mode=2,
        rotation_interval_ns=86400_000_000_000,  # 1 day
        rotation_time_ns=0,  # midnight
    )
    assert writer3 is not None

    # 3 = NO_ROTATION (default)
    writer4 = StreamingFeatherWriterV2(
        path=f"{path}_no_rotation",
        cache=py_cache,
        clock=py_clock,
        rotation_mode=3,
    )
    assert writer4 is not None


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_include_types(catalog: ParquetDataCatalog):
    """
    Test creating StreamingFeatherWriterV2 with include_types filter.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")

    # Act
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,
        clock=py_clock,
        include_types=["quotes", "trades"],
    )

    # Assert - should create without error
    assert writer is not None


@pytest.mark.skip(reason="Requires PyClock/PyCache from actor setup")
def test_streaming_feather_writer_v2_flush_interval(catalog: ParquetDataCatalog):
    """
    Test creating StreamingFeatherWriterV2 with flush_interval_ms.
    """
    # Arrange
    py_clock = PyClock.new_test()
    py_cache = PyCache.from_rc(None)
    path = os.path.join(catalog.path, "streaming_test")

    # Act
    writer = StreamingFeatherWriterV2(
        path=path,
        cache=py_cache,
        clock=py_clock,
        flush_interval_ms=500,  # 500ms
    )

    # Assert - should create without error
    assert writer is not None

    # Write some data
    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)

    # Flush should work
    writer.flush()
