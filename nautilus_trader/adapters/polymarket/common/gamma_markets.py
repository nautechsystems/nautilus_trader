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
Thin Gamma Markets API client utilities for Polymarket.

Provides functions to fetch markets using server-side filters, returning
raw market dictionaries ready for further client-side filtering.

References
----------
- Gamma Get Markets docs: https://docs.polymarket.com/developers/gamma-markets-api/get-markets

"""

from __future__ import annotations

import os
from collections.abc import AsyncGenerator
from math import ceil
from typing import Any

import msgspec

from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpResponse


DEFAULT_GAMMA_BASE_URL = os.getenv("GAMMA_API_URL", "https://gamma-api.polymarket.com")


def _normalize_base_url(base_url: str | None) -> str:
    url = base_url or DEFAULT_GAMMA_BASE_URL
    return url.removesuffix("/")


def build_markets_query(filters: dict[str, Any] | None = None) -> dict[str, Any]:
    """
    Build query params for Gamma Get Markets from a generic filter dict.

    Supported keys (passed through if present):
    - active, archived, closed, limit, offset, order, ascending, id, slug,
      clob_token_ids, condition_ids,
      liquidity_num_min, liquidity_num_max,
      volume_num_min, volume_num_max,
      start_date_min, start_date_max,
      end_date_min, end_date_max,
      tag_id, related_tags

    Special handling:
    - is_active=True implies active=true, archived=false, closed=false
    - next_cursor: will be added separately by the fetch function

    """
    params: dict[str, Any] = {}
    if not filters:
        return params

    if filters.get("is_active") is True:
        params["active"] = "true"
        params["archived"] = "false"
        params["closed"] = "false"

    passthrough_keys = (
        "active",
        "archived",
        "closed",
        "limit",
        "offset",
        "order",
        "ascending",
        "id",
        "slug",
        "clob_token_ids",
        "condition_ids",
        "liquidity_num_min",
        "liquidity_num_max",
        "volume_num_min",
        "volume_num_max",
        "start_date_min",
        "start_date_max",
        "end_date_min",
        "end_date_max",
        "tag_id",
        "related_tags",
    )
    for key in passthrough_keys:
        if key in filters and filters[key] is not None:
            params[key] = filters[key]

    return params


async def _request_markets_page(
    http_client: HttpClient,
    base_url: str,
    params: dict[str, Any],
    offset: int,
    limit: int,
    timeout: float,
) -> list[dict[str, Any]]:
    """
    Fetch a single page of markets using limit/offset pagination.

    Returns a list of market dicts.

    """
    base_endpoint = f"{base_url}/markets"
    effective_params = dict(params)
    effective_params["limit"] = limit
    effective_params["offset"] = offset

    resp: HttpResponse = await http_client.get(
        base_endpoint,
        params=effective_params,
        timeout_secs=max(1, ceil(timeout)),
    )
    if resp.status != 200:
        body = resp.body.decode("utf-8", errors="replace")
        raise RuntimeError(f"Gamma Get Markets failed: {resp.status} for url {base_endpoint} with params {effective_params} and body {body}")

    data = msgspec.json.decode(resp.body)
    if isinstance(data, list):
        return data
    if isinstance(data, dict) and "data" in data:
        return data.get("data", []) or []

    raise RuntimeError("Unrecognized response schema from Gamma Get Markets")


async def iter_markets(
    http_client: HttpClient,
    filters: dict[str, Any] | None = None,
    base_url: str | None = None,
    timeout: float = 10.0,
) -> AsyncGenerator[dict[str, Any]]:
    """
    Iterate markets that pass server-side filters, yielding raw market dicts.
    """
    base = _normalize_base_url(base_url)
    params = build_markets_query(filters)
    limit = int(filters.get("limit", 500)) if filters else 500
    offset = int(filters.get("offset", 0)) if filters else 0

    while True:
        markets = await _request_markets_page(
            http_client=http_client,
            base_url=base,
            params=params,
            offset=offset,
            limit=limit,
            timeout=timeout,
        )
        if not markets:
            break
        for market in markets:
            yield market
        if len(markets) < limit:
            break
        offset += limit


def normalize_gamma_market_to_clob_format(gamma_market: dict[str, Any]) -> dict[str, Any]:
    """
    Normalize Gamma API market format to CLOB API format.

    Gamma API uses camelCase field names, while the CLOB API and parsing code
    expects snake_case field names.

    Parameters
    ----------
    gamma_market : dict[str, Any]
        Market data from Gamma API in camelCase format.

    Returns
    -------
    dict[str, Any]
        Market data normalized to CLOB API format with snake_case fields.

    """
    rewards = gamma_market.get("clobRewards", [])
    rewards_dict = None
    if rewards and len(rewards) > 0:
        reward = rewards[0]
        rewards_dict = {
            "rates": reward.get("rewardsDailyRate"),
            "min_size": gamma_market.get("rewardsMinSize"),
            "max_spread": gamma_market.get("rewardsMaxSpread"),
        }

    tokens = []
    clob_token_ids = gamma_market.get("clobTokenIds", [])
    outcomes = gamma_market.get("outcomes", [])
    outcome_prices = gamma_market.get("outcomePrices", [])

    if isinstance(clob_token_ids, str):
        clob_token_ids = msgspec.json.decode(clob_token_ids)
    if isinstance(outcomes, str):
        outcomes = msgspec.json.decode(outcomes)
    if isinstance(outcome_prices, str):
        outcome_prices = msgspec.json.decode(outcome_prices)

    for i, (token_id, outcome) in enumerate(zip(clob_token_ids, outcomes, strict=False)):
        token_entry = {
            "token_id": token_id,
            "outcome": outcome,
            "price": float(outcome_prices[i]) if i < len(outcome_prices) else 0.5,
            "winner": False,
        }
        tokens.append(token_entry)

    normalized = {
        # Core identifiers
        "condition_id": gamma_market.get("conditionId"),
        "question_id": gamma_market.get("questionID"),
        "question": gamma_market.get("question"),
        "description": gamma_market.get("description"),
        "market_slug": gamma_market.get("slug"),
        # Order book and trading settings
        "enable_order_book": gamma_market.get("enableOrderBook", True),
        "minimum_tick_size": gamma_market.get("orderPriceMinTickSize", 0.001),
        "minimum_order_size": gamma_market.get("orderMinSize", 5),
        "accepting_orders": gamma_market.get("acceptingOrders", True),
        "accepting_order_timestamp": gamma_market.get("acceptingOrdersTimestamp"),
        "seconds_delay": gamma_market.get("secondsDelay", 0),
        # Market status flags
        "active": gamma_market.get("active", False),
        "closed": gamma_market.get("closed", False),
        "archived": gamma_market.get("archived", False),
        # Dates
        "end_date_iso": gamma_market.get("endDateIso"),
        "game_start_time": gamma_market.get("startDateIso"),
        # Fee structure
        "maker_base_fee": 0,
        "taker_base_fee": 0,
        "fpmm": gamma_market.get("marketMakerAddress", ""),
        # Negative risk settings
        "neg_risk": gamma_market.get("negRisk", False),
        "neg_risk_market_id": gamma_market.get("negRiskMarketID"),
        "neg_risk_request_id": gamma_market.get("negRiskRequestID"),
        # Media
        "icon": gamma_market.get("icon"),
        "image": gamma_market.get("image"),
        # Rewards and notifications
        "rewards": rewards_dict,
        "notifications_enabled": True,
        # Tokens array (CLOB API format)
        "tokens": tokens,
        # Preserve original data for reference
        "_gamma_original": gamma_market,
    }
    return normalized


async def list_markets(
    http_client: HttpClient,
    filters: dict[str, Any] | None = None,
    base_url: str | None = None,
    timeout: float = 10.0,
    max_results: int | None = None,
) -> list[dict[str, Any]]:
    """
    Collect markets into a list.

    Use `max_results` to cap total items fetched.

    """
    results: list[dict[str, Any]] = []
    async for market in iter_markets(http_client=http_client, filters=filters, base_url=base_url, timeout=timeout):
        results.append(market)
        if max_results is not None and len(results) >= max_results:
            break
    return results
