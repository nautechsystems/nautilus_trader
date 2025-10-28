#!/usr/bin/env python3
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

from __future__ import annotations

import argparse
import asyncio

import pandas as pd

from nautilus_trader.adapters.okx.factories import get_cached_okx_http_client
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3


MAX_PAGE_SIZE = 100
TZ_UTC = "UTC"  # simpler literal


# ----------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------
def utc_now() -> pd.Timestamp:
    """
    Timezone-aware `now` helper.
    """
    return pd.Timestamp.now(tz=TZ_UTC)


# ----------------------------------------------------------------------
# Pagination helper
# ----------------------------------------------------------------------
async def paginate_bars(
    http_client,
    *,
    bar_type: nautilus_pyo3.BarType,
    start: pd.Timestamp | None,
    end: pd.Timestamp | None,
    total_limit: int | None,
    sleep_between: float = 0.05,
) -> list[nautilus_pyo3.Bar]:
    remaining = total_limit
    cursor = start
    collected: list = []

    while True:
        batch_limit = MAX_PAGE_SIZE if remaining is None else min(MAX_PAGE_SIZE, remaining)
        batch = await http_client.request_bars(
            bar_type=bar_type,
            start=cursor,
            end=end,
            limit=batch_limit,
        )
        if not batch:
            break

        collected.extend(batch)

        if remaining is not None:
            remaining -= len(batch)
            if remaining <= 0:
                break

        # Advance cursor by 1 ms (matches the adapter's millisecond resolution)
        cursor = pd.Timestamp(batch[-1].ts_event, unit="ns", tz=TZ_UTC) + pd.Timedelta("1ms")
        if end is not None and cursor >= end:
            break

        await asyncio.sleep(sleep_between)

    return collected


# ----------------------------------------------------------------------
# Validation helpers
# ----------------------------------------------------------------------
def assert_chronological(bars: list) -> None:
    """
    Ensure bars are strictly chronological and highlight anomalies.
    """
    if not bars:
        return

    ts = [b.ts_event for b in bars]
    series = pd.Series(ts)

    if not series.is_monotonic_increasing:
        # Locate the first offending pair
        for i in range(1, len(ts)):
            if ts[i] <= ts[i - 1]:
                print(f"Chronological violation at index {i}:")
                print(f"   Bar {i - 1}: {pd.Timestamp(ts[i - 1], tz='UTC')}")
                print(f"   Bar {i}:   {pd.Timestamp(ts[i], tz='UTC')}")
                print(f"   Difference: {ts[i] - ts[i - 1]} ns")
                break

        # Duplicates?
        dup_idx = series.duplicated()
        if dup_idx.any():
            print(f"Found {dup_idx.sum()} duplicate timestamp(s)")

        raise AssertionError("Bars are not strictly ascending")

    # Spot unusual gaps (assumes 1-minute bars when spec.step == 1)
    if len(ts) > 1:
        gaps = pd.Series(ts).diff().dropna()
        expected_ns = 60 * 1_000_000_000  # 60 s
        odd = gaps[(gaps < expected_ns * 0.8) | (gaps > expected_ns * 1.5)]
        if not odd.empty:
            print(f"Detected {len(odd)} gap(s) outside 60 s ±20 % window")

    print(f"Chronological check passed for {len(bars)} bar(s)")


# ----------------------------------------------------------------------
# Test suites
# ----------------------------------------------------------------------
async def quick_tests(http_client, bar_type: nautilus_pyo3.BarType, logger: Logger) -> None:
    logger.info("\n=== QUICK TESTS ===")

    # Latest 5
    latest = await http_client.request_bars(bar_type=bar_type, limit=5)
    logger.info(f"[Quick-1] latest 5 → {len(latest)}")
    assert 0 < len(latest) <= 5
    assert_chronological(latest)

    # Fixed start, 10 bars
    start = utc_now() - pd.Timedelta(hours=1)
    fixed = await http_client.request_bars(bar_type=bar_type, start=start, limit=10)
    logger.info(f"[Quick-2] from {start} → {len(fixed)}")
    assert 0 < len(fixed) <= 10
    assert_chronological(fixed)


async def limit_tests(http_client, bar_type: nautilus_pyo3.BarType, logger: Logger) -> None:
    logger.info("\n=== LIMIT BEHAVIOR TESTS ===")
    tgt_start = utc_now() - pd.Timedelta(hours=8)

    for limit, label in [
        (50, "small"),
        (100, "one page"),
        (150, "1½ pages"),
        (200, "two pages"),
        (300, "three pages"),
        (500, "large"),
    ]:
        bars = await http_client.request_bars(bar_type=bar_type, start=tgt_start, limit=limit)
        logger.info(f"[Limit {limit}] {label} → {len(bars)}")
        assert len(bars) <= limit
        assert_chronological(bars)


async def edge_case_tests(http_client, bar_type: nautilus_pyo3.BarType, logger: Logger) -> None:
    logger.info("\n=== EDGE-CASE TESTS ===")

    # 300-bar manual pagination
    start = utc_now() - pd.Timedelta(hours=6)
    page = await paginate_bars(
        http_client,
        bar_type=bar_type,
        start=start,
        end=None,
        total_limit=300,
    )
    logger.info(f"[Edge-1] manual pagination 300 → {len(page)}")
    assert len(page) == 300
    assert_chronological(page)

    # Future window
    fut_start = utc_now() + pd.Timedelta(days=1)
    fut_end = fut_start + pd.Timedelta(minutes=10)
    try:
        fut = await http_client.request_bars(
            bar_type=bar_type,
            start=fut_start,
            end=fut_end,
            limit=5,
        )
        logger.info(f"[Edge-2] future window returned {len(fut)}")
        assert not fut
    except ValueError:
        logger.info("[Edge-2] future window correctly raised ValueError")

    # Pre-listing
    pre_start = pd.Timestamp("2015-01-01T00:00:00Z")
    pre_end = pre_start + pd.Timedelta(minutes=30)
    pre = await http_client.request_bars(bar_type=bar_type, start=pre_start, end=pre_end, limit=50)
    logger.info(f"[Edge-3] pre-listing → {len(pre)}")
    assert not pre

    # Reversed window
    try:
        await http_client.request_bars(bar_type=bar_type, start=pre_end, end=pre_start, limit=5)
    except ValueError:
        logger.info("[Edge-4] reversed window correctly raised ValueError")

    # Wrong instrument
    wrong_type = nautilus_pyo3.BarType.from_str("BTC-USD-SWAP.OKX-1-MINUTE-SETTLEMENT-EXTERNAL")
    try:
        await http_client.request_bars(bar_type=wrong_type, limit=1)
    except Exception:
        logger.info("[Edge-5] invalid instrument correctly raised")

    logger.info("Edge-case suite passed")


async def pagination_demo(http_client, bar_type: nautilus_pyo3.BarType, logger: Logger) -> None:
    logger.info("\n=== PAGINATION FIX DEMONSTRATION ===")

    # 173-bar request (2 pages)
    start = utc_now() - pd.Timedelta(hours=3)
    demo = await http_client.request_bars(bar_type=bar_type, start=start, limit=173)
    logger.info(f"[Demo-1] 173 requested → {len(demo)}")
    assert len(demo) == 173
    assert_chronological(demo)

    # 300-bar request (3 pages)
    start_large = utc_now() - pd.Timedelta(hours=5)
    demo_large = await http_client.request_bars(bar_type=bar_type, start=start_large, limit=300)
    logger.info(f"[Demo-2] 300 requested → {len(demo_large)}")
    assert len(demo_large) == 300
    assert_chronological(demo_large)

    logger.info("Pagination demo finished - cursors advanced monotonically")  # ← fixed hyphen


# ----------------------------------------------------------------------
# Main entry
# ----------------------------------------------------------------------
async def main(args: argparse.Namespace) -> None:
    nautilus_pyo3.init_tracing()
    _guard = init_logging(level_stdout=LogLevel.TRACE)
    logger = Logger("okx-sandbox")

    http_client = get_cached_okx_http_client()

    # Cache instruments
    inst_type = nautilus_pyo3.OKXInstrumentType.SWAP
    instruments = await http_client.request_instruments(inst_type, None)
    for inst in instruments:
        http_client.add_instrument(inst)
    logger.info(f"Cached {len(instruments)} {inst_type} instruments")

    # BarType
    bar_type = nautilus_pyo3.BarType.from_str(args.bar_type)
    sym = bar_type.instrument_id.symbol.value
    if sym not in http_client.get_cached_symbols():
        raise ValueError(f"Instrument {sym} not in cache")

    # Choose test suite(s)
    if args.pagination or not any(vars(args).values()):
        await pagination_demo(http_client, bar_type, logger)
    if args.limits or not any(vars(args).values()):
        await limit_tests(http_client, bar_type, logger)
    if args.quick or not any(vars(args).values()):
        await quick_tests(http_client, bar_type, logger)
    if args.edge or not any(vars(args).values()):
        await edge_case_tests(http_client, bar_type, logger)

    logger.info("\nAll requested test suites passed")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--bar-type",
        default="BTC-USD-SWAP.OKX-1-MINUTE-LAST-EXTERNAL",
        help="Fully-qualified BarType string",
    )
    grp = parser.add_mutually_exclusive_group()
    grp.add_argument("--quick", action="store_true", help="Run quick sanity checks")
    grp.add_argument("--edge", action="store_true", help="Run edge-case suite")
    grp.add_argument("--limits", action="store_true", help="Run varying-limit checks")
    grp.add_argument("--pagination", action="store_true", help="Run pagination demo")
    asyncio.run(main(parser.parse_args()))
