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
Validate bounded, read-only FinFeed data alongside native Polymarket definitions.

Run manually with the built Python extension and FINFEED_API_KEY. The check allows two
raw GETs plus eight FinFeed managed attempts, spaced ten seconds apart with no retries.
Native Polymarket loads one explicit condition without subscriptions or periodic
refresh. No execution client is built.

"""

import asyncio
import json
import logging
import os
import time
from datetime import UTC
from datetime import datetime
from decimal import ROUND_HALF_EVEN
from decimal import Decimal
from typing import Any
from urllib.parse import urlencode
from urllib.request import HTTPRedirectHandler
from urllib.request import ProxyHandler
from urllib.request import Request
from urllib.request import build_opener

from pandas import Timestamp

from nautilus_trader.adapters.finfeed import FINFEED_CLIENT_ID
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.config import DataActorConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import NodeState
from nautilus_trader.model import ActorId
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


MARKET_ID = "2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949_NO"
INSTRUMENT_ID = InstrumentId.from_str(f"{MARKET_ID}.FINFEED_POLYMARKET")
BAR_TYPE = BarType.from_str(f"{INSTRUMENT_ID}-1-DAY-LAST-EXTERNAL")
HISTORY_START = datetime(2026, 9, 1, tzinfo=UTC)
HISTORY_END = datetime(2026, 10, 20, tzinfo=UTC)
HISTORY_LIMIT = 3


async def main() -> None:
    """
    Validate provider coexistence, exact prices, bounded bar volume, and clean shutdown.
    """
    key = os.environ["FINFEED_API_KEY"]
    metadata = read_response(f"markets/POLYMARKET/{MARKET_ID}", key)
    assert metadata["market_id"] == MARKET_ID
    assert metadata["exchange_id"] == "POLYMARKET"
    assert metadata["outcome_name"] == "No"
    source = metadata["source_specific_data"]
    tokens = json.loads(source["Tokens"], parse_float=Decimal)
    token = next(token for token in tokens if token["outcome"] == "No")
    native_id = InstrumentId.from_str(f"{source['ConditionId']}-{token['token_id']}.POLYMARKET")
    await asyncio.sleep(10)
    query = urlencode(
        {
            "period_id": "1DAY",
            "time_start": HISTORY_START.isoformat(),
            "time_end": HISTORY_END.isoformat(),
            "limit": 100_000,
        },
    )
    history = read_response(f"ohlcv/POLYMARKET/{MARKET_ID}/history?{query}", key)
    await asyncio.sleep(10)

    config = FinFeedDataClientConfig(
        api_key=key,
        instruments=[FinFeedInstrumentSelection("POLYMARKET", [MARKET_ID])],
        attempt_budget=8,
        min_request_interval_ms=10_000,
        poll_interval_ms=60_000,
        poll_limit=100,
        max_retries=0,
        connect_timeout_secs=60,
    )
    native = PolymarketDataClientConfig(
        instrument_config=PolymarketInstrumentProviderConfig(load_ids=[native_id]),
        update_instruments_interval_mins=0,
        subscribe_new_markets=False,
        auto_load_missing_instruments=False,
        auto_load_max_retries=0,
        resolve_poll_enabled=False,
        http_timeout_secs=10,
    )
    node = (
        LiveNode.builder("FINFEED-LIVE-ACCEPTANCE", TraderId("TESTER-001"), Environment.LIVE)
        .with_reconciliation(False)
        .with_timeout_connection(90)
        .with_timeout_portfolio(0)
        .with_delay_post_stop_secs(0)
        .with_delay_shutdown_secs(0)
        .add_data_client(None, FinFeedDataClientFactory(), config)
        .add_data_client(None, PolymarketDataClientFactory(), native)
        .build()
    )
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            actor_id=ActorId("FinFeed-Live-DataTester"),
            client_id=FINFEED_CLIENT_ID,
            instrument_ids=[INSTRUMENT_ID],
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_book_deltas=True,
            stats_interval_secs=0,
            log_data=True,
        ),
    )
    observer = HistoryObserver()
    node.add_actor(observer)
    cache = node.cache
    handle = node.handle()
    try:
        task = asyncio.create_task(node.run_async())
        try:
            async with asyncio.timeout(75):
                while (
                    cache.instrument(native_id) is None
                    or cache.quote_count(INSTRUMENT_ID) == 0
                    or cache.trade_count(INSTRUMENT_ID) == 0
                    or cache.order_book(INSTRUMENT_ID) is None
                    or observer.bars is None
                ):
                    await asyncio.sleep(0.01)
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=20)

        quotes = cache.quotes(INSTRUMENT_ID)
        trades = cache.trades(INSTRUMENT_ID)
        book = cache.order_book(INSTRUMENT_ID)
        instrument = cache.instrument(INSTRUMENT_ID)
        native_instrument = cache.instrument(native_id)
        assert handle.state == NodeState.STOPPED
        assert instrument.id == INSTRUMENT_ID
        assert native_instrument.id == native_id
        assert instrument.id != native_instrument.id
        assert str(instrument.raw_symbol) == MARKET_ID
        assert str(native_instrument.raw_symbol) == str(token["token_id"])
        assert instrument.price_precision == 4
        assert instrument.size_precision == 6
        assert Decimal(str(instrument.price_increment)) == Decimal(source["MinimumTickSize"])
        for record in history:
            source_volume = Decimal(str(record["volume_traded"]))
            normalized = source_volume.quantize(Decimal("0.000001"), rounding=ROUND_HALF_EVEN)
            if normalized != source_volume:
                assert Decimal(0) < source_volume < Decimal(500_000_000)
                assert abs(source_volume - normalized) <= source_volume * Decimal("1e-15")
        expected = [
            record
            for record in history
            if record["price_open"] is not None
            and Timestamp(record["time_period_start"]) >= HISTORY_START
            and Timestamp(record["time_period_end"]) <= HISTORY_END
        ]
        expected.sort(key=lambda record: Timestamp(record["time_period_end"]).value)
        expected = expected[:HISTORY_LIMIT]
        assert len(history) < 100_000
        assert expected
        assert len(observer.bars) == len(expected)
        for bar, record in zip(observer.bars, expected, strict=True):
            assert bar.bar_type == BAR_TYPE
            assert Decimal(str(bar.open)) == Decimal(str(record["price_open"]))
            assert Decimal(str(bar.high)) == Decimal(str(record["price_high"]))
            assert Decimal(str(bar.low)) == Decimal(str(record["price_low"]))
            assert Decimal(str(bar.close)) == Decimal(str(record["price_close"]))
            assert Decimal(str(bar.volume)) == Decimal(str(record["volume_traded"])).quantize(
                Decimal("0.000001"),
                rounding=ROUND_HALF_EVEN,
            )
            assert bar.ts_event == Timestamp(record["time_period_end"]).value
            assert bar.ts_init == bar.ts_event
        now = time.time_ns()

        for quote in quotes:
            assert quote.instrument_id == INSTRUMENT_ID
            assert quote.bid_price.precision == quote.ask_price.precision == 4
            assert quote.bid_size.precision == quote.ask_size.precision == 6
            assert Decimal(0) <= Decimal(str(quote.bid_price)) <= Decimal(1)
            assert Decimal(0) <= Decimal(str(quote.ask_price)) <= Decimal(1)
            assert 10**16 <= quote.ts_event <= now
            assert 10**16 <= quote.ts_init <= now
        for trade in trades:
            assert trade.instrument_id == INSTRUMENT_ID
            assert trade.aggressor_side == AggressorSide.NO_AGGRESSOR
            assert trade.price.precision == 4
            assert trade.size.precision == 6
            assert trade.ts_init == trade.ts_event
            assert 10**16 <= trade.ts_event <= now
        bids = book.bids_to_dict()
        asks = book.asks_to_dict()
        assert bids
        assert asks
        assert max(bids) <= min(asks)
        assert all(
            Decimal(0) <= price <= Decimal(1) and size > 0
            for price, size in [*bids.items(), *asks.items()]
        )
        await asyncio.sleep(0.35)
        assert cache.quotes(INSTRUMENT_ID) == quotes
        assert cache.trades(INSTRUMENT_ID) == trades
        assert cache.order_book(INSTRUMENT_ID).bids_to_dict() == bids
        assert cache.order_book(INSTRUMENT_ID).asks_to_dict() == asks
        logging.getLogger(__name__).info(
            json.dumps(
                {
                    "result": "passed",
                    "finfeed_managed_attempt_ceiling": 8,
                    "additional_raw_gets": 2,
                    "historical_bars_compared": len(observer.bars),
                    "native_instruments_distinct": True,
                    "quotes": len(quotes),
                    "trades": len(trades),
                    "bids": len(bids),
                    "asks": len(asks),
                    "shutdown": "stopped_cache_stable",
                },
            ),
        )
    finally:
        node.dispose()


def read_response(path: str, key: str) -> Any:
    """
    Read fixed-origin HTTPS with exact decimals, no redirects, and no ambient proxies.
    """
    request = Request(
        f"https://api.prediction-markets.finfeedapi.com/v1/{path}",
        headers={"Authorization": key, "X-Referrer": "nautilus", "Accept": "application/json"},
    )
    with build_opener(ProxyHandler({}), RejectRedirects()).open(request, timeout=30) as response:
        return json.load(response, parse_float=Decimal)


class HistoryObserver(DataActor):
    """
    Receive the bounded historical bar batch for comparison with raw provider fields.
    """

    def __init__(self) -> None:
        """
        Initialize a distinct actor identity and its response slot.
        """
        super().__init__(DataActorConfig(actor_id=ActorId("FinFeed-History")))
        self.bars = None

    def on_start(self) -> None:
        """
        Request the configured bounded historical window.
        """
        self.request_bars(
            BAR_TYPE,
            start=HISTORY_START,
            end=HISTORY_END,
            limit=HISTORY_LIMIT,
            client_id=FINFEED_CLIENT_ID,
        )

    def on_historical_bars(self, bars) -> None:
        """
        Retain the complete batch for exact field comparisons.
        """
        self.bars = list(bars)


class RejectRedirects(HTTPRedirectHandler):
    """
    Preserve the adapter's no-redirect credential boundary for both raw REST GETs.
    """

    def redirect_request(self, *_args: object, **_kwargs: object) -> None:
        """
        Reject redirection instead of forwarding credentials.
        """
        return


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    asyncio.run(main())
