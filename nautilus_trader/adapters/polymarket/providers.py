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

import asyncio
from typing import Any

import msgspec
from py_clob_client.client import ClobClient

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.parsing import parse_instrument
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_condition_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_token_id
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BinaryOption


class PolymarketInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Polymarket.

    Parameters
    ----------
    client : ClobClient
        The Polymarket CLOB HTTP client.
    clock : LiveClock
        The clock instance.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: ClobClient,
        clock: LiveClock,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._clock = clock
        self._client = client

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        await self._load_markets([], filters)

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

        await self._load_markets_seq(instrument_ids, filters)

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        condition_id = get_polymarket_condition_id(instrument_id)
        token_id = get_polymarket_token_id(instrument_id)

        response = await asyncio.to_thread(self._client.get_market, condition_id)
        if isinstance(response, str):
            raise RuntimeError(f"API error: {response}")  # TBD

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
            if isinstance(response, str):
                raise RuntimeError(f"API error: {response}")  # TBD

            try:
                active = response["active"]
                if filter_is_active and not active:
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

    async def _load_markets(  # noqa: C901 (too complex)
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if filters is None:
            filters = {}

        if instrument_ids:
            instruments_str = "instruments: " + ", ".join([str(x) for x in instrument_ids])
        else:
            instruments_str = "all instruments"
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading {instruments_str}{filters_str}")

        condition_ids = [str(x.symbol) for x in instrument_ids]

        filter_is_active = filters.get("is_active", False)

        next_cursor = filters.get("next_cursor", "MA==")
        while next_cursor != "LTE=":
            self._log.info(f"Cursor = '{next_cursor}'")
            response: dict[str, Any] | str = await asyncio.to_thread(
                self._client.get_markets,
                next_cursor=next_cursor,
            )
            if isinstance(response, str):
                raise RuntimeError(f"API error: {response}")  # TBD

            for market_info in response["data"]:
                try:
                    active = market_info["active"]
                    if filter_is_active and not active:
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

    def _load_instrument(
        self,
        market_info: dict[str, Any],
        token_id: str,
        outcome: str,
    ) -> BinaryOption:
        instrument = parse_instrument(
            market_info=market_info,
            token_id=token_id,
            outcome=outcome,
            ts_init=self._clock.timestamp_ns(),
        )
        if instrument.expiration_ns == 0:
            self._log.warning(f"{instrument.id} expiration was `None`")
        self.add(instrument)
        return instrument
