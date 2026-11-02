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
Poll one Polymarket outcome through FinFeed for 60 seconds, then stop the node.

Requires `FINFEED_API_KEY` with prediction-market access. Uses at most eight managed GET
attempts with ten-second pacing and no application retries. Local attempts are not billing units.
Quotes and trades share a poll; L2 book deltas replace the whole book at each accepted snapshot.
For longer runs or more outcomes, size the finite allowance and both connection deadlines
using `docs/integrations/finfeed.md`; this example keeps its bounded one-outcome defaults.

Run from the repository root:
`uv run --project python --no-sync python examples/live/finfeed/data_tester.py`

"""

from __future__ import annotations

import asyncio

from nautilus_trader.adapters.finfeed import FINFEED_CLIENT_ID
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


MARKET_ID = "2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949_NO"
INSTRUMENT_ID = InstrumentId.from_str(f"{MARKET_ID}.FINFEED_POLYMARKET")
RUN_SECONDS = 60
ATTEMPT_BUDGET = 8
HTTP_TIMEOUT_SECS = 10
MAX_CONCURRENT_REQUESTS = 1
MIN_REQUEST_INTERVAL_MS = 10_000
MAX_RETRIES = 0
RETRY_OPERATION_TIMEOUT_MS = 30_000
RETRY_MAX_ELAPSED_MS = 120_000
POLL_INTERVAL_MS = 60_000
POLL_LIMIT = 100
CONNECT_TIMEOUT_SECS = 45
NODE_CONNECTION_TIMEOUT_SECS = 60


async def main() -> None:
    """
    Run the bounded data example.
    """
    config = FinFeedDataClientConfig(
        instruments=[FinFeedInstrumentSelection("POLYMARKET", [MARKET_ID])],
        attempt_budget=ATTEMPT_BUDGET,
        http_timeout_secs=HTTP_TIMEOUT_SECS,
        max_concurrent_requests=MAX_CONCURRENT_REQUESTS,
        min_request_interval_ms=MIN_REQUEST_INTERVAL_MS,
        max_retries=MAX_RETRIES,
        retry_operation_timeout_ms=RETRY_OPERATION_TIMEOUT_MS,
        retry_max_elapsed_ms=RETRY_MAX_ELAPSED_MS,
        poll_interval_ms=POLL_INTERVAL_MS,
        poll_limit=POLL_LIMIT,
        connect_timeout_secs=CONNECT_TIMEOUT_SECS,
    )
    node = (
        LiveNode.builder("FINFEED-DATA-TESTER", TraderId("TESTER-001"), Environment.LIVE)
        .with_load_state(False)
        .with_save_state(False)
        .with_reconciliation(False)
        .with_timeout_connection(NODE_CONNECTION_TIMEOUT_SECS)
        .with_timeout_portfolio(0)
        .with_delay_post_stop_secs(0)
        .with_delay_shutdown_secs(0)
        .add_data_client(None, FinFeedDataClientFactory(), config)
        .build()
    )
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=FINFEED_CLIENT_ID,
            instrument_ids=[INSTRUMENT_ID],
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_book_deltas=True,
            stats_interval_secs=0,
            log_data=True,
        ),
    )
    handle = node.handle()
    task = asyncio.create_task(node.run_async())
    try:
        await asyncio.wait_for(asyncio.shield(task), timeout=RUN_SECONDS)
    except TimeoutError:
        pass
    finally:
        handle.stop()
        await task


if __name__ == "__main__":
    asyncio.run(main())
