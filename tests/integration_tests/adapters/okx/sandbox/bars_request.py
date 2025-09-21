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
"""
Sandbox script for requesting historical bar data from OKX.

- Initializes the OKXHttpClient
- Caches instruments necessary for parsing with correct precisions
- Requests bars for a specific instrument
- Logs any received bars

This is useful for testing request and pagination logic.

"""

import asyncio

import pandas as pd

from nautilus_trader.adapters.okx.factories import get_cached_okx_http_client
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3


async def main():
    # Setup logging (to see Rust logs run `export RUST_LOG=debug,h2=off`)
    nautilus_pyo3.init_tracing()
    _guard = init_logging(level_stdout=LogLevel.TRACE)
    logger = Logger("okx-sandbox")

    # Setup client: we must cache all instruments we intend on using for requests
    http_client = get_cached_okx_http_client()

    # Instrument type must match the symbol for the bar type
    okx_instrument_type = nautilus_pyo3.OKXInstrumentType.SWAP
    instruments = await http_client.request_instruments(okx_instrument_type)

    logger.info(f"Received {len(instruments)} instruments")

    for inst in instruments:
        http_client.add_instrument(inst)

    logger.info("Cached instruments for HTTP client")

    # Request params (use the correct types for PyO3)
    bar_type = nautilus_pyo3.BarType.from_str("BTC-USD-SWAP.OKX-1-MINUTE-LAST-EXTERNAL")

    # Use relative timestamps so tests remain valid over time
    now = pd.Timestamp.now(tz="UTC")

    # Test with different start times
    start_time_old = now - pd.Timedelta(days=16)  # 16 days ago
    start_time_recent = now - pd.Timedelta(hours=4)  # 4 hours ago

    # Use a recent range for Range mode test
    range_start = now - pd.Timedelta(hours=2)  # 2 hours ago
    range_end = now - pd.Timedelta(hours=1)  # 1 hour ago

    # Test with a 1-hour range like okx_data_tester uses
    narrow_start = now - pd.Timedelta(hours=1)
    narrow_end = now

    # Test with old start time and a reasonable limit
    logger.info(f"Requesting bars with OLD start time ({start_time_old})...")

    bars_old = await http_client.request_bars(
        bar_type=bar_type,
        start=start_time_old,
        # end=end_time,
        limit=100,
    )

    # Test with limit=0 (should be treated as no limit)
    logger.info("Testing with limit=0 (should return bars, not 0 bars)...")

    bars_limit_zero = await http_client.request_bars(
        bar_type=bar_type,
        start=start_time_recent,
        limit=0,
    )

    logger.info(f"Received {len(bars_old)} bar(s) with old start time")
    logger.info(f"Received {len(bars_limit_zero)} bar(s) with limit=0 (expected: many bars)")

    # Test with recent start time
    logger.info(f"Requesting bars with RECENT start time ({start_time_recent})...")

    bars_recent = await http_client.request_bars(
        bar_type=bar_type,
        start=start_time_recent,
        # end=end_time,
        # limit=100,
    )

    logger.info(f"Received {len(bars_recent)} bar(s) with recent start time")

    # Test with both start and end times (Range mode)
    logger.info(f"Requesting bars with RANGE ({range_start} to {range_end})...")

    bars_range = await http_client.request_bars(
        bar_type=bar_type,
        start=range_start,
        end=range_end,
        limit=50,
    )

    logger.info(f"Received {len(bars_range)} bar(s) with start and end time")

    # Test with narrow 1-hour range (mimics okx_data_tester behavior)
    logger.info(f"Requesting bars with NARROW RANGE ({narrow_start} to {narrow_end})...")

    bars_narrow = await http_client.request_bars(
        bar_type=bar_type,
        start=narrow_start,
        end=narrow_end,
        limit=0,  # Same as okx_data_tester
    )

    logger.info(f"Received {len(bars_narrow)} bar(s) with narrow range and limit=0")

    # Test with ETH-USDT-SWAP like okx_data_tester
    eth_bar_type = nautilus_pyo3.BarType.from_str("ETH-USDT-SWAP.OKX-1-MINUTE-LAST-EXTERNAL")
    logger.info(f"Testing ETH-USDT-SWAP with narrow range ({narrow_start} to {narrow_end})...")

    bars_eth = await http_client.request_bars(
        bar_type=eth_bar_type,
        start=narrow_start,
        end=narrow_end,
        limit=0,
    )

    logger.info(f"Received {len(bars_eth)} bar(s) for ETH-USDT-SWAP with narrow range")

    # Now test without start parameter
    logger.info("Testing without start parameter...")
    bars_no_start = await http_client.request_bars(
        bar_type=bar_type,
        # end=end_time,
        # limit=100,
    )

    logger.info(f"Received {len(bars_no_start)} bar(s) without start parameter")

    # Helper function to convert timestamp nanoseconds to pandas timestamp
    def ns_to_timestamp(ts_ns):
        return pd.Timestamp(ts_ns // 1_000_000_000, unit="s").tz_localize("UTC")

    # Validate bars_limit_zero (should have received bars, not 0)
    if bars_limit_zero:
        logger.info(f"limit=0 works correctly: received {len(bars_limit_zero)} bars")
    else:
        logger.error("limit=0 failed: received 0 bars (should have received many)")

    # Validate bars_old (start-only mode, should be >= start_time_old)
    if bars_old:
        first_ts = ns_to_timestamp(bars_old[0].ts_event)
        last_ts = ns_to_timestamp(bars_old[-1].ts_event)
        logger.info(f"bars_old[0]  = {bars_old[0]} (time: {first_ts})")
        logger.info(f"bars_old[-1] = {bars_old[-1]} (time: {last_ts})")
        assert first_ts >= start_time_old, f"First bar {first_ts} should be >= {start_time_old}"
        logger.info(f"bars_old time range valid: {first_ts} to {last_ts}")

    # Validate bars_recent (start-only mode, should be >= start_time_recent)
    if bars_recent:
        first_ts = ns_to_timestamp(bars_recent[0].ts_event)
        last_ts = ns_to_timestamp(bars_recent[-1].ts_event)
        logger.info(f"bars_recent[0]  = {bars_recent[0]} (time: {first_ts})")
        logger.info(f"bars_recent[-1] = {bars_recent[-1]} (time: {last_ts})")
        assert (
            first_ts >= start_time_recent
        ), f"First bar {first_ts} should be >= {start_time_recent}"
        logger.info(f"bars_recent time range valid: {first_ts} to {last_ts}")

    # Validate bars_narrow (should have bars within the 1-hour range)
    if bars_narrow:
        first_ts = ns_to_timestamp(bars_narrow[0].ts_event)
        last_ts = ns_to_timestamp(bars_narrow[-1].ts_event)
        logger.info(f"bars_narrow[0]  = {bars_narrow[0]} (time: {first_ts})")
        logger.info(f"bars_narrow[-1] = {bars_narrow[-1]} (time: {last_ts})")

        # For a 1-hour range with 1-minute bars, we expect around 60 bars
        # Allow some tolerance for missing bars or API limitations
        expected_bars = 60
        tolerance_pct = 0.2  # Allow 20% deviation
        min_expected = int(expected_bars * (1 - tolerance_pct))
        max_expected = int(expected_bars * (1 + tolerance_pct))

        assert (
            len(bars_narrow) >= min_expected
        ), f"Expected at least {min_expected} bars for 1-hour range, was {len(bars_narrow)}"
        assert (
            len(bars_narrow) <= max_expected
        ), f"Expected at most {max_expected} bars for 1-hour range, was {len(bars_narrow)}"

        # Check time bounds with small tolerance
        time_tolerance = pd.Timedelta(minutes=10)  # Allow 10 min tolerance due to API adjustments
        assert (
            first_ts >= narrow_start - time_tolerance
        ), f"First bar {first_ts} should be >= {narrow_start - time_tolerance}"
        assert (
            last_ts <= narrow_end + time_tolerance
        ), f"Last bar {last_ts} should be <= {narrow_end + time_tolerance}"

        logger.info(f"bars_narrow: {len(bars_narrow)} bars in range {first_ts} to {last_ts}")
    else:
        logger.error(f"No bars returned for narrow range {narrow_start} to {narrow_end}")

    # Validate bars_range (should be within range_start and range_end)
    if bars_range:
        first_ts = ns_to_timestamp(bars_range[0].ts_event)
        last_ts = ns_to_timestamp(bars_range[-1].ts_event)
        logger.info(f"bars_range[0]  = {bars_range[0]} (time: {first_ts})")
        logger.info(f"bars_range[-1] = {bars_range[-1]} (time: {last_ts})")
        assert first_ts >= range_start, f"First bar {first_ts} should be >= {range_start}"
        assert last_ts <= range_end, f"Last bar {last_ts} should be <= {range_end}"
        logger.info(f"bars_range time range valid: {first_ts} to {last_ts}")
    else:
        logger.warning(f"No bars returned for range {range_start} to {range_end}")

    # Validate bars_no_start (latest mode, no specific time constraints)
    if bars_no_start:
        first_ts = ns_to_timestamp(bars_no_start[0].ts_event)
        last_ts = ns_to_timestamp(bars_no_start[-1].ts_event)
        logger.info(f"bars_no_start[0]  = {bars_no_start[0]} (time: {first_ts})")
        logger.info(f"bars_no_start[-1] = {bars_no_start[-1]} (time: {last_ts})")
        logger.info(f"bars_no_start (latest mode): {first_ts} to {last_ts}")

    # Summary
    logger.info("\n" + "=" * 60)
    logger.info("TEST SUMMARY:")
    logger.info(f"  Old start (16 days ago):     {len(bars_old)} bars")
    logger.info(f"  Limit=0 test:                 {len(bars_limit_zero)} bars")
    logger.info(f"  Recent start (4 hours ago):   {len(bars_recent)} bars")
    logger.info(f"  Range mode (1 hour window):   {len(bars_range)} bars")
    logger.info(f"  Narrow range (1 hour):        {len(bars_narrow)} bars")
    logger.info(f"  ETH-USDT-SWAP (1 hour):       {len(bars_eth)} bars")
    logger.info(f"  No start (latest mode):       {len(bars_no_start)} bars")
    logger.info("=" * 60 + "\n")
    logger.info("All bar request tests passed")


if __name__ == "__main__":
    asyncio.run(main())
