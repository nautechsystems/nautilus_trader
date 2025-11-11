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
Provides data loaders for historical Polymarket data from various APIs.
"""

from __future__ import annotations

from typing import Any

import msgspec
import pandas as pd

from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments import BinaryOption


class PolymarketDataLoader:
    """
    Provides a data loader for historical Polymarket market data.

    This loader fetches data from:
    - Polymarket Gamma API (market information)
    - Polymarket CLOB API (price/trade history)
    - DomeAPI (orderbook history, available from October 14th, 2025)

    Parameters
    ----------
    instrument : BinaryOption
        The binary option instrument to load data for.
    token_id : str, optional
        The Polymarket token ID for this instrument.
    http_client : nautilus_pyo3.HttpClient, optional
        The HTTP client to use for requests. If not provided, a new client will be created.

    """

    def __init__(
        self,
        instrument: BinaryOption,
        token_id: str | None = None,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> None:
        self._instrument = instrument
        self._token_id = token_id
        self._http_client = http_client or nautilus_pyo3.HttpClient()

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
        # Find market by slug
        market = await cls.find_market_by_slug(slug, http_client=http_client)
        condition_id = market["conditionId"]

        # Fetch detailed market info
        market_details = await cls.fetch_market_details(
            condition_id,
            http_client=http_client,
        )

        # Get token information
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

        # Create instrument
        instrument = parse_polymarket_instrument(
            market_info=market_details,
            token_id=token_id,
            outcome=outcome,
        )

        return cls(instrument, token_id=token_id, http_client=http_client)

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

    async def load_orderbook_snapshots(
        self,
        start: pd.Timestamp,
        end: pd.Timestamp,
        limit: int = 500,
    ) -> list[OrderBookDeltas]:
        """
        Load orderbook snapshots for the loader's instrument.

        This is a convenience method that fetches and parses orderbook history
        using the loader's stored token_id.

        Parameters
        ----------
        start : pd.Timestamp
            Start time for query window.
        end : pd.Timestamp
            End time for query window.
        limit : int, default 500
            Number of snapshots per request (max 500).

        Returns
        -------
        list[OrderBookDeltas]
            Parsed orderbook deltas ready for backtesting.

        Raises
        ------
        ValueError
            If token_id was not provided during initialization.

        """
        if self._token_id is None:
            raise ValueError(
                "token_id is required for this method. "
                "Use from_market_slug() to create a loader with token_id, "
                "or pass token_id to __init__()",
            )

        # Convert timestamps to milliseconds for the API
        start_time_ms = int(start.timestamp() * 1000)
        end_time_ms = int(end.timestamp() * 1000)

        snapshots = await self.fetch_orderbook_history(
            token_id=self._token_id,
            start_time_ms=start_time_ms,
            end_time_ms=end_time_ms,
            limit=limit,
        )

        return self.parse_orderbook_snapshots(snapshots)

    async def load_trades(
        self,
        start: pd.Timestamp,
        end: pd.Timestamp,
        fidelity: int = 1,
    ) -> list[TradeTick]:
        """
        Load synthetic trade ticks from price history for the loader's instrument.

        This is a convenience method that fetches and parses price history
        using the loader's stored token_id.

        Parameters
        ----------
        start : pd.Timestamp
            Start time for range.
        end : pd.Timestamp
            End time for range.
        fidelity : int, default 1
            Data resolution in minutes.

        Returns
        -------
        list[TradeTick]
            Parsed trade ticks ready for backtesting.

        Raises
        ------
        ValueError
            If token_id was not provided during initialization.

        """
        if self._token_id is None:
            raise ValueError(
                "token_id is required for this method. "
                "Use from_market_slug() to create a loader with token_id, "
                "or pass token_id to __init__()",
            )

        # Convert timestamps to milliseconds for the API
        start_time_ms = int(start.timestamp() * 1000)
        end_time_ms = int(end.timestamp() * 1000)

        history = await self.fetch_price_history(
            token_id=self._token_id,
            start_time_ms=start_time_ms,
            end_time_ms=end_time_ms,
            fidelity=fidelity,
        )

        return self.parse_price_history(history)

    @staticmethod
    async def fetch_markets(
        active: bool = True,
        closed: bool = False,
        archived: bool = False,
        limit: int = 100,
        http_client: nautilus_pyo3.HttpClient | None = None,
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
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for requests. If not provided, a new client will be created.

        Returns
        -------
        list[dict]
            List of market data dictionaries.

        """
        client = http_client or nautilus_pyo3.HttpClient()
        params = {
            "active": str(active).lower(),
            "closed": str(closed).lower(),
            "archived": str(archived).lower(),
            "limit": str(limit),
        }
        response = await client.get(
            url="https://gamma-api.polymarket.com/markets",
            params=params,
        )

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        return msgspec.json.decode(response.body)

    @staticmethod
    async def find_market_by_slug(
        slug: str,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> dict[str, Any]:
        """
        Find a specific market by slug.

        Parameters
        ----------
        slug : str
            The market slug to search for.
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for requests. If not provided, a new client will be created.

        Returns
        -------
        dict[str, Any]
            Market data dictionary.

        Raises
        ------
        ValueError
            If market with the given slug is not found.

        """
        markets = await PolymarketDataLoader.fetch_markets(
            limit=100,
            http_client=http_client,
        )
        for market in markets:
            if market.get("slug") == slug:
                return market

        raise ValueError(f"Market with slug '{slug}' not found in active markets")

    @staticmethod
    async def fetch_market_details(
        condition_id: str,
        http_client: nautilus_pyo3.HttpClient | None = None,
    ) -> dict[str, Any]:
        """
        Fetch detailed market information from Polymarket CLOB API.

        Parameters
        ----------
        condition_id : str
            The market condition ID.
        http_client : nautilus_pyo3.HttpClient, optional
            The HTTP client to use for requests. If not provided, a new client will be created.

        Returns
        -------
        dict[str, Any]
            Detailed market information.

        """
        client = http_client or nautilus_pyo3.HttpClient()
        url = f"https://clob.polymarket.com/markets/{condition_id}"

        response = await client.get(url=url)

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        return msgspec.json.decode(response.body)

    async def fetch_orderbook_history(
        self,
        token_id: str,
        start_time_ms: int,
        end_time_ms: int,
        limit: int = 500,
    ) -> list[dict[str, Any]]:
        """
        Fetch orderbook history from DomeAPI.

        Parameters
        ----------
        token_id : str
            The Polymarket asset/token identifier.
        start_time_ms : int
            Unix timestamp in milliseconds for query window start.
        end_time_ms : int
            Unix timestamp in milliseconds for query window end.
        limit : int, default 500
            Number of snapshots per request (max 500).

        Returns
        -------
        list[dict[str, Any]]
            List of orderbook snapshot dictionaries.

        Notes
        -----
        DomeAPI orderbook history only has data starting from October 14th, 2025.
        This method automatically handles pagination.

        """
        all_snapshots = []
        pagination_key = None

        while True:
            params = {
                "token_id": token_id,
                "start_time": start_time_ms,
                "end_time": end_time_ms,
                "limit": limit,
            }

            if pagination_key:
                params["pagination_key"] = pagination_key

            response = await self._http_client.get(
                url="https://api.domeapi.io/v1/polymarket/orderbooks",
                params=params,
            )

            if response.status != 200:
                raise RuntimeError(
                    f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
                )

            data = msgspec.json.decode(response.body)

            snapshots = data.get("snapshots", [])
            all_snapshots.extend(snapshots)

            pagination = data.get("pagination", {})
            if not pagination.get("has_more", False):
                break

            pagination_key = pagination.get("pagination_key")
            if not pagination_key:
                break

        return all_snapshots

    async def fetch_price_history(
        self,
        token_id: str,
        start_time_ms: int,
        end_time_ms: int,
        fidelity: int = 1,
    ) -> list[dict[str, Any]]:
        """
        Fetch price history from Polymarket CLOB API.

        Parameters
        ----------
        token_id : str
            The market/token identifier.
        start_time_ms : int
            Unix timestamp in milliseconds for range start.
        end_time_ms : int
            Unix timestamp in milliseconds for range end.
        fidelity : int, default 1
            Data resolution in minutes.

        Returns
        -------
        list[dict[str, Any]]
            List of price history points with 't' (timestamp) and 'p' (price).

        """
        # Convert milliseconds to seconds for the CLOB API
        start_time_s = start_time_ms // 1000
        end_time_s = end_time_ms // 1000

        params = {
            "market": token_id,
            "startTs": str(start_time_s),
            "endTs": str(end_time_s),
            "fidelity": str(fidelity),
        }
        response = await self._http_client.get(
            url="https://clob.polymarket.com/prices-history",
            params=params,
        )

        if response.status != 200:
            raise RuntimeError(
                f"HTTP request failed with status {response.status}: {response.body.decode('utf-8')}",
            )

        data = msgspec.json.decode(response.body)

        return data.get("history", [])

    def parse_orderbook_snapshots(
        self,
        snapshots: list[dict],
    ) -> list[OrderBookDeltas]:
        """
        Parse orderbook snapshots into OrderBookDeltas.

        Parameters
        ----------
        snapshots : list[dict]
            Raw orderbook snapshots from DomeAPI.

        Returns
        -------
        list[OrderBookDeltas]
            List of OrderBookDeltas for backtesting.

        """
        all_deltas: list[OrderBookDelta] = []

        for snapshot in snapshots:
            timestamp_ms = snapshot["timestamp"]
            ts_event = millis_to_nanos(timestamp_ms)

            deltas = []

            # Clear the book first
            clear_delta = OrderBookDelta.clear(
                instrument_id=self.instrument.id,
                ts_event=ts_event,
                ts_init=ts_event,
                sequence=0,
            )
            deltas.append(clear_delta)

            # Add bids
            for bid in snapshot.get("bids", []):
                price = self.instrument.make_price(bid["price"])
                size = self.instrument.make_qty(bid["size"])

                order = BookOrder(
                    side=OrderSide.BUY,
                    price=price,
                    size=size,
                    order_id=0,
                )

                delta = OrderBookDelta(
                    instrument_id=self.instrument.id,
                    action=BookAction.ADD,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_event,
                )
                deltas.append(delta)

            # Add asks
            for ask in snapshot.get("asks", []):
                price = self.instrument.make_price(ask["price"])
                size = self.instrument.make_qty(ask["size"])

                order = BookOrder(
                    side=OrderSide.SELL,
                    price=price,
                    size=size,
                    order_id=0,
                )

                delta = OrderBookDelta(
                    instrument_id=self.instrument.id,
                    action=BookAction.ADD,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_event,
                )
                deltas.append(delta)

            if deltas:
                book_deltas = OrderBookDeltas(
                    instrument_id=self.instrument.id,
                    deltas=deltas,
                )
                all_deltas.append(book_deltas)

        return all_deltas

    def parse_price_history(
        self,
        history: list[dict],
    ) -> list[TradeTick]:
        """
        Parse price history into TradeTicks.

        Parameters
        ----------
        history : list[dict]
            Raw price history from CLOB API.

        Returns
        -------
        list[TradeTick]
            List of synthetic TradeTicks for backtesting.

        Notes
        -----
        Price history doesn't include actual trade information, so we synthesize
        trades from price points for demonstration purposes.

        """
        trades: list[TradeTick] = []

        for i, point in enumerate(history):
            timestamp = point["t"]  # Unix timestamp
            price_value = point["p"]

            ts_event = millis_to_nanos(int(timestamp * 1000))

            price = self.instrument.make_price(price_value)
            size = self.instrument.make_qty(1.0)

            # Determine aggressor side from price movement
            aggressor_side = AggressorSide.NO_AGGRESSOR
            if i > 0:
                prev_price = history[i - 1]["p"]
                if price_value > prev_price:
                    aggressor_side = AggressorSide.BUYER
                elif price_value < prev_price:
                    aggressor_side = AggressorSide.SELLER

            trade = TradeTick(
                instrument_id=self.instrument.id,
                price=price,
                size=size,
                aggressor_side=aggressor_side,
                trade_id=TradeId(str(i)),
                ts_event=ts_event,
                ts_init=ts_event,
            )
            trades.append(trade)

        return trades
