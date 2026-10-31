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
Replay one bounded historical L2 book window into a Nautilus OrderBook.

Requires FINFEED_API_KEY and a returned snapshot at or before START in the same UTC
hour. Three managed GET attempts cover bootstrap and one history hour, with ten-second
pacing and no retries. The 150-second deadline includes bootstrap. No subscriptions or
execution client are created. Returned-event replay does not prove source completeness.

Run from the repository root:
`uv run --project python --no-sync python examples/live/finfeed/historical_book.py`

"""

import asyncio
from collections.abc import Sequence
from datetime import UTC
from datetime import datetime

from nautilus_trader.adapters.finfeed import FINFEED_CLIENT_ID
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import TraderId


MARKET_ID = (
    "2-FREE-APP-IN-THE-US-APPLE-APP-STORE-ON-OCTOBER-9-20261002T180000000Z-"
    "WILL-VINTED-PRE-LOVED-MARKETPL_NO"
)
INSTRUMENT_ID = InstrumentId.from_str(f"{MARKET_ID}.FINFEED_POLYMARKET")
START = datetime(2026, 10, 8, 0, 0, 5, tzinfo=UTC)
END = datetime(2026, 10, 8, 0, 2, tzinfo=UTC)
RUN_SECONDS = 150


class HistoricalBook(DataActor):
    """
    Apply returned deltas in order and check each completed batch.
    """

    def __init__(self) -> None:
        """
        Retain the reconstructed book and response completion signal.
        """
        super().__init__()
        self.book = OrderBook(INSTRUMENT_ID, BookType.L2_MBP)
        self.batches = 0
        self.received = asyncio.Event()

    def on_start(self) -> None:
        """
        Request the fixed historical window after definitions load.
        """
        self.request_book_deltas(
            INSTRUMENT_ID,
            start=START,
            end=END,
            client_id=FINFEED_CLIENT_ID,
        )

    def on_historical_book_deltas(self, deltas: Sequence[OrderBookDelta]) -> None:
        """
        Replay returned records and signal a complete response.
        """
        for delta in deltas:
            self.book.apply_delta(delta)
            if RecordFlag.F_LAST.matches(delta.flags):
                self.book.check_integrity()
                self.batches += 1
        self.received.set()


async def main() -> None:
    """
    Request the historical window, print the final supplied book, and dispose the node.
    """
    config = FinFeedDataClientConfig(
        instruments=[FinFeedInstrumentSelection("POLYMARKET", [MARKET_ID])],
        attempt_budget=3,
        http_timeout_secs=60,
        retry_operation_timeout_ms=75_000,
        retry_max_elapsed_ms=90_000,
        min_request_interval_ms=10_000,
        max_concurrent_requests=1,
        max_retries=0,
        connect_timeout_secs=45,
    )
    node = (
        LiveNode.builder("FINFEED-HISTORICAL-BOOK", TraderId("TESTER-001"), Environment.LIVE)
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
    actor = HistoricalBook()
    node.add_actor(actor)
    handle = node.handle()
    task = asyncio.create_task(node.run_async())
    try:
        await asyncio.wait_for(actor.received.wait(), timeout=RUN_SECONDS)
        if actor.batches == 0:
            raise RuntimeError("No completed historical book batch received")
        print(f"Completed batches: {actor.batches}")
        print(f"Bids: {actor.book.bids_to_dict()}")
        print(f"Asks: {actor.book.asks_to_dict()}")
    finally:
        handle.stop()
        try:
            await task
        finally:
            node.dispose()


if __name__ == "__main__":
    asyncio.run(main())
