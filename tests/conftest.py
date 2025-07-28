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

import gc
import os
import tracemalloc

import psutil
import pytest

from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# Global memory tracking configuration
# Read from environment variables, with defaults
MEMORY_TRACKING_ENABLED = bool(
    os.environ.get("MEMORY_TRACKING_ENABLED_PY", "False").lower() in ("true", "1", "yes", "on"),
)
MEMORY_LEAK_THRESHOLD_BYTES = int(
    os.environ.get("MEMORY_LEAK_THRESHOLD_BYTES_PY", 1024 * 1024 * 10),
)  # Default 10 MB


@pytest.fixture(autouse=True)
def memory_tracker(request):
    """
    Automatic memory tracking fixture that runs for all tests.
    """
    if not MEMORY_TRACKING_ENABLED:
        yield
        return

    # Start tracemalloc for detailed memory tracking
    tracemalloc.start()
    initial_tracemalloc, _ = tracemalloc.get_traced_memory()

    # Force garbage collection before test
    gc.collect()

    # Store initial memory state
    process = psutil.Process()
    initial_memory_bytes = process.memory_info().rss

    # Run the test
    yield

    # Force garbage collection after test
    gc.collect()

    final_tracemalloc, peak = tracemalloc.get_traced_memory()

    # Calculate memory usage
    process = psutil.Process()
    final_memory_bytes = process.memory_info().rss
    memory_increase_bytes = final_memory_bytes - initial_memory_bytes

    try:
        # Only report and fail if memory increase is significant
        if memory_increase_bytes > MEMORY_LEAK_THRESHOLD_BYTES:
            test_name = request.node.nodeid
            initial_mb = initial_memory_bytes / 1024 / 1024
            final_mb = final_memory_bytes / 1024 / 1024
            increase_mb = memory_increase_bytes / 1024 / 1024
            initial_tracemalloc_mb = initial_tracemalloc / 1024 / 1024
            final_tracemalloc_mb = final_tracemalloc / 1024 / 1024
            peak_tracemalloc_mb = peak / 1024 / 1024
            threshold_mb = MEMORY_LEAK_THRESHOLD_BYTES / 1024 / 1024

            print(f"\nMemory Leak Detected in {test_name}")
            print(f"  Initial RSS: {initial_mb:.2f} MB")
            print(f"  Final RSS: {final_mb:.2f} MB")
            print(f"  Memory Growth: {increase_mb:.2f} MB")
            print(f"  Initial Tracemalloc: {initial_tracemalloc_mb:.2f} MB")
            print(f"  Final Tracemalloc: {final_tracemalloc_mb:.2f} MB")
            print(f"  Peak Tracemalloc: {peak_tracemalloc_mb:.2f} MB")
            print(f"  Threshold: {threshold_mb:.2f} MB")
            print("")

            # Get and print top 10 memory allocations
            snapshot = tracemalloc.take_snapshot()
            top_stats = snapshot.statistics("lineno")

            print("\n  Top 10 Memory Allocations:")
            print("  " + "-" * 80)
            for index, stat in enumerate(top_stats[:10], 1):
                traceback = f"{stat.traceback}"
                size_mb = stat.size / 1024 / 1024
                print(f"  {index:2d}. {traceback:<60} {size_mb:>8.2f} MB ({stat.count:,} blocks)")
            print("  " + "-" * 80)

            raise MemoryError("Memory leak detected during test execution.")
    finally:
        # Stop tracemalloc
        tracemalloc.stop()


@pytest.fixture(scope="session", autouse=True)
def bypass_logging():
    """
    Fixture to bypass logging for all tests.

    `autouse=True` will mean this function is run prior to every test. To disable this
    to debug specific tests, simply comment this out.

    """
    # Uncomment below for tracing logs from Rust
    # from nautilus_trader.core import nautilus_pyo3
    # nautilus_pyo3.init_tracing()
    guard = init_logging(
        level_stdout=LogLevel.DEBUG,
        bypass=True,  # Set this to False to see logging in tests
        # print_config=True,
    )
    # Yield guard to keep it alive for the session lifetime, avoiding garbage collection
    yield guard


@pytest.fixture(name="audusd_instrument")
def fixture_audusd_instrument() -> CurrencyPair:
    return TestInstrumentProvider.default_fx_ccy("AUD/USD", Venue("SIM"))


@pytest.fixture(name="data_provider", scope="session")
def fixture_data_provider() -> TestDataProvider:
    return TestDataProvider()


@pytest.fixture(name="audusd_quote_ticks", scope="session")
def fixture_audusd_quote_ticks(
    data_provider: TestDataProvider,
    audusd_instrument: CurrencyPair,
) -> list[QuoteTick]:
    wrangler = QuoteTickDataWrangler(instrument=audusd_instrument)
    return wrangler.process(data_provider.read_csv_ticks("truefx/audusd-ticks.csv"))
