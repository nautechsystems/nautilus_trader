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
Polymarket Python v2 Up/Down smoke tester example.

The default path builds a live node configured with the Rust-backed Up/Down event slug
builder without connecting to Polymarket or submitting orders. Pass --run to connect in
dry-run mode. Pass --live-orders only when the account is funded and you intend to test
live order flow.

"""

from __future__ import annotations

import argparse
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
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig
from nautilus_trader.testkit import ExecTesterConfig


POLYMARKET = "POLYMARKET"
DEFAULT_GAMMA_URL = "https://gamma-api.polymarket.com"


def main() -> None:
    args = parse_args()
    trader_id = TraderId.from_str(args.trader_id)
    instrument_id = resolve_requested_instrument_id(args)
    instrument_config = PolymarketInstrumentProviderConfig(
        event_slug_builder=PolymarketUpDownEventSlugConfig(
            assets=args.assets,
            interval_mins=args.interval_mins,
            periods=args.periods,
            start_offset_periods=args.start_offset_periods,
        ),
    )

    builder = (
        LiveNode.builder("POLYMARKET-UPDOWN-SMOKE-001", trader_id, Environment.LIVE)
        .with_reconciliation(args.run and not args.data_only)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=instrument_config,
                base_url_gamma=args.base_url_gamma,
                update_instruments_interval_mins=args.update_instruments_interval_mins,
                subscribe_new_markets=args.subscribe_new_markets,
            ),
        )
    )

    if not args.data_only and not args.dry_run:
        builder = builder.add_exec_client(
            None,
            PolymarketExecutionClientFactory(),
            PolymarketExecClientConfig(
                trader_id=args.trader_id,
                account_id=args.account_id,
                private_key=None if args.run else args.private_key,
                api_key=None if args.run else args.api_key,
                api_secret=None if args.run else args.api_secret,
                passphrase=None if args.run else args.passphrase,
                funder=None if args.run else args.funder,
                signature_type=signature_type_from_name(args.signature_type),
            ),
        )

    node = builder.build()
    live_orders = not args.dry_run

    if instrument_id is not None and args.data_only:
        node.add_native_actor(
            "DataTester",
            DataTesterConfig(
                client_id=ClientId.from_str(POLYMARKET),
                instrument_ids=[instrument_id],
                subscribe_trades=True,
                subscribe_quotes=True,
                manage_book=True,
                log_data=args.log_data,
            ),
        )
    elif instrument_id is not None:
        node.add_native_strategy(
            "ExecTester",
            ExecTesterConfig(
                strategy_id=StrategyId.from_str("UPDOWN_SMOKE-001"),
                instrument_id=instrument_id,
                client_id=ClientId.from_str(POLYMARKET),
                external_order_claims=[instrument_id],
                order_qty=Quantity.from_str(args.quantity),
                subscribe_quotes=True,
                subscribe_trades=True,
                open_position_on_start_qty=Decimal(args.quantity) if live_orders else None,
                open_position_on_first_quote=live_orders,
                open_position_time_in_force=TimeInForce.IOC,
                enable_limit_buys=live_orders,
                enable_limit_sells=live_orders and args.limit_sells,
                enable_stop_buys=False,
                enable_stop_sells=False,
                tob_offset_ticks=args.tob_offset_ticks,
                use_post_only=True,
                cancel_orders_on_stop=live_orders,
                close_positions_on_stop=live_orders,
                reduce_only_on_stop=False,
                dry_run=args.dry_run,
                log_data=args.log_data,
            ),
        )

    if args.run:
        node.run()
    elif instrument_id is None:
        print(
            "Built Polymarket Up/Down smoke node without an instrument. "
            "Pass --run to resolve a current Up/Down instrument, or pass --instrument.",
        )
    else:
        print("Built Polymarket Up/Down smoke node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build or run the Polymarket Python v2 Up/Down smoke tester.",
    )
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--account-id", default="POLYMARKET-001")
    parser.add_argument("--assets", nargs="+", default=["btc"])
    parser.add_argument("--interval-mins", type=positive_int, default=5)
    parser.add_argument("--periods", type=positive_int, default=3)
    parser.add_argument("--start-offset-periods", type=int, default=0)
    parser.add_argument("--update-instruments-interval-mins", type=positive_int, default=1)
    parser.add_argument("--instrument", default=None)
    parser.add_argument("--outcome", choices=["up", "down"], default="up")
    parser.add_argument("--quantity", default="5")
    parser.add_argument("--private-key", default="0x00")
    parser.add_argument("--api-key", default="test_key")
    parser.add_argument("--api-secret", default="test_secret")
    parser.add_argument("--passphrase", default="test_passphrase")
    parser.add_argument("--funder", default="0x0000000000000000000000000000000000000000")
    parser.add_argument(
        "--signature-type",
        choices=["eoa", "poly-proxy", "poly-gnosis-safe", "poly-1271"],
        default="poly-gnosis-safe",
    )
    parser.add_argument("--base-url-gamma", default=None)
    parser.add_argument("--http-timeout-secs", type=positive_int, default=10)
    parser.add_argument("--tob-offset-ticks", type=positive_int, default=5)
    parser.add_argument("--run", action="store_true")
    order_mode = parser.add_mutually_exclusive_group()
    order_mode.add_argument("--dry-run", dest="dry_run", action="store_true")
    order_mode.add_argument("--live-orders", dest="dry_run", action="store_false")
    parser.add_argument("--limit-sells", action="store_true")
    parser.add_argument("--data-only", action="store_true")
    parser.add_argument("--no-resolve-instrument", dest="resolve_instrument", action="store_false")
    parser.add_argument("--subscribe-new-markets", action="store_true")
    parser.add_argument("--log-data", action="store_true")
    parser.set_defaults(dry_run=True, resolve_instrument=True)
    return parser.parse_args()


def resolve_requested_instrument_id(args: argparse.Namespace) -> InstrumentId | None:
    if args.instrument is not None:
        return InstrumentId.from_str(args.instrument)

    if not args.run or not args.resolve_instrument:
        return None

    instrument_id = resolve_updown_instrument_id(
        assets=args.assets,
        interval_mins=args.interval_mins,
        periods=args.periods,
        start_offset_periods=args.start_offset_periods,
        outcome=args.outcome,
        base_url_gamma=args.base_url_gamma or DEFAULT_GAMMA_URL,
        timeout_secs=args.http_timeout_secs,
    )
    print(f"Resolved Polymarket {args.outcome.upper()} instrument: {instrument_id}")
    return instrument_id


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
    slugs = []

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
    except urllib.error.URLError as exc:
        raise RuntimeError(f"Failed to fetch Polymarket event slug '{slug}': {exc}") from exc

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


def signature_type_from_name(name: str) -> SignatureType:
    if name == "eoa":
        return SignatureType.Eoa
    if name == "poly-proxy":
        return SignatureType.PolyProxy
    if name == "poly-1271":
        return SignatureType.Poly1271

    return SignatureType.PolyGnosisSafe


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than 0")

    return parsed


if __name__ == "__main__":
    main()
