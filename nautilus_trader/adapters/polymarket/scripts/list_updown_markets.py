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
Script to list active BTC and ETH UpDown markets on Polymarket.

These are short-term prediction markets for BTC and ETH price movements, ideal for
backtesting with high-frequency orderbook and trade data.

"""

import ast
import asyncio

import msgspec

from nautilus_trader.core.nautilus_pyo3 import HttpClient


async def fetch_active_markets(http_client: HttpClient, limit: int = 100) -> list[dict]:
    """
    Fetch active markets from Polymarket.
    """
    params: dict[str, str | int] = {
        "active": "true",
        "closed": "false",
        "archived": "false",
        "limit": limit,
    }
    base_url = "https://gamma-api.polymarket.com/markets"

    resp = await http_client.get(base_url, params=params, timeout_secs=30)

    if resp.status != 200:
        raise RuntimeError(f"HTTP error: {resp.status}")

    return msgspec.json.decode(resp.body)


def filter_updown_markets(markets: list[dict], asset: str | None = None) -> list[dict]:
    """
    Filter markets for UpDown events.

    Parameters
    ----------
    markets : list[dict]
        List of markets to filter.
    asset : str, optional
        Filter by asset (e.g., 'BTC', 'ETH'), by default None (all assets).

    Returns
    -------
    list[dict]
        Filtered list of UpDown markets.

    """
    updown_markets = []

    for market in markets:
        slug = market.get("slug", "").lower()
        question = market.get("question", "").lower()

        # Check if it's an updown market
        if "updown" not in slug and "up/down" not in question:
            continue

        # Filter by asset if specified
        if asset:
            asset_lower = asset.lower()
            if asset_lower not in slug and asset_lower not in question:
                continue

        updown_markets.append(market)

    return updown_markets


def print_market_info(market: dict) -> None:
    """
    Print formatted market information.
    """
    slug = market.get("slug", "N/A")
    question = market.get("question", "N/A")
    active = market.get("active", False)
    condition_id = market.get("conditionId", "N/A")
    clob_token_ids = market.get("clobTokenIds", "[]")

    if isinstance(clob_token_ids, str):
        try:
            clob_token_ids = ast.literal_eval(clob_token_ids)
        except Exception:
            clob_token_ids = []

    if not isinstance(clob_token_ids, list):
        clob_token_ids = []

    token_ids = ", ".join(clob_token_ids) if clob_token_ids else "N/A"

    print(f"Question: {question}")
    print(f"Slug: {slug}")
    print(f"Active: {active}")
    print(f"Condition ID: {condition_id}")
    print(f"Token IDs: {token_ids}")
    print(f"Link: https://polymarket.com/event/{slug}")
    print("-" * 80)


def _print_no_updown_found(markets: list[dict]) -> None:
    """
    Print message when no UpDown markets found with crypto alternatives.
    """
    print(f"{'=' * 80}")
    print("NO UPDOWN MARKETS FOUND")
    print(f"{'=' * 80}")
    print("\nUpDown markets are short-term prediction markets for BTC/ETH price movements.")
    print("There are currently no active UpDown markets on Polymarket.")
    print("\nRelated crypto price prediction markets found:")

    crypto_markets = []
    for market in markets:
        slug = market.get("slug", "").lower()
        question = market.get("question", "").lower()
        if any(term in slug or term in question for term in ["bitcoin", "btc", "ethereum", "eth"]) and any(
            price_term in question for price_term in ["reach", "hit", "dip", "$"]
        ):
            crypto_markets.append(market)

    for market in crypto_markets[:10]:
        question = market.get("question", "N/A")
        slug = market.get("slug", "")
        print(f"\n  • {question}")
        print(f"    https://polymarket.com/event/{slug}")

    if len(crypto_markets) > 10:
        print(f"\n  ... and {len(crypto_markets) - 10} more crypto price markets")


def _print_updown_results(btc_markets: list[dict], eth_markets: list[dict], other_updown: list[dict]) -> None:
    """
    Print UpDown market results.
    """
    print(f"{'=' * 80}")
    print(f"BTC UPDOWN MARKETS ({len(btc_markets)} found)")
    print(f"{'=' * 80}")
    for market in btc_markets:
        print_market_info(market)
    print()

    print(f"{'=' * 80}")
    print(f"ETH UPDOWN MARKETS ({len(eth_markets)} found)")
    print(f"{'=' * 80}")
    for market in eth_markets:
        print_market_info(market)
    print()

    if other_updown:
        print(f"{'=' * 80}")
        print(f"OTHER UPDOWN MARKETS ({len(other_updown)} found)")
        print(f"{'=' * 80}")
        for market in other_updown:
            print_market_info(market)


async def main():
    http_client = HttpClient(timeout_secs=30)

    print("Fetching active Polymarket markets...\n")
    markets = await fetch_active_markets(http_client, limit=200)
    print(f"Found {len(markets)} total active markets\n")

    all_updown = filter_updown_markets(markets)
    btc_markets = filter_updown_markets(markets, asset="BTC")
    eth_markets = filter_updown_markets(markets, asset="ETH")
    other_updown = [m for m in all_updown if m not in btc_markets and m not in eth_markets]

    if not all_updown:
        _print_no_updown_found(markets)
    else:
        _print_updown_results(btc_markets, eth_markets, other_updown)


if __name__ == "__main__":
    asyncio.run(main())
