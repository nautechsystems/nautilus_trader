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
Provides data loaders for historical Polymarket data from various APIs.
"""

from __future__ import annotations

from typing import Any

import msgspec
import pandas as pd

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_HTTP_RATE_LIMIT
from nautilus_trader.adapters.polymarket.common.gamma_markets import fetch_fee_schedules
from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments import BinaryOption


class PolymarketDataLoader:
    """
    Provides a data loader for historical Polymarket market data.

    This loader fetches data from:
    - Polymarket Gamma API (market and event information)
    - Polymarket CLOB API (market details)
    - Polymarket Data API (historical trades)

    If no `http_client` is provided, the loader creates one with a default rate limit
    of 100 requests per minute, matching Polymarket's public endpoint limit.

    Parameters
    ----------
    instrument : BinaryOption
        The binary option instrument to load data for.
    token_id : str, optional
        The Polymarket token ID for this instrument.
    condition_id : str, optional
        The Polymarket condition ID for this instrument's market.
    http_client : nautilus_pyo3.HttpClient, optional
        The HTTP client to use for requests. If not provided, a new client will be created.

    """

    def __init__(
        self,
        instrument: BinaryOption,
        token_id: str | None = None,
        condition_id: str | None = None,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> None:
        self._instrument = instrument
        self._token_id = token_id
        self._condition_id = condition_id
        self._http_client = http_client or self._create_http_client()

    @staticmethod
    def _create_http_client() -> nautilus_pyo3.HttpClient:
        return nautilus_pyo3.HttpClient(
            default_quota=nautilus_pyo3.Quota.rate_per_minute(POLYMARKET_HTTP_RATE_LIMIT),
        )

    @staticmethod
    async def _fetch_market_by_slug(
        slug: str,
        http_client: nautilus_pyo3.HttpClient,
    ) -> dict[str, Any]:
        PyCondition.valid_string(slug, "slug")

        response = await http_client.get(
            url=f"https://gamma-api.polymarket.com/markets/slug/{slug}",
        )

        if response.status == 404:
            raise ValueError(f"Market with slug '{slug}' not found")

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        data = msgspec.json.decode(response.body)

        if isinstance(data, list):
            if not data:
                raise ValueError(f"Market with slug '{slug}' not found")
            market = data[0]
        else:
            market = data

        if not isinstance(market, dict):
            raise RuntimeError(
                f"Unexpected response type for slug '{slug}': {type(market).__name__}",
            )

        return market

    @staticmethod
    async def _fetch_market_details(
        condition_id: str,
        http_client: nautilus_pyo3.HttpClient,
    ) -> dict[str, Any]:
        PyCondition.valid_string(condition_id, "condition_id")

        response = await http_client.get(
            url=f"https://clob.polymarket.com/markets/{condition_id}",
        )

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        return msgspec.json.decode(response.body)

    @staticmethod
    async def _fetch_event_by_slug(
        slug: str,
        http_client: nautilus_pyo3.HttpClient,
    ) -> dict[str, Any]:
        PyCondition.valid_string(slug, "slug")

        response = await http_client.get(
            url="https://gamma-api.polymarket.com/events",
            params={"slug": slug},
        )

        if response.status == 404:
            raise ValueError(f"Event with slug '{slug}' not found")

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        events = msgspec.json.decode(response.body)

        if not events:
            raise ValueError(f"Event with slug '{slug}' not found")

        return events[0]

    @classmethod
    async def from_market_slug(
        cls,
        slug: str,
        token_index: int = 0,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> PolymarketDataLoader:
        """
        Create a loader by fetching market data from Polymarket APIs.

        Parameters
        ----------
        slug : str
            The market slug to search for.
        token_index : int, default 0
            The index of the token to use (0 for first outcome, 1 for second).
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for requests. If not provided, a new client will be created.

        Returns
        -------
        PolymarketDataLoader

        Raises
        ------
        ValueError
            If market with slug is not found or has no tokens.
        RuntimeError
            If HTTP requests fail.

        """
        client = http_client or cls._create_http_client()
        market = await cls._fetch_market_by_slug(slug, client)
        condition_id = market["conditionId"]
        market_details = await cls._fetch_market_details(condition_id, client)
        # Populate an effective feeSchedule on the CLOB payload so
        # `parse_polymarket_instrument` can derive an accurate taker fee rate.
        # Reference: https://docs.polymarket.com/trading/fees
        await _populate_fee_schedule(market_details, market, client)
        tokens = market_details.get("tokens", [])

        if not tokens:
            raise ValueError(f"No tokens found for market: {condition_id}")

        if token_index >= len(tokens):
            raise ValueError(
                f"Token index {token_index} out of range (market has {len(tokens)} tokens)",
            )

        token = tokens[token_index]
        token_id = token["token_id"]
        outcome = token["outcome"]

        instrument = parse_polymarket_instrument(
            market_info=market_details,
            token_id=token_id,
            outcome=outcome,
        )

        return cls(
            instrument=instrument,
            token_id=token_id,
            condition_id=condition_id,
            http_client=client,
        )

    @classmethod
    async def from_event_slug(
        cls,
        slug: str,
        token_index: int = 0,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> list[PolymarketDataLoader]:
        """
        Create loaders for all markets in an event.

        This is useful for events that contain multiple related markets,
        such as temperature bucket markets where each bucket is a separate market.

        Parameters
        ----------
        slug : str
            The event slug to fetch.
        token_index : int, default 0
            The index of the token to use (0 for first outcome, 1 for second).
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for requests. If not provided, a new client will be created.

        Returns
        -------
        list[PolymarketDataLoader]
            List of loaders, one for each market in the event.

        Raises
        ------
        ValueError
            If event with slug is not found, has no markets, or token_index is out of range.

        """
        client = http_client or cls._create_http_client()
        event = await cls._fetch_event_by_slug(slug, client)
        markets = event.get("markets", [])

        if not markets:
            raise ValueError(f"No markets found in event '{slug}'")

        loaders: list[PolymarketDataLoader] = []

        for market in markets:
            condition_id = market.get("conditionId")
            if not condition_id:
                continue

            market_details = await cls._fetch_market_details(condition_id, client)
            await _populate_fee_schedule(market_details, market, client)

            tokens = market_details.get("tokens", [])
            if not tokens:
                continue

            if token_index >= len(tokens):
                raise ValueError(
                    f"Token index {token_index} out of range "
                    f"(market {condition_id} has {len(tokens)} tokens)",
                )

            token = tokens[token_index]
            token_id = token["token_id"]
            outcome = token["outcome"]

            instrument = parse_polymarket_instrument(
                market_info=market_details,
                token_id=token_id,
                outcome=outcome,
            )

            loaders.append(
                cls(
                    instrument=instrument,
                    token_id=token_id,
                    condition_id=condition_id,
                    http_client=client,
                ),
            )

        return loaders

    @staticmethod
    async def query_market_by_slug(
        slug: str,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> dict[str, Any]:
        """
        Query market data by slug without requiring a loader instance.

        Parameters
        ----------
        slug : str
            The market slug to fetch.
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for the request.

        Returns
        -------
        dict[str, Any]
            Market data dictionary.

        Raises
        ------
        ValueError
            If market with the given slug is not found.
        RuntimeError
            If HTTP request fails.

        """
        client = http_client or PolymarketDataLoader._create_http_client()
        return await PolymarketDataLoader._fetch_market_by_slug(slug, client)

    @staticmethod
    async def query_market_details(
        condition_id: str,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> dict[str, Any]:
        """
        Query detailed market information without requiring a loader instance.

        Parameters
        ----------
        condition_id : str
            The market condition ID.
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for the request.

        Returns
        -------
        dict[str, Any]
            Detailed market information.

        Raises
        ------
        RuntimeError
            If HTTP request fails.

        """
        client = http_client or PolymarketDataLoader._create_http_client()
        return await PolymarketDataLoader._fetch_market_details(condition_id, client)

    @staticmethod
    async def query_event_by_slug(
        slug: str,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> dict[str, Any]:
        """
        Query event data by slug without requiring a loader instance.

        Parameters
        ----------
        slug : str
            The event slug to fetch.
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for the request.

        Returns
        -------
        dict[str, Any]
            Event data dictionary containing 'markets' array and event metadata.

        Raises
        ------
        ValueError
            If event with the given slug is not found.
        RuntimeError
            If HTTP request fails.

        """
        client = http_client or PolymarketDataLoader._create_http_client()
        return await PolymarketDataLoader._fetch_event_by_slug(slug, client)

    @property
    def instrument(self) -> BinaryOption:
        """
        Return the instrument for this loader.
        """
        return self._instrument

    @property
    def token_id(self) -> str | None:
        """
        Return the token ID for this loader.
        """
        return self._token_id

    @property
    def condition_id(self) -> str | None:
        """
        Return the condition ID for this loader.
        """
        return self._condition_id

    async def load_trades(
        self,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[TradeTick]:
        """
        Load trade ticks from the Polymarket Data API.

        This is a convenience method that fetches and parses historical trades
        using the loader's stored condition_id and token_id.

        Parameters
        ----------
        start : pd.Timestamp, optional
            Start time filter (client-side). If ``None``, no lower bound.
        end : pd.Timestamp, optional
            End time filter (client-side). If ``None``, no upper bound.

        Returns
        -------
        list[TradeTick]
            Parsed trade ticks sorted chronologically, ready for backtesting.

        Raises
        ------
        ValueError
            If condition_id or token_id was not provided during initialization.

        """
        if self._condition_id is None:
            raise ValueError(
                "condition_id is required for this method. "
                "Use from_market_slug() to create a loader with condition_id, "
                "or pass condition_id to __init__()",
            )
        if self._token_id is None:
            raise ValueError(
                "token_id is required for this method. "
                "Use from_market_slug() to create a loader with token_id, "
                "or pass token_id to __init__()",
            )

        raw_trades = await self.fetch_trades(condition_id=self._condition_id)

        # Filter by token_id (API returns trades for all tokens in the condition)
        token_trades = [t for t in raw_trades if t["asset"] == self._token_id]

        # Filter by time range (client-side, API has no time params)
        start_ts = int(start.timestamp()) if start is not None else None
        end_ts = int(end.timestamp()) if end is not None else None

        if start_ts is not None:
            token_trades = [t for t in token_trades if t["timestamp"] >= start_ts]
        if end_ts is not None:
            token_trades = [t for t in token_trades if t["timestamp"] <= end_ts]

        # Sort chronologically (API returns newest first)
        token_trades.sort(key=lambda t: t["timestamp"])

        return self.parse_trades(token_trades)

    async def fetch_event_by_slug(self, slug: str) -> dict[str, Any]:
        """
        Fetch an event by slug from the Polymarket Gamma API.

        Events contain multiple markets (e.g., temperature bucket markets
        are grouped under a single event like "highest-temperature-in-nyc-on-january-26").

        Parameters
        ----------
        slug : str
            The event slug to fetch.

        Returns
        -------
        dict[str, Any]
            Event data dictionary containing 'markets' array and event metadata.

        Raises
        ------
        ValueError
            If event with the given slug is not found.
        RuntimeError
            If HTTP requests fail.

        """
        return await self._fetch_event_by_slug(slug, self._http_client)

    async def fetch_events(
        self,
        active: bool = True,
        closed: bool = False,
        archived: bool = False,
        limit: int = 100,
        offset: int = 0,
    ) -> list[dict[str, Any]]:
        """
        Fetch events from Polymarket Gamma API.

        Parameters
        ----------
        active : bool, default True
            Filter for active events.
        closed : bool, default False
            Include closed events.
        archived : bool, default False
            Include archived events.
        limit : int, default 100
            Maximum number of events to return.
        offset : int, default 0
            Offset for pagination.

        Returns
        -------
        list[dict[str, Any]]
            List of event data dictionaries.

        """
        params = {
            "active": str(active).lower(),
            "closed": str(closed).lower(),
            "archived": str(archived).lower(),
            "limit": str(limit),
            "offset": str(offset),
        }
        response = await self._http_client.get(
            url="https://gamma-api.polymarket.com/events",
            params=params,
        )

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        return msgspec.json.decode(response.body)

    async def get_event_markets(self, slug: str) -> list[dict[str, Any]]:
        """
        Get all markets within an event by slug.

        This is a convenience method that fetches an event and extracts its markets.

        Parameters
        ----------
        slug : str
            The event slug to fetch markets from.

        Returns
        -------
        list[dict[str, Any]]
            List of market dictionaries within the event.

        Raises
        ------
        ValueError
            If event with the given slug is not found.

        """
        event = await self.fetch_event_by_slug(slug)
        return event.get("markets", [])

    async def fetch_markets(
        self,
        active: bool = True,
        closed: bool = False,
        archived: bool = False,
        limit: int = 100,
        offset: int = 0,
    ) -> list[dict]:
        """
        Fetch markets from Polymarket Gamma API.

        Parameters
        ----------
        active : bool, default True
            Filter for active markets.
        closed : bool, default False
            Include closed markets.
        archived : bool, default False
            Include archived markets.
        limit : int, default 100
            Maximum number of markets to return.
        offset : int, default 0
            Offset for pagination.

        Returns
        -------
        list[dict]
            List of market data dictionaries.

        """
        params = {
            "active": str(active).lower(),
            "closed": str(closed).lower(),
            "archived": str(archived).lower(),
            "limit": str(limit),
            "offset": str(offset),
        }
        response = await self._http_client.get(
            url="https://gamma-api.polymarket.com/markets",
            params=params,
        )

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        return msgspec.json.decode(response.body)

    async def fetch_market_by_slug(self, slug: str) -> dict[str, Any]:
        """
        Fetch a single market by slug using the Polymarket Gamma API slug endpoint.

        Parameters
        ----------
        slug : str
            The market slug to fetch.

        Returns
        -------
        dict[str, Any]
            Market data dictionary.

        Raises
        ------
        ValueError
            If market with the given slug is not found.
        RuntimeError
            If HTTP requests fail.

        """
        return await self._fetch_market_by_slug(slug, self._http_client)

    async def find_market_by_slug(self, slug: str) -> dict[str, Any]:
        """
        Find a specific market by slug.

        Parameters
        ----------
        slug : str
            The market slug to search for.

        Returns
        -------
        dict[str, Any]
            Market data dictionary.

        Raises
        ------
        ValueError
            If market with the given slug is not found.

        """
        return await self.fetch_market_by_slug(slug)

    async def fetch_market_details(self, condition_id: str) -> dict[str, Any]:
        """
        Fetch detailed market information from Polymarket CLOB API.

        Parameters
        ----------
        condition_id : str
            The market condition ID.

        Returns
        -------
        dict[str, Any]
            Detailed market information.

        """
        return await self._fetch_market_details(condition_id, self._http_client)

    async def fetch_trades(
        self,
        condition_id: str,
        limit: int = 10_000,
    ) -> list[dict[str, Any]]:
        """
        Fetch trades from the Polymarket Data API.

        Parameters
        ----------
        condition_id : str
            The market condition ID.
        limit : int, default 10_000
            Number of trades per request (max 10,000).

        Returns
        -------
        list[dict[str, Any]]
            List of trade dictionaries (newest first).

        Notes
        -----
        This method automatically handles pagination using offset-based requests.
        The API caps offset at 10,000, so a maximum of ~20,000 trades can be
        fetched per condition.

        """
        PyCondition.valid_string(condition_id, "condition_id")

        all_trades: list[dict[str, Any]] = []
        offset = 0
        page_limit = min(limit, 10_000)

        while True:
            params: dict[str, Any] = {
                "market": condition_id,
                "limit": page_limit,
                "offset": offset,
            }

            response = await self._http_client.get(
                url="https://data-api.polymarket.com/trades",
                params=params,
            )

            if response.status != 200:
                raise RuntimeError(
                    f"HTTP request failed with status {response.status}: "
                    f"{response.body.decode('utf-8')}",
                )

            data = msgspec.json.decode(response.body)

            if not data:
                break

            all_trades.extend(data)
            offset += len(data)

            if len(data) < page_limit or offset > 10_000:
                break

        return all_trades

    def parse_trades(
        self,
        trades_data: list[dict],
    ) -> list[TradeTick]:
        """
        Parse trade data into TradeTicks.

        Parameters
        ----------
        trades_data : list[dict]
            Raw trade data from the Polymarket Data API.

        Returns
        -------
        list[TradeTick]
            List of TradeTicks for backtesting.

        """
        if self._token_id is None:
            raise ValueError(
                "token_id is required to parse trades. "
                "Use from_market_slug() to create a loader with token_id, "
                "or pass token_id to __init__()",
            )

        trades: list[TradeTick] = []
        instrument_id = self._instrument.id
        make_price = self._instrument.make_price
        make_qty = self._instrument.make_qty
        token_id = self._token_id

        for trade_data in trades_data:
            # Skip trades for other tokens in the same condition
            if trade_data.get("asset") != token_id:
                continue
            ts_event = secs_to_nanos(trade_data["timestamp"])

            side_str = trade_data["side"]
            if side_str == "BUY":
                aggressor_side = AggressorSide.BUYER
            elif side_str == "SELL":
                aggressor_side = AggressorSide.SELLER
            else:
                aggressor_side = AggressorSide.NO_AGGRESSOR

            trade = TradeTick(
                instrument_id=instrument_id,
                price=make_price(trade_data["price"]),
                size=make_qty(trade_data["size"]),
                aggressor_side=aggressor_side,
                trade_id=TradeId(trade_data["transactionHash"][-36:]),
                ts_event=ts_event,
                ts_init=ts_event,
            )
            trades.append(trade)

        return trades


async def _populate_fee_schedule(
    market_details: dict[str, Any],
    gamma_market: dict[str, Any],
    http_client: nautilus_pyo3.HttpClient,
) -> None:
    """
    Populate the CLOB market payload with an effective `feeSchedule` in place.

    Callers first attempt to reuse a `feeSchedule` already present on the Gamma
    market they fetched. Some Gamma responses (e.g. `/markets/slug/{slug}` and
    `/events?slug=...`) omit the schedule, so the helper falls back to a
    `fetch_fee_schedules` lookup by condition ID. If neither source yields a
    schedule, the CLOB payload is left unchanged and the instrument taker fee
    defaults to zero.

    References
    ----------
    https://docs.polymarket.com/trading/fees

    """
    fee_schedule = gamma_market.get("feeSchedule")
    if fee_schedule is None:
        condition_id = gamma_market.get("conditionId") or market_details.get("condition_id")
        if condition_id:
            fee_schedules = await fetch_fee_schedules(
                http_client=http_client,
                condition_ids=[condition_id],
            )
            fee_schedule = fee_schedules.get(condition_id)

    if fee_schedule is not None:
        market_details["feeSchedule"] = fee_schedule
