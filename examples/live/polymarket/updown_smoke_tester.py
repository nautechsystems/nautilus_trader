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
Test Polymarket Up/Down execution with the built-in ExecTester strategy.

WARNING: This example connects to Polymarket and places REAL orders with REAL
funds. It resolves a current Up/Down instrument through the Gamma API, opens a
position with an IOC order, then maintains post-only limit buy quotes. On stop it
cancels all orders and closes all positions. Run only against a funded account
you intend to test. The strategy has no alpha advantage whatsoever and is not
intended for production trading.

"""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.parse
import urllib.request
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket import PolymarketExecutionClientFactory
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.adapters.polymarket import PolymarketUpDownEventSlugConfig
from nautilus_trader.adapters.polymarket import SignatureType
from nautilus_trader.common import Environment
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import ExecTesterConfig


POLYMARKET = "POLYMARKET"
DEFAULT_GAMMA_URL = "https://gamma-api.polymarket.com"
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = "POLYMARKET-001"
STRATEGY_ID = StrategyId.from_str("UPDOWN_SMOKE-001")
ASSETS = ["btc"]
INTERVAL_MINS = 5
PERIODS = 3
START_OFFSET_PERIODS = 0
OUTCOME = "up"
QUANTITY = "5"
SIGNATURE_TYPE = SignatureType.PolyGnosisSafe
BASE_URL_GAMMA = None
HTTP_TIMEOUT_SECS = 10
UPDATE_INSTRUMENTS_INTERVAL_MINS = 1
SUBSCRIBE_NEW_MARKETS = False
TOB_OFFSET_TICKS = 5


def main() -> None:
    instrument_id = resolve_updown_instrument_id(
        assets=ASSETS,
        interval_mins=INTERVAL_MINS,
        periods=PERIODS,
        start_offset_periods=START_OFFSET_PERIODS,
        outcome=OUTCOME,
        base_url_gamma=BASE_URL_GAMMA or DEFAULT_GAMMA_URL,
        timeout_secs=HTTP_TIMEOUT_SECS,
    )
    print(f"Resolved Polymarket {OUTCOME.upper()} instrument: {instrument_id}")

    instrument_config = PolymarketInstrumentProviderConfig(
        event_slug_builder=PolymarketUpDownEventSlugConfig(
            assets=ASSETS,
            interval_mins=INTERVAL_MINS,
            periods=PERIODS,
            start_offset_periods=START_OFFSET_PERIODS,
        ),
    )

    node = (
        LiveNode.builder("POLYMARKET-UPDOWN-SMOKE-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(True)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=instrument_config,
                base_url_gamma=BASE_URL_GAMMA,
                update_instruments_interval_mins=UPDATE_INSTRUMENTS_INTERVAL_MINS,
                subscribe_new_markets=SUBSCRIBE_NEW_MARKETS,
            ),
        )
        .add_exec_client(
            None,
            PolymarketExecutionClientFactory(),
            PolymarketExecClientConfig(
                trader_id=str(TRADER_ID),
                account_id=ACCOUNT_ID,
                signature_type=SIGNATURE_TYPE,
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=instrument_id,
            client_id=ClientId.from_str(POLYMARKET),
            external_order_claims=[instrument_id],
            order_qty=Quantity.from_str(QUANTITY),
            subscribe_quotes=True,
            subscribe_trades=True,
            open_position_on_start_qty=Decimal(QUANTITY),
            open_position_on_first_quote=True,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=True,
            enable_limit_sells=False,  # Outcome token sells require holding the token
            enable_stop_buys=False,
            enable_stop_sells=False,
            tob_offset_ticks=TOB_OFFSET_TICKS,
            use_post_only=True,
            cancel_orders_on_stop=True,
            close_positions_on_stop=True,
            close_positions_qty_precision=2,
            close_positions_time_in_force=TimeInForce.IOC,
            reduce_only_on_stop=False,
            dry_run=False,  # Set True to log intended order flow without submitting orders
            log_data=False,
        ),
    )

    node.run()


def resolve_updown_instrument_id(
    *,
    assets: list[str],
    interval_mins: int,
    periods: int,
    start_offset_periods: int,
    outcome: str,
    base_url_gamma: str,
    timeout_secs: int,
) -> InstrumentId:
    slugs = build_updown_event_slugs(
        assets=assets,
        interval_mins=interval_mins,
        periods=periods,
        start_offset_periods=start_offset_periods,
    )

    for slug in slugs:
        events = request_gamma_events_by_slug(base_url_gamma, slug, timeout_secs)
        instrument_id = find_updown_instrument_id(events, outcome)
        if instrument_id is not None:
            print(f"Resolved Polymarket event slug: {slug}")
            return instrument_id

    slug_text = ", ".join(slugs)
    raise RuntimeError(
        f"Could not resolve a current Polymarket Up/Down instrument from: {slug_text}",
    )


def build_updown_event_slugs(
    *,
    assets: list[str],
    interval_mins: int,
    periods: int,
    start_offset_periods: int,
    unix_secs: int | None = None,
) -> list[str]:
    normalized_assets = []

    for asset in assets:
        normalized_asset = asset.strip().lower()
        if not normalized_asset or normalized_asset in normalized_assets:
            continue

        normalized_assets.append(normalized_asset)

    if not normalized_assets:
        raise ValueError("assets must include at least one non-empty asset")

    period_secs = interval_mins * 60
    now = unix_secs if unix_secs is not None else int(time.time())
    period_start = (now // period_secs) * period_secs
    slugs: list[str] = []

    for period in range(periods):
        timestamp = period_start + (start_offset_periods + period) * period_secs
        if timestamp < 0:
            raise ValueError("start_offset_periods resolves before the Unix epoch")

        slugs.extend(f"{asset}-updown-{interval_mins}m-{timestamp}" for asset in normalized_assets)

    return slugs


def request_gamma_events_by_slug(
    base_url_gamma: str,
    slug: str,
    timeout_secs: int,
) -> list[dict[str, Any]]:
    parsed_base_url = urllib.parse.urlparse(base_url_gamma)
    if parsed_base_url.scheme not in {"http", "https"}:
        raise ValueError("base_url_gamma must use http or https")

    query = urllib.parse.urlencode({"slug": slug})
    url = f"{base_url_gamma.rstrip('/')}/events?{query}"
    request = urllib.request.Request(url, headers={"User-Agent": "nautilus-trader"})  # noqa: S310

    try:
        with urllib.request.urlopen(request, timeout=timeout_secs) as response:  # noqa: S310
            payload = response.read().decode()
    except urllib.error.URLError as e:
        raise RuntimeError(f"Failed to fetch Polymarket event slug '{slug}': {e}") from e

    data = json.loads(payload)
    if not isinstance(data, list):
        raise RuntimeError(f"Gamma returned an unexpected event response for slug '{slug}'")

    return data


def find_updown_instrument_id(
    events: list[dict[str, Any]],
    outcome: str,
) -> InstrumentId | None:
    expected_outcome = outcome.lower()

    for event in events:
        markets = event.get("markets", [])
        if not isinstance(markets, list):
            continue

        for market in markets:
            if not isinstance(market, dict):
                continue

            if not market_is_tradable(market):
                continue

            token_id = token_id_for_outcome(market, expected_outcome)
            condition_id = market.get("conditionId")

            if token_id is None or not isinstance(condition_id, str) or not condition_id:
                continue

            return InstrumentId.from_str(f"{condition_id}-{token_id}.{POLYMARKET}")

    return None


def market_is_tradable(market: dict[str, Any]) -> bool:
    return (
        bool(market.get("active"))
        and not bool(market.get("closed"))
        and bool(market.get("acceptingOrders"))
        and bool(market.get("enableOrderBook"))
    )


def token_id_for_outcome(
    market: dict[str, Any],
    expected_outcome: str,
) -> str | None:
    outcomes = json_array_field(market.get("outcomes"))
    token_ids = json_array_field(market.get("clobTokenIds"))

    if len(outcomes) != len(token_ids):
        return None

    for outcome, token_id in zip(outcomes, token_ids, strict=True):
        if str(outcome).lower() == expected_outcome:
            return str(token_id)

    return None


def json_array_field(value: Any) -> list[Any]:
    if isinstance(value, list):
        return value

    if isinstance(value, str) and value:
        try:
            parsed = json.loads(value)
        except json.JSONDecodeError:
            return []

        return parsed if isinstance(parsed, list) else []

    return []


if __name__ == "__main__":
    main()
