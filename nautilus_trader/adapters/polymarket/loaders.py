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

This module includes loaders for:
- Market information from Polymarket's Gamma API
- Orderbook history from DomeAPI
- Price/trade history from Polymarket's CLOB API

"""

import requests

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
    - DomeAPI (orderbook history, available from October 14th, 2025)
    - Polymarket CLOB API (price/trade history)

    """

    @staticmethod
    def fetch_markets(
        active: bool = True,
        closed: bool = False,
        archived: bool = False,
        limit: int = 100,
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

        Returns
        -------
        list[dict]
            List of market data dictionaries.

        """
        url = "https://gamma-api.polymarket.com/markets"
        params: dict[str, str | int] = {
            "active": str(active).lower(),
            "closed": str(closed).lower(),
            "archived": str(archived).lower(),
            "limit": limit,
        }

        response = requests.get(url, params=params)
        response.raise_for_status()
        return response.json()

    @staticmethod
    def find_market_by_slug(slug: str) -> dict:
        """
        Find a specific market by slug.

        Parameters
        ----------
        slug : str
            The market slug to search for.

        Returns
        -------
        dict
            Market data dictionary.

        Raises
        ------
        ValueError
            If market with the given slug is not found.

        """
        markets = PolymarketDataLoader.fetch_markets(limit=100)
        for market in markets:
            if market.get("slug") == slug:
                return market

        raise ValueError(f"Market with slug '{slug}' not found in active markets")

    @staticmethod
    def fetch_market_details(condition_id: str) -> dict:
        """
        Fetch detailed market information from Polymarket CLOB API.

        Parameters
        ----------
        condition_id : str
            The market condition ID.

        Returns
        -------
        dict
            Detailed market information.

        """
        url = f"https://clob.polymarket.com/markets/{condition_id}"
        response = requests.get(url)
        response.raise_for_status()
        return response.json()

    @staticmethod
    def fetch_orderbook_history(
        token_id: str,
        start_time_ms: int,
        end_time_ms: int,
        limit: int = 500,
    ) -> list[dict]:
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
        list[dict]
            List of orderbook snapshot dictionaries.

        Notes
        -----
        DomeAPI orderbook history only has data starting from October 14th, 2025.
        This method automatically handles pagination.

        """
        url = "https://api.domeapi.io/v1/polymarket/orderbooks"
        all_snapshots = []
        pagination_key = None

        while True:
            params: dict[str, str | int] = {
                "token_id": token_id,
                "start_time": start_time_ms,
                "end_time": end_time_ms,
                "limit": limit,
            }

            if pagination_key:
                params["pagination_key"] = pagination_key

            response = requests.get(url, params=params)
            response.raise_for_status()
            data = response.json()

            snapshots = data.get("snapshots", [])
            all_snapshots.extend(snapshots)

            pagination = data.get("pagination", {})
            if not pagination.get("has_more", False):
                break

            pagination_key = pagination.get("pagination_key")
            if not pagination_key:
                break

        return all_snapshots

    @staticmethod
    def fetch_price_history(
        token_id: str,
        start_ts: int,
        end_ts: int,
        fidelity: int = 1,
    ) -> list[dict]:
        """
        Fetch price history from Polymarket CLOB API.

        Parameters
        ----------
        token_id : str
            The market/token identifier.
        start_ts : int
            Unix timestamp in seconds for range start.
        end_ts : int
            Unix timestamp in seconds for range end.
        fidelity : int, default 1
            Data resolution in minutes.

        Returns
        -------
        list[dict]
            List of price history points with 't' (timestamp) and 'p' (price).

        """
        url = "https://clob.polymarket.com/prices-history"
        params: dict[str, str | int] = {
            "market": token_id,
            "startTs": start_ts,
            "endTs": end_ts,
            "fidelity": fidelity,
        }

        response = requests.get(url, params=params)
        response.raise_for_status()
        data = response.json()

        return data.get("history", [])

    @staticmethod
    def parse_orderbook_snapshots(
        snapshots: list[dict],
        instrument: BinaryOption,
    ) -> list[OrderBookDeltas]:
        """
        Parse orderbook snapshots into OrderBookDeltas.

        Parameters
        ----------
        snapshots : list[dict]
            Raw orderbook snapshots from DomeAPI.
        instrument : BinaryOption
            The trading instrument for precision handling.

        Returns
        -------
        list[OrderBookDeltas]
            List of OrderBookDeltas for backtesting.

        """
        all_deltas = []

        for snapshot in snapshots:
            timestamp_ms = snapshot["timestamp"]
            ts_event = millis_to_nanos(timestamp_ms)
            ts_init = ts_event

            deltas = []

            # Clear the book first
            clear_delta = OrderBookDelta.clear(
                instrument_id=instrument.id,
                ts_event=ts_event,
                ts_init=ts_init,
                sequence=0,
            )
            deltas.append(clear_delta)

            # Add bids
            for bid in snapshot.get("bids", []):
                price = instrument.make_price(bid["price"])
                size = instrument.make_qty(bid["size"])

                order = BookOrder(
                    side=OrderSide.BUY,
                    price=price,
                    size=size,
                    order_id=0,
                )

                delta = OrderBookDelta(
                    instrument_id=instrument.id,
                    action=BookAction.ADD,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
                deltas.append(delta)

            # Add asks
            for ask in snapshot.get("asks", []):
                price = instrument.make_price(ask["price"])
                size = instrument.make_qty(ask["size"])

                order = BookOrder(
                    side=OrderSide.SELL,
                    price=price,
                    size=size,
                    order_id=0,
                )

                delta = OrderBookDelta(
                    instrument_id=instrument.id,
                    action=BookAction.ADD,
                    order=order,
                    flags=0,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
                deltas.append(delta)

            if deltas:
                book_deltas = OrderBookDeltas(
                    instrument_id=instrument.id,
                    deltas=deltas,
                )
                all_deltas.append(book_deltas)

        return all_deltas

    @staticmethod
    def parse_price_history(
        history: list[dict],
        instrument: BinaryOption,
    ) -> list[TradeTick]:
        """
        Parse price history into TradeTicks.

        Parameters
        ----------
        history : list[dict]
            Raw price history from CLOB API.
        instrument : BinaryOption
            The trading instrument for precision handling.

        Returns
        -------
        list[TradeTick]
            List of synthetic TradeTicks for backtesting.

        Notes
        -----
        Price history doesn't include actual trade information, so we synthesize
        trades from price points for demonstration purposes.

        """
        trades = []

        for i, point in enumerate(history):
            timestamp = point["t"]  # Unix timestamp
            price_value = point["p"]

            ts_event = millis_to_nanos(int(timestamp * 1000))
            ts_init = ts_event

            price = instrument.make_price(price_value)
            size = instrument.make_qty(1.0)

            # Determine aggressor side from price movement
            aggressor_side = AggressorSide.NO_AGGRESSOR
            if i > 0:
                prev_price = history[i - 1]["p"]
                if price_value > prev_price:
                    aggressor_side = AggressorSide.BUYER
                elif price_value < prev_price:
                    aggressor_side = AggressorSide.SELLER

            trade = TradeTick(
                instrument_id=instrument.id,
                price=price,
                size=size,
                aggressor_side=aggressor_side,
                trade_id=TradeId(str(i)),
                ts_event=ts_event,
                ts_init=ts_init,
            )
            trades.append(trade)

        return trades
