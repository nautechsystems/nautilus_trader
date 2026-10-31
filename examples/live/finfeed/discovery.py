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
Discover a bounded sample, select one supported outcome, and run a data-only node.

Requires `FINFEED_API_KEY`. Discovery permits four managed GET attempts: one active-ID
page of at most three IDs, then at most three full definitions. The node has a separate
four-attempt allowance. Both clients use ten-second pacing and no application retries.
Discovery has a 60-second deadline; the node runs for 60 seconds including bootstrap.
Local attempts are not billing units. No execution client is built.

Run from the repository root:
`uv run --project python --no-sync python examples/live/finfeed/discovery.py`

"""

import asyncio

from nautilus_trader.adapters.finfeed import FINFEED_CLIENT_ID
from nautilus_trader.adapters.finfeed import FinFeedActiveQuery
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedHttpClient
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.adapters.finfeed import FinFeedMarketStatus
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BinaryOption
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


DISCOVERY_LIMIT = 3
DISCOVERY_ATTEMPTS = 4
DISCOVERY_SECONDS = 60
NODE_ATTEMPTS = 4
RUN_SECONDS = 60


async def discover(client: FinFeedHttpClient) -> BinaryOption:
    """
    Return the first open, supported full definition from one bounded active-ID page.
    """
    ids = await client.get_active("POLYMARKET", FinFeedActiveQuery(DISCOVERY_LIMIT, 1))
    for market_id in ids:
        market = await client.get_market("POLYMARKET", market_id)
        if market.status != FinFeedMarketStatus.OPEN:
            continue
        try:
            return market.to_instrument()
        except ValueError:
            continue
    raise RuntimeError("No supported open outcome in the discovery sample")


async def main() -> None:
    """
    Discover an explicit selection, request its definition, and subscribe.
    """
    client = FinFeedHttpClient(
        FinFeedDataClientConfig(
            attempt_budget=DISCOVERY_ATTEMPTS,
            min_request_interval_ms=10_000,
            http_timeout_secs=10,
            max_retries=0,
        ),
    )
    try:
        async with asyncio.timeout(DISCOVERY_SECONDS):
            instrument = await discover(client)
    finally:
        client.cancel()
    config = FinFeedDataClientConfig(
        instruments=[FinFeedInstrumentSelection("POLYMARKET", [str(instrument.raw_symbol)])],
        attempt_budget=NODE_ATTEMPTS,
        min_request_interval_ms=10_000,
        http_timeout_secs=10,
        max_retries=0,
        poll_interval_ms=60_000,
        connect_timeout_secs=45,
    )
    node = (
        LiveNode.builder("FINFEED-DISCOVERY", TraderId("TESTER-001"), Environment.LIVE)
        .with_load_state(False)
        .with_save_state(False)
        .with_reconciliation(False)
        .with_timeout_connection(60)
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
            instrument_ids=[instrument.id],
            request_instruments=True,
            subscribe_quotes=True,
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
        node.dispose()


if __name__ == "__main__":
    asyncio.run(main())
