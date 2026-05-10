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
RSS-based memory monitoring for capsule round-trip.

Unlike tracemalloc (which only tracks Python allocations), this monitors actual
process RSS which includes Rust allocations via the system allocator.

Run as: python tests/mem_leak_tests/rss_monitor_capsule.py

"""

import gc
import os
import subprocess
import sys
import time

from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick as Pyo3QuoteTick
from nautilus_trader.model.data import capsule_to_data


def get_rss_mb() -> float:
    pid = os.getpid()

    # Use ps command which gives current RSS (not max RSS like rusage)
    result = subprocess.run(  # noqa: S603 (safe - no untrusted input)
        ["/bin/ps", "-o", "rss=", "-p", str(pid)],
        capture_output=True,
        text=True,
        check=True,
    )
    rss_kb = int(result.stdout.strip())
    return rss_kb / 1024


def run_sustained_test(duration_seconds: int = 60, report_interval: int = 5):
    """
    Run capsule round-trip continuously and monitor RSS growth.

    Parameters
    ----------
    duration_seconds : int
        How long to run the test.
    report_interval : int
        How often to report RSS (in seconds).

    """
    print("=" * 70)
    print("RSS Monitor: Capsule Round-Trip Memory Test")
    print("=" * 70)
    print(f"Duration: {duration_seconds}s, Report interval: {report_interval}s")
    print()

    # Pre-create instruments to simulate multi-instrument scenario
    instruments = [InstrumentId.from_str(f"TEST{i}.GLBX") for i in range(100)]

    gc.collect()
    initial_rss = get_rss_mb()
    print(f"Initial RSS: {initial_rss:.2f} MB")
    print()

    start_time = time.time()
    last_report = start_time
    iteration = 0
    ts = 0

    rss_samples = [(0.0, initial_rss)]

    while (time.time() - start_time) < duration_seconds:
        # Simulate live data: create pyo3 object, convert via capsule
        instrument_id = instruments[iteration % len(instruments)]
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

        iteration += 1
        ts += 1

        # Periodic GC and reporting
        current_time = time.time()
        if current_time - last_report >= report_interval:
            gc.collect()
            current_rss = get_rss_mb()
            elapsed = current_time - start_time
            growth = current_rss - initial_rss
            rate = (growth / elapsed) * 60 if elapsed > 0 else 0

            print(
                f"[{elapsed:5.1f}s] RSS: {current_rss:.2f} MB "
                f"(+{growth:.2f} MB, {rate:.2f} MB/min) "
                f"- {iteration:,} iterations",
            )

            rss_samples.append((elapsed, current_rss))
            last_report = current_time

    # Final report
    gc.collect()
    final_rss = get_rss_mb()
    total_time = time.time() - start_time
    total_growth = final_rss - initial_rss
    rate_per_min = (total_growth / total_time) * 60 if total_time > 0 else 0
    bytes_per_iter = (total_growth * 1024 * 1024) / iteration if iteration > 0 else 0

    print()
    print("=" * 70)
    print("SUMMARY")
    print("=" * 70)
    print(f"Total iterations: {iteration:,}")
    print(f"Total time: {total_time:.1f}s")
    print(f"Initial RSS: {initial_rss:.2f} MB")
    print(f"Final RSS: {final_rss:.2f} MB")
    print(f"Total growth: {total_growth:.2f} MB")
    print(f"Growth rate: {rate_per_min:.2f} MB/min")
    print(f"Bytes per iteration: {bytes_per_iter:.1f}")
    print()

    # Assess leak
    if rate_per_min > 1.0:
        print("WARNING: Significant memory growth detected!")
        print(f"  Issue #3485 reported ~7 MB/min, we see {rate_per_min:.2f} MB/min")
    elif rate_per_min > 0.1:
        print("NOTICE: Minor memory growth detected (may be normal)")
    else:
        print("OK: No significant memory growth in capsule path")

    return {
        "iterations": iteration,
        "duration_s": total_time,
        "initial_rss_mb": initial_rss,
        "final_rss_mb": final_rss,
        "growth_mb": total_growth,
        "rate_mb_per_min": rate_per_min,
        "bytes_per_iter": bytes_per_iter,
        "samples": rss_samples,
    }


if __name__ == "__main__":
    duration = int(sys.argv[1]) if len(sys.argv) > 1 else 60
    run_sustained_test(duration_seconds=duration)
