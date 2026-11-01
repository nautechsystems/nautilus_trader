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
Inspect one explicitly selected OHLCV source without mapping raw rows to instruments.

Requires FINFEED_API_KEY. Each run permits one managed GET attempt, no retries, and a
30-second caller deadline. A five-row sample does not establish complete history. The
flat-file and DB-backed sources can differ; no fallback occurs. Local attempts are not
billing units. Run sources separately, at least ten seconds apart.

Run from the repository root, selecting market, flat-file, or exchange: `uv run
--project python --no-sync python examples/live/finfeed/historical_research.py market`

"""

import argparse
import asyncio

from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedFlatFileOhlcvQuery
from nautilus_trader.adapters.finfeed import FinFeedHttpClient
from nautilus_trader.adapters.finfeed import FinFeedOhlcvQuery


MARKET_ID = "2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949_NO"
START = "2026-10-11T00:00:00Z"
END = "2026-10-12T00:00:00Z"
LIMIT = 5


async def main(source: str) -> None:
    """
    Print raw timestamps, closing prices, and volumes with their selected source.
    """
    client = FinFeedHttpClient(
        FinFeedDataClientConfig(
            attempt_budget=1,
            http_timeout_secs=20,
            max_concurrent_requests=1,
            min_request_interval_ms=10_000,
            max_retries=0,
        ),
    )
    try:
        async with asyncio.timeout(30):
            if source == "flat-file":
                query = FinFeedFlatFileOhlcvQuery(
                    "1HRS",
                    LIMIT,
                    time_start=START,
                    time_end=END,
                )
                rows = await client.get_ohlcv_flat_file_history("POLYMARKET", MARKET_ID, query)
            else:
                query = FinFeedOhlcvQuery(
                    "1HRS",
                    LIMIT,
                    time_start=START,
                    time_end=END,
                )

                if source == "exchange":
                    rows = await client.get_ohlcv_exchange_history("POLYMARKET", query)
                else:
                    rows = await client.get_ohlcv_history("POLYMARKET", MARKET_ID, query)
        print(
            f"Source: {source}; rows: {len(rows)}; remaining attempts: {client.attempts_remaining}",
        )

        for row in rows:
            print(
                row.market_id_exchange,
                row.time_period_start,
                row.time_period_end,
                row.price_close,
                row.volume_traded,
            )
        if len(rows) == LIMIT:
            print("Sample limit reached; more rows may exist, with no verified continuation")
        elif not rows:
            print("Empty sample; source coverage is unverified")
    finally:
        client.cancel()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", choices=("market", "flat-file", "exchange"))
    asyncio.run(main(parser.parse_args().source))
