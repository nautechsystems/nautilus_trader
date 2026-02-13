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

import asyncio
import traceback
from typing import Any

import msgspec
from py_clob_client.client import ClobClient

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.gamma_markets import list_markets
from nautilus_trader.adapters.polymarket.common.gamma_markets import (
    normalize_gamma_market_to_clob_format,
)
from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_condition_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_token_id
from nautilus_trader.adapters.polymarket.http.errors import check_clob_response
from nautilus_trader.adapters.polymarket.loaders import PolymarketDataLoader
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.config import resolve_path
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BinaryOption


class PolymarketInstrumentProviderConfig(InstrumentProviderConfig, frozen=True, kw_only=True):
    """
    Configuration for ``PolymarketInstrumentProvider`` instances.

    Parameters
    ----------
    event_slug_builder : str, optional
        A fully qualified path to a callable that returns a list of event slugs to fetch.
        The callable should have signature: `() -> list[str]`.

        When set, the provider will call this function on each initialization/refresh cycle
        to dynamically generate event slugs, then fetch only those specific events from
        the Gamma API. This is much more efficient for niche markets with predictable
        slug patterns (e.g., temperature markets, UpDown crypto markets).

        Example: "myproject.slugs:build_temperature_slugs"

    """

    event_slug_builder: str | None = None

    def __post_init__(self):
        if self.event_slug_builder and not self.load_all:
            msgspec.structs.force_setattr(self, "load_all", True)


class PolymarketInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Polymarket.

    Parameters
    ----------
    client : ClobClient
        The Polymarket CLOB HTTP client.
    clock : LiveClock
        The clock instance.
    config : PolymarketInstrumentProviderConfig, optional
        The instrument provider configuration, by default None.
    http_client : HttpClient, optional
        The HTTP client for Gamma API requests.

    """

    def __init__(
        self,
        client: ClobClient,
        clock: LiveClock,
        config: PolymarketInstrumentProviderConfig | None = None,
        http_client: HttpClient | None = None,
    ) -> None:
        super().__init__(config=config)
        self._clock = clock
        self._client = client
        self._http_client = http_client or HttpClient(timeout_secs=30)

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        # Check for event_slug_builder first (most efficient for niche markets)
        if (
            isinstance(self._config, PolymarketInstrumentProviderConfig)
            and self._config.event_slug_builder
        ):
            await self._load_from_event_slugs()
        elif self._config.use_gamma_markets:
            await self._load_all_using_gamma_markets(filters)
        else:
            await self._load_markets([], filters)

    async def _load_from_event_slugs(self) -> None:
        """
        Load instruments by fetching specific events via their slugs.

        This method resolves the configured `event_slug_builder` callable,
        invokes it to get a list of event slugs, then fetches each event
        from the Gamma API and loads all instruments from their markets.

        """
        if (
            not isinstance(self._config, PolymarketInstrumentProviderConfig)
            or not self._config.event_slug_builder
        ):
            return
        slug_builder = resolve_path(self._config.event_slug_builder)
        event_slugs: list[str] = slug_builder()

        self._log.info(f"Loading instruments from {len(event_slugs)} event slugs")

        instruments_loaded = 0
        events_loaded = 0

        for slug in event_slugs:
            try:
                event = await PolymarketDataLoader._fetch_event_by_slug(
                    slug=slug,
                    http_client=self._http_client,
                )
                events_loaded += 1
                instruments_loaded += self._load_instruments_from_event(event)

            except ValueError as e:
                # Event not found - log and continue
                if self._log_warnings:
                    self._log.warning(f"Event slug '{slug}' not found: {e}")
            except Exception:
                self._log.error(
                    f"Failed to load event slug '{slug}':\n{traceback.format_exc()}",
                )

        self._log.info(
            f"Loaded {instruments_loaded} instruments from {events_loaded} events",
        )

    def _load_instruments_from_event(self, event: dict[str, Any]) -> int:
        count = 0
        for market in event.get("markets", []):
            condition_id = market.get("conditionId")
            if not condition_id:
                continue

            normalized_market = normalize_gamma_market_to_clob_format(market)

            for token_info in normalized_market.get("tokens", []):
                token_id = token_info["token_id"]
                if not token_id:
                    if self._log_warnings:
                        self._log.warning(f"Market {condition_id} had an empty token")
                    continue

                outcome = token_info["outcome"]
                self._load_instrument(normalized_market, token_id, outcome)
                count += 1
        return count

    async def _load_all_using_gamma_markets(self, filters: dict | None = None) -> None:
        filters = filters.copy() if filters is not None else {}

        self._log.info(f"Loading all instruments via Gamma API with filters {filters}...")

        markets = await list_markets(http_client=self._http_client, filters=filters)
        self._log.info(f"Loaded {len(markets)} markets from Gamma API")

        for market in markets:
            condition_id = market.get("conditionId")
            if not condition_id:
                continue

            normalized_market = normalize_gamma_market_to_clob_format(market)

            for token_info in normalized_market.get("tokens", []):
                token_id = token_info["token_id"]
                if not token_id:
                    self._log.warning(f"Market {condition_id} had an empty token")
                    continue

                outcome = token_info["outcome"]
                self._load_instrument(normalized_market, token_id, outcome)

    async def _load_ids_using_gamma_markets(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load instruments using Gamma API markets.
        """
        # Extract unique condition IDs (markets can have multiple tokens/instruments)
        condition_ids = list({get_polymarket_condition_id(inst_id) for inst_id in instrument_ids})

        # Build set of requested token_ids for filtering
        requested_token_ids = {get_polymarket_token_id(inst_id) for inst_id in instrument_ids}

        # Create a copy to avoid mutating the caller's filters
        filters = filters.copy() if filters is not None else {}

        if (
            len(condition_ids) <= 100
        ):  # We can filter directly by condition_id, but there is an API limit of max 100 condition_ids in the query string
            self._log.info(
                f"Loading {len(instrument_ids)} instruments from {len(condition_ids)} markets, using direct condition_id filtering",
            )
            filters["condition_ids"] = condition_ids
        else:
            self._log.info(
                f"Loading {len(instrument_ids)} instruments from {len(condition_ids)} markets, using bulk load of all markets",
            )

        markets = await list_markets(http_client=self._http_client, filters=filters)
        self._log.info(f"Loaded {len(markets)} markets using Gamma API")
        for market in markets:
            condition_id = market.get("conditionId")
            if not condition_id:
                continue

            if condition_ids and condition_id not in condition_ids:
                continue

            normalized_market = normalize_gamma_market_to_clob_format(market)

            for token_info in normalized_market.get("tokens", []):
                token_id = token_info["token_id"]

                # Only load if this specific token was requested
                if requested_token_ids and token_id not in requested_token_ids:
                    continue

                outcome = token_info["outcome"]
                self._load_instrument(normalized_market, token_id, outcome)

    async def _load_ids_using_clob_api(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load instruments using CLOB API.
        """
        if len(instrument_ids) > 200:
            self._log.warning(
                f"Loading {len(instrument_ids)} instruments, using bulk load of all markets as a faster alternative",
            )
            await self._load_markets(instrument_ids, filters)
        else:
            await self._load_markets_seq(instrument_ids, filters)

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(
                instrument_id.venue,
                POLYMARKET_VENUE,
                "instrument_id.venue",
                "POLYMARKET",
            )

        if self._config.use_gamma_markets:
            await self._load_ids_using_gamma_markets(instrument_ids, filters)
        else:
            await self._load_ids_using_clob_api(instrument_ids, filters)

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        condition_id = get_polymarket_condition_id(instrument_id)
        token_id = get_polymarket_token_id(instrument_id)

        response = await asyncio.to_thread(self._client.get_market, condition_id)
        response = check_clob_response(response)

        for token_info in response["tokens"]:
            if token_id != token_info["token_id"]:
                continue

            outcome = token_info["outcome"]

            try:
                self._load_instrument(response, token_id, outcome)
            except ValueError as e:
                self._log.error(f"Unable to parse market: {e}, {response}")

    async def _load_markets_seq(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        filter_is_active = filters.get("is_active", False) if filters else False

        for instrument_id in instrument_ids:
            response: dict[str, Any] | str = await asyncio.to_thread(
                self._client.get_market,
                condition_id=get_polymarket_condition_id(instrument_id),
            )
            response = check_clob_response(response)

            try:
                active = response["active"]
                closed = response["closed"]

                if filter_is_active and (not active or closed):
                    continue

                condition_id = response["condition_id"]
                if not condition_id:
                    self._log.warning(f"{instrument_id} was archived (no `condition_id`)")
                    continue  # Archived

                for token_info in response["tokens"]:
                    token_id = token_info["token_id"]
                    if not token_id:
                        self._log.warning(f"Market {condition_id} had an empty token")
                        continue
                    outcome = token_info["outcome"]
                    self._load_instrument(response, token_id, outcome)
                    self._log.info(f"Loaded instrument {instrument_id}")
            except ValueError as e:
                self._log.error(f"Unable to parse market: {e}, {response}")

    async def _load_markets(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        # Create a copy to avoid mutating the caller's filters
        filters = filters.copy() if filters is not None else {}

        self._warn_unsupported_clob_filters(filters)

        if instrument_ids:
            instruments_str = "instruments: " + ", ".join([str(x) for x in instrument_ids])
        else:
            instruments_str = "all instruments"
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading {instruments_str}{filters_str}")

        condition_ids = [get_polymarket_condition_id(x) for x in instrument_ids]

        filter_is_active = filters.get("is_active", False)

        markets_visited = 0
        next_cursor = filters.get("next_cursor", "MA==")
        while next_cursor != "LTE=":
            self._log.info(f"Cursor = '{next_cursor}', markets visited = {markets_visited}")
            response: dict[str, Any] | str = await asyncio.to_thread(
                self._client.get_markets,
                next_cursor=next_cursor,
            )
            response = check_clob_response(response)

            for market_info in response["data"]:
                try:
                    active = market_info["active"]
                    closed = market_info["closed"]

                    if filter_is_active and (not active or closed):
                        continue

                    condition_id = market_info["condition_id"]
                    if not condition_id:
                        continue  # Archived

                    if condition_ids and condition_id not in condition_ids:
                        continue  # Filtering

                    for token_info in market_info["tokens"]:
                        token_id = token_info["token_id"]
                        if not token_id:
                            self._log.warning(f"Market {condition_id} had an empty token")
                            continue

                        outcome = token_info["outcome"]
                        self._load_instrument(market_info, token_id, outcome)
                except ValueError as e:
                    self._log.error(f"Unable to parse market: {e}, {market_info}")
                    continue
            next_cursor = response["next_cursor"]
            markets_visited += len(response["data"])

    def _load_instrument(
        self,
        market_info: dict[str, Any],
        token_id: str,
        outcome: str,
    ) -> BinaryOption:
        instrument = parse_polymarket_instrument(
            market_info=market_info,
            token_id=token_id,
            outcome=outcome,
            ts_init=self._clock.timestamp_ns(),
        )
        if market_info["end_date_iso"] is None:
            self._log.warning(f"{instrument.id} expiration is missing, assuming it is still active")

        self.add(instrument)
        return instrument

    def _warn_unsupported_clob_filters(self, filters: dict) -> None:
        gamma_only_filters = {"end_date_min", "end_date_max", "start_date_min", "start_date_max"}
        unsupported = gamma_only_filters & filters.keys()
        if unsupported:
            self._log.warning(
                f"Filters {unsupported} are ignored by CLOB API; "
                "set use_gamma_markets=True to enable server-side filtering",
            )
