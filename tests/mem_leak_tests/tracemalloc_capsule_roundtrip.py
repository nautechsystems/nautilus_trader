#!/usr/bin/env python3
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
Memory leak investigation test for GitHub issue #3485.

Tests the capsule round-trip path used in live Databento data:
  pyo3 data object -> as_pycapsule() -> capsule_to_data() -> Cython object

This simulates the live data path without requiring a connection.
Run as: python tests/mem_leak_tests/tracemalloc_capsule_roundtrip.py

"""

import gc
import tracemalloc
from pathlib import Path

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick as Pyo3QuoteTick
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


RUST_TEST_DATA = Path(__file__).parents[2] / "crates" / "adapters" / "databento" / "test_data"


def measure_memory_growth(func, iterations: int, label: str) -> dict:
    tracemalloc.start()
    gc.collect()

    initial_snapshot = tracemalloc.take_snapshot()
    initial_memory, _ = tracemalloc.get_traced_memory()

    for i in range(iterations):
        func()
        if (i + 1) % 1000 == 0:
            gc.collect()

    gc.collect()
    final_snapshot = tracemalloc.take_snapshot()
    final_memory, peak_memory = tracemalloc.get_traced_memory()

    top_stats = final_snapshot.compare_to(initial_snapshot, "lineno")

    tracemalloc.stop()

    growth_mb = (final_memory - initial_memory) / (1024 * 1024)
    peak_mb = peak_memory / (1024 * 1024)
    bytes_per_iter = (final_memory - initial_memory) / iterations if iterations > 0 else 0

    print(f"\n=== {label} ===")
    print(f"Iterations: {iterations}")
    print(f"Memory growth: {growth_mb:.3f} MB")
    print(f"Peak memory: {peak_mb:.3f} MB")
    print(f"Growth per iteration: {bytes_per_iter:.1f} bytes")
    print("\nTop allocations:")
    for stat in top_stats[:5]:
        print(f"  {stat}")

    return {
        "label": label,
        "iterations": iterations,
        "growth_mb": growth_mb,
        "peak_mb": peak_mb,
        "bytes_per_iter": bytes_per_iter,
    }


def check_quote_tick_capsule_roundtrip():
    pyo3_quote = TestDataProviderPyo3.quote_tick()

    def roundtrip():
        capsule = pyo3_quote.as_pycapsule()
        _ = capsule_to_data(capsule)

    return measure_memory_growth(roundtrip, 10000, "QuoteTick capsule round-trip (reused obj)")


def check_quote_tick_new_each_iter():
    ts = 0

    def roundtrip():
        nonlocal ts
        pyo3_quote = TestDataProviderPyo3.quote_tick(ts_event=ts, ts_init=ts)
        capsule = pyo3_quote.as_pycapsule()
        _ = capsule_to_data(capsule)
        ts += 1

    return measure_memory_growth(
        roundtrip,
        10000,
        "QuoteTick capsule round-trip (new obj each iter)",
    )


def check_trade_tick_capsule_roundtrip():
    pyo3_trade = TestDataProviderPyo3.trade_tick()

    def roundtrip():
        capsule = pyo3_trade.as_pycapsule()
        _ = capsule_to_data(capsule)

    return measure_memory_growth(roundtrip, 10000, "TradeTick capsule round-trip")


def check_bar_capsule_roundtrip():
    pyo3_bar = TestDataProviderPyo3.bar_5decimal()

    def roundtrip():
        capsule = pyo3_bar.as_pycapsule()
        _ = capsule_to_data(capsule)

    return measure_memory_growth(roundtrip, 10000, "Bar capsule round-trip")


def check_order_book_delta_capsule_roundtrip():
    pyo3_delta = TestDataProviderPyo3.order_book_delta()

    def roundtrip():
        capsule = pyo3_delta.as_pycapsule()
        _ = capsule_to_data(capsule)

    return measure_memory_growth(roundtrip, 10000, "OrderBookDelta capsule round-trip")


def check_order_book_depth10_capsule_roundtrip():
    pyo3_depth = TestDataProviderPyo3.order_book_depth10()

    def roundtrip():
        capsule = pyo3_depth.as_pycapsule()
        _ = capsule_to_data(capsule)

    return measure_memory_growth(roundtrip, 10000, "OrderBookDepth10 capsule round-trip")


def check_capsule_creation_only():
    pyo3_quote = TestDataProviderPyo3.quote_tick()

    def create_capsule_only():
        capsule = pyo3_quote.as_pycapsule()
        del capsule

    return measure_memory_growth(
        create_capsule_only,
        10000,
        "Capsule creation only (no conversion)",
    )


def check_databento_loader_quotes():
    cbbo_path = RUST_TEST_DATA / "test_data.cbbo-1s.dbn.zst"
    if not cbbo_path.exists():
        print(f"Skipping: {cbbo_path} not found")
        return None

    loader = DatabentoDataLoader()

    def load_and_convert():
        _ = loader.from_dbn_file(cbbo_path, as_legacy_cython=True)

    return measure_memory_growth(load_and_convert, 100, "Databento CBBO loader (100 file loads)")


def check_databento_loader_deltas():
    mbo_path = RUST_TEST_DATA / "test_data.mbo.dbn.zst"
    if not mbo_path.exists():
        print(f"Skipping: {mbo_path} not found")
        return None

    loader = DatabentoDataLoader()

    def load_and_convert():
        _ = loader.from_dbn_file(mbo_path, as_legacy_cython=True)

    return measure_memory_growth(load_and_convert, 100, "Databento MBO loader (100 file loads)")


def check_sustained_load():
    pyo3_quote = TestDataProviderPyo3.quote_tick()
    pyo3_trade = TestDataProviderPyo3.trade_tick()

    counter = 0

    def mixed_roundtrip():
        nonlocal counter
        if counter % 5 == 0:
            capsule = pyo3_trade.as_pycapsule()
        else:
            capsule = pyo3_quote.as_pycapsule()
        _ = capsule_to_data(capsule)
        counter += 1

    return measure_memory_growth(mixed_roundtrip, 50000, "Sustained mixed load (50k iterations)")


def check_high_volume_simulation():
    # Simulate ~500 instruments with CBBO-1S schema per issue #3485
    # 500 quotes/second = 30k quotes/minute, 100k iterations ~3 minutes of data
    instruments = [InstrumentId.from_str(f"TEST{i}.GLBX") for i in range(500)]

    ts = 0
    idx = 0

    def simulate_live_msg():
        nonlocal ts, idx
        instrument_id = instruments[idx % 500]
        pyo3_quote = Pyo3QuoteTick(
            instrument_id=instrument_id,
            bid_price=Price.from_str("100.00"),
            ask_price=Price.from_str("100.01"),
            bid_size=Quantity.from_str("100"),
            ask_size=Quantity.from_str("100"),
            ts_event=ts,
            ts_init=ts,
        )
        capsule = pyo3_quote.as_pycapsule()
        _ = capsule_to_data(capsule)
        ts += 1
        idx += 1

    return measure_memory_growth(
        simulate_live_msg,
        100000,
        "High-volume simulation (100k msgs, 500 instruments)",
    )


def run_all_checks():
    print("=" * 70)
    print("Memory Leak Investigation for GitHub Issue #3485")
    print("Testing capsule round-trip path: as_pycapsule() -> capsule_to_data()")
    print("=" * 70)

    results = []
    results.append(check_quote_tick_capsule_roundtrip())
    results.append(check_trade_tick_capsule_roundtrip())
    results.append(check_bar_capsule_roundtrip())
    results.append(check_order_book_delta_capsule_roundtrip())
    results.append(check_order_book_depth10_capsule_roundtrip())
    results.append(check_quote_tick_new_each_iter())
    results.append(check_capsule_creation_only())
    results.append(check_databento_loader_quotes())
    results.append(check_databento_loader_deltas())
    results.append(check_sustained_load())
    results.append(check_high_volume_simulation())

    print("\n" + "=" * 70)
    print("SUMMARY")
    print("=" * 70)

    leak_threshold = 10.0  # bytes per iteration threshold for leak detection

    for r in results:
        if r is None:
            continue
        status = "LEAK?" if r["bytes_per_iter"] > leak_threshold else "OK"
        print(f"{r['label']}: {r['bytes_per_iter']:.1f} bytes/iter [{status}]")


if __name__ == "__main__":
    run_all_checks()
