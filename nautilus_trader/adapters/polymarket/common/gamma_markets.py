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
from typing import Any, Dict, Generator, List, Optional
import requests

DEFAULT_GAMMA_BASE_URL = os.getenv("GAMMA_API_URL", "https://gamma-api.polymarket.com")
from trader.common.logging import get_logger

_logger = get_logger(__name__)

def _normalize_base_url(base_url: Optional[str]) -> str:
    url = base_url or DEFAULT_GAMMA_BASE_URL
    return url[:-1] if url.endswith("/") else url


def build_markets_query(filters: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
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
    params: Dict[str, Any] = {}
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


def _request_markets_page(
    session: requests.Session,
    base_url: str,
    params: Dict[str, Any],
    offset: int,
    limit: int,
    timeout: float,
) -> List[Dict[str, Any]]:
    """
    Fetch a single page of markets using limit/offset pagination.
    Returns a list of market dicts.
    """
    url = f"{base_url}/markets"
    effective_params = dict(params)
    effective_params["limit"] = limit
    effective_params["offset"] = offset

    resp = session.get(url, params=effective_params, timeout=timeout)
    if resp.status_code != 200:
        _logger.error("Gamma Get Markets failed status=%s url=%s params=%s body=%s", resp.status_code, url, effective_params, resp.text)
        raise RuntimeError(f"Gamma Get Markets failed: {resp.status_code} {resp.text}")

    data = resp.json()
    if isinstance(data, list):
        return data
    if isinstance(data, dict) and "data" in data:
        return data.get("data", []) or []

    raise RuntimeError("Unrecognized response schema from Gamma Get Markets")


def iter_markets(
    filters: Optional[Dict[str, Any]] = None,
    base_url: Optional[str] = None,
    timeout: float = 10.0,
) -> Generator[Dict[str, Any], None, None]:
    """
    Iterate markets that pass server-side filters, yielding raw market dicts.
    """
    base = _normalize_base_url(base_url)
    params = build_markets_query(filters)
    limit = int(filters.get("limit", 500)) if filters else 500
    offset = int(filters.get("offset", 0)) if filters else 0

    _logger.info("Fetching Gamma markets with server-side filters limit=%s offset=%s base=%s", limit, offset, base)
    with requests.Session() as session:
        while True:
            _logger.debug("Requesting markets page limit=%s offset=%s", limit, offset)
            markets = _request_markets_page(
                session=session,
                base_url=base,
                params=params,
                offset=offset,
                limit=limit,
                timeout=timeout,
            )
            if not markets:
                _logger.info("No markets returned for offset=%s; stopping", offset)
                break
            for market in markets:
                yield market
            if len(markets) < limit:
                _logger.info("Final page received count=%s (< limit=%s); stopping", len(markets), limit)
                break
            offset += limit
            _logger.debug("Advancing to next page offset=%s", offset)


def normalize_gamma_market_to_clob_format(gamma_market: Dict[str, Any]) -> Dict[str, Any]:
    """
    Normalize Gamma API market format to CLOB API format.

    Gamma API uses camelCase field names, while the CLOB API and parsing code
    expects snake_case field names.

    Parameters
    ----------
    gamma_market : Dict[str, Any]
        Market data from Gamma API in camelCase format.

    Returns
    -------
    Dict[str, Any]
        Market data normalized to CLOB API format with snake_case fields.
    """
    normalized = {
        "condition_id": gamma_market.get("conditionId"),
        "question": gamma_market.get("question"),
        "minimum_tick_size": gamma_market.get("orderPriceMinTickSize", 0.001),
        "minimum_order_size": gamma_market.get("orderMinSize", 5),
        "end_date_iso": gamma_market.get("endDateIso"),
        "maker_base_fee": 0,  # Gamma API doesn't provide fees, use defaults
        "taker_base_fee": 0,  # Gamma API doesn't provide fees, use defaults
        "active": gamma_market.get("active", False),
        "neg_risk": gamma_market.get("negRisk", False),
        "neg_risk_market_id": gamma_market.get("negRiskMarketID"),
        "neg_risk_request_id": gamma_market.get("negRiskRequestID"),
        # Preserve original data for reference
        "_gamma_original": gamma_market,
    }
    return normalized


def list_markets(
    filters: Optional[Dict[str, Any]] = None,
    base_url: Optional[str] = None,
    timeout: float = 10.0,
    max_results: Optional[int] = None,
) -> List[Dict[str, Any]]:
    """
    Collect markets into a list. Use `max_results` to cap total items fetched.
    """
    results: List[Dict[str, Any]] = []
    count = 0
    for market in iter_markets(filters=filters, base_url=base_url, timeout=timeout):
        results.append(market)
        count += 1
        if max_results is not None and len(results) >= max_results:
            break
    _logger.info("Collected %s markets (max_results=%s)", count, max_results)
    return results


