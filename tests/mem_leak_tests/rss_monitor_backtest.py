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
RSS-based memory monitoring for repeated BacktestEngine runs.

Unlike tracemalloc (which only tracks Python allocations), this monitors actual
process RSS which includes Rust allocations via the system allocator. This catches
memory retained by the Rust-side accumulator, cache, and message bus after dispose().

Run as: python tests/mem_leak_tests/rss_monitor_backtest.py

"""

import gc
import os
import subprocess
import sys
from decimal import Decimal

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAP
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAPConfig
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def get_rss_mb() -> float:
    """
    Return current process RSS in megabytes.
    """
    pid = os.getpid()
    result = subprocess.run(  # noqa: S603 (safe - no untrusted input)
        ["/bin/ps", "-o", "rss=", "-p", str(pid)],
        capture_output=True,
        text=True,
        check=True,
    )
    rss_kb = int(result.stdout.strip())
    return rss_kb / 1024


def run_backtest_iteration() -> None:
    """
    Run a single backtest iteration with full setup and teardown.
    """
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(bypass_logging=True),
    )
    engine = BacktestEngine(config=config)

    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=None,
        starting_balances=[Money(1_000_000.0, USDT), Money(10.0, ETH)],
    )
    engine.add_instrument(instrument)

    provider = TestDataProvider()
    wrangler = TradeTickDataWrangler(instrument=instrument)
    ticks = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv"))
    engine.add_data(ticks)

    strategy_config = EMACrossTWAPConfig(
        instrument_id=instrument.id,
        bar_type=BarType.from_str("ETHUSDT.BINANCE-250-TICK-LAST-INTERNAL"),
        trade_size=Decimal("0.05"),
        fast_ema_period=10,
        slow_ema_period=20,
        twap_horizon_secs=10.0,
        twap_interval_secs=2.5,
    )
    strategy = EMACrossTWAP(config=strategy_config)
    engine.add_strategy(strategy=strategy)

    exec_algorithm = TWAPExecAlgorithm()
    engine.add_exec_algorithm(exec_algorithm)

    engine.run()
    engine.reset()
    engine.dispose()

    del engine
    gc.collect()


def run_monitor(total_runs: int = 16, warmup: int = 2) -> dict:
    """
    Run repeated backtests and monitor RSS growth.

    Parameters
    ----------
    total_runs : int
        Total number of backtest iterations.
    warmup : int
        Iterations to skip before measuring growth (allocator warm-up).

    Returns
    -------
    dict
        Summary with RSS samples and growth metrics.

    """
    print("=" * 70)
    print("RSS Monitor: Repeated BacktestEngine Run Memory Test")
    print("=" * 70)
    print(f"Runs: {total_runs}, Warmup: {warmup}")
    print()

    gc.collect()
    initial_rss = get_rss_mb()
    print(f"Initial RSS: {initial_rss:.1f} MB")
    print()

    rss_samples: list[tuple[int, float]] = []

    for i in range(total_runs):
        before = get_rss_mb()
        run_backtest_iteration()
        after = get_rss_mb()
        delta = after - before

        rss_samples.append((i, after))
        print(f"Run {i + 1:3d}/{total_runs}: {after:.1f} MB (delta: {delta:+.1f} MB)")

    final_rss = get_rss_mb()
    total_growth = final_rss - initial_rss

    # Measure growth only after warmup
    if len(rss_samples) > warmup + 1:
        post_warmup_start = rss_samples[warmup][1]
        post_warmup_end = rss_samples[-1][1]
        measured_runs = total_runs - warmup - 1
        measured_growth = post_warmup_end - post_warmup_start
        per_run_growth = measured_growth / measured_runs if measured_runs > 0 else 0
    else:
        per_run_growth = 0
        measured_growth = 0

    print()
    print("=" * 70)
    print("SUMMARY")
    print("=" * 70)
    print(f"Initial RSS:        {initial_rss:.1f} MB")
    print(f"Final RSS:          {final_rss:.1f} MB")
    print(f"Total growth:       {total_growth:.1f} MB")
    print(f"Post-warmup growth: {measured_growth:.1f} MB over {total_runs - warmup - 1} runs")
    print(f"Per-run growth:     {per_run_growth:.1f} MB/run")
    print()

    threshold_mb = 2.0
    if per_run_growth > threshold_mb:
        print(
            f"FAIL: Per-run RSS growth ({per_run_growth:.1f} MB) exceeds {threshold_mb} MB threshold",
        )
        print("Memory is not being properly released by dispose()")
    else:
        print(
            f"OK: Per-run RSS growth ({per_run_growth:.1f} MB) within {threshold_mb} MB threshold",
        )

    return {
        "initial_rss_mb": initial_rss,
        "final_rss_mb": final_rss,
        "total_growth_mb": total_growth,
        "per_run_growth_mb": per_run_growth,
        "samples": rss_samples,
    }


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 16
    run_monitor(total_runs=runs)
