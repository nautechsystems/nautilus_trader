# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software distributed under the
#  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
#  KIND, either express or implied. See the License for the specific language governing
#  permissions and limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import logging
from datetime import datetime

from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.common.providers import InstrumentProvider


_log = logging.getLogger(__name__)

KALSHI_REST_BASE = "https://api.elections.kalshi.com/trade-api/v2"


class _KalshiHttpClient:
    """
    Thin async HTTP client wrapper exposing a ``get_markets`` method.

    This wrapper exists so that the ``KalshiInstrumentProvider`` can be tested
    by mocking ``provider._http_client.get_markets`` without coupling the
    provider directly to httpx internals.  When the Rust-backed PyO3 HTTP
    client gains a ``get_markets`` binding this class can be replaced.

    Parameters
    ----------
    base_url : str
        The REST base URL (without trailing slash).
    """

    def __init__(self, base_url: str) -> None:
        self._base_url = base_url
        try:
            import httpx  # noqa: PLC0415

            self._client = httpx.AsyncClient(base_url=base_url, timeout=60)
        except ImportError as exc:
            raise RuntimeError(
                "httpx is required for KalshiInstrumentProvider; "
                "install it with: pip install httpx"
            ) from exc

    async def get_markets(
        self,
        series_tickers: tuple[str, ...] = (),
        event_tickers: tuple[str, ...] = (),
    ) -> list[dict]:
        """
        Fetch all active markets from the Kalshi REST API.

        Paginates automatically using the ``cursor`` field returned by the API.
        Server-side filtering by ``series_ticker`` is applied when
        ``series_tickers`` is non-empty.  Client-side filtering by
        ``event_ticker`` is applied when ``event_tickers`` is non-empty.

        Parameters
        ----------
        series_tickers : tuple[str, ...], optional
            Series tickers to restrict the query, e.g. ``("KXBTC",)``.
        event_tickers : tuple[str, ...], optional
            Event tickers for additional client-side filtering.

        Returns
        -------
        list[dict]
            Raw market dictionaries as returned by the Kalshi API.
        """
        params: dict = {"limit": 1000, "status": "open"}
        if series_tickers:
            params["series_ticker"] = ",".join(series_tickers)

        markets: list[dict] = []
        cursor: str | None = None

        while True:
            if cursor:
                params["cursor"] = cursor
            resp = await self._client.get("/markets", params=params)
            resp.raise_for_status()
            data = resp.json()
            markets.extend(data.get("markets", []))
            cursor = data.get("cursor") or None
            if not cursor:
                break

        if event_tickers:
            event_set = set(event_tickers)
            markets = [m for m in markets if m.get("event_ticker") in event_set]

        return markets


class KalshiInstrumentProvider(InstrumentProvider):
    """
    Provides Kalshi prediction market instruments as ``BinaryOption`` objects.

    Instruments are fetched from the Kalshi REST API and filtered by the
    configured series and/or event tickers.

    Parameters
    ----------
    config : KalshiDataClientConfig
        Configuration for the Kalshi adapter.
    """

    def __init__(self, config: KalshiDataClientConfig) -> None:
        super().__init__()  # InstrumentProvider expects InstrumentProviderConfig, not LiveDataClientConfig
        self._config = config
        self._base_url = config.base_url or KALSHI_REST_BASE
        self._http_client = _KalshiHttpClient(base_url=self._base_url)

    async def load_all_async(self, filters: dict | None = None) -> None:
        """Fetch and cache all instruments matching the configured filters."""
        markets = await self._fetch_markets()
        for market in markets:
            try:
                instrument = self._market_to_instrument(market)
                self.add(instrument)
            except Exception as exc:  # noqa: BLE001
                _log.warning(
                    "Kalshi: failed to parse market %s: %s",
                    market.get("ticker"),
                    exc,
                )

    async def _fetch_markets(self) -> list[dict]:
        """Fetch markets from the Kalshi REST API with series/event filtering."""
        return await self._http_client.get_markets(
            series_tickers=self._config.series_tickers,
            event_tickers=self._config.event_tickers,
        )

    def _market_to_instrument(self, market: dict) -> BinaryOption:
        """Convert a Kalshi market dict to a NautilusTrader ``BinaryOption``."""
        import decimal  # noqa: PLC0415

        from nautilus_trader.core.datetime import dt_to_unix_nanos  # noqa: PLC0415
        from nautilus_trader.model.enums import AssetClass  # noqa: PLC0415
        from nautilus_trader.model.identifiers import InstrumentId  # noqa: PLC0415
        from nautilus_trader.model.identifiers import Symbol  # noqa: PLC0415
        from nautilus_trader.model.identifiers import Venue  # noqa: PLC0415
        from nautilus_trader.model.instruments import BinaryOption  # noqa: PLC0415
        from nautilus_trader.model.objects import Currency  # noqa: PLC0415
        from nautilus_trader.model.objects import Price  # noqa: PLC0415
        from nautilus_trader.model.objects import Quantity  # noqa: PLC0415

        ticker = market["ticker"]
        venue = Venue("KALSHI")
        instrument_id = InstrumentId(Symbol(ticker), venue)

        def parse_ts(s: str | None) -> int:
            if not s:
                return 0
            dt = datetime.fromisoformat(s.replace("Z", "+00:00"))
            return dt_to_unix_nanos(dt)

        return BinaryOption(
            instrument_id=instrument_id,
            raw_symbol=Symbol(ticker),
            asset_class=AssetClass.ALTERNATIVE,
            currency=Currency.from_str("USD"),
            activation_ns=parse_ts(market.get("open_time")),
            expiration_ns=parse_ts(
                market.get("close_time") or market.get("latest_expiration_time")
            ),
            price_precision=4,
            size_precision=2,
            price_increment=Price.from_str("0.0001"),
            size_increment=Quantity.from_str("0.01"),
            maker_fee=decimal.Decimal("0"),
            taker_fee=decimal.Decimal("0"),
            outcome="Yes",
            description=market.get("title"),
            ts_event=0,
            ts_init=0,
        )
