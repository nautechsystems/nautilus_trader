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

from __future__ import annotations

from collections.abc import Iterable
from decimal import Decimal
from typing import Any

from nautilus_trader._libnautilus.hyperliquid import HyperliquidHttpClient
from nautilus_trader._libnautilus.hyperliquid import HyperliquidInstrumentDef
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class HyperliquidInstrumentProvider(InstrumentProvider):
    """
    Load spot and perpetual instruments from Hyperliquid's REST ``/info`` API.
    """

    def __init__(
        self,
        client: HyperliquidHttpClient,
        config: InstrumentProviderConfig | None = None,
        *,
        include_spot: bool = True,
        include_perp: bool = True,
    ) -> None:
        PyCondition.not_none(client, "client")
        super().__init__(config=config or InstrumentProviderConfig())

        if not include_spot and not include_perp:
            raise ValueError("At least one of include_spot/include_perp must be True")

        self._client: HyperliquidHttpClient = client
        self._include_spot = include_spot
        self._include_perp = include_perp

        self._definitions: dict[InstrumentId, HyperliquidInstrumentDef] = {}

    # ---------------------------------------------------------------------
    # Public helpers
    # ---------------------------------------------------------------------

    def instruments_pyo3(self) -> list[HyperliquidInstrumentDef]:
        """
        Return the cached PyO3 instrument definitions (mostly for debugging).
        """
        return list(self._definitions.values())

    # ---------------------------------------------------------------------
    # InstrumentProvider interface
    # ---------------------------------------------------------------------

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters = filters or self._filters

        self._log.info("Loading Hyperliquid instrument definitions...")

        try:
            definitions = await self._client.load_instrument_definitions(
                include_perp=self._include_perp,
                include_spot=self._include_spot,
            )
        except AttributeError:  # method missing (old wheel?)
            self._log.error("HyperliquidHttpClient is missing load_instrument_definitions")
            raise
        except Exception as exc:  # pragma: no cover - defensive logging
            self._log.exception("Failed to fetch Hyperliquid instrument metadata", exc)
            raise

        self._log.info("Parsing instrument definitions")

        # Reset caches before repopulating
        self._instruments.clear()
        self._currencies.clear()
        self._definitions.clear()

        loaded = 0
        skipped = 0

        for definition in definitions:
            if not definition.active:
                skipped += 1
                continue

            if not self._accept_definition(definition, filters):
                continue

            try:
                instrument = self._definition_to_instrument(definition)
            except Exception as exc:  # pragma: no cover - defensive logging
                self._log.exception(
                    f"Failed to convert Hyperliquid def {definition.symbol} into instrument",
                    exc,
                )
                skipped += 1
                continue

            self._definitions[instrument.id] = definition
            self.add(instrument)
            loaded += 1

        if loaded:
            self._log.info(f"Loaded {loaded} instruments for venue {HYPERLIQUID_VENUE.value}")
        else:
            self._log.warning("No Hyperliquid instruments matched the requested filters")

        if skipped:
            self._log.debug(f"Skipped {skipped} definitions (inactive or conversion failures)")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        PyCondition.not_none(instrument_ids, "instrument_ids")
        if not instrument_ids:
            self._log.debug("No instrument IDs provided; nothing to load")
            return

        for instrument_id in instrument_ids:
            PyCondition.equal(
                instrument_id.venue,
                HYPERLIQUID_VENUE,
                "instrument_id.venue",
                HYPERLIQUID_VENUE.value,
            )

        # We currently fetch the full catalog (low cost) and rely on filtering afterwards.
        await self.load_all_async(filters)

        missing = [i for i in instrument_ids if i not in self._instruments]
        if missing:
            self._log.warning(
                "Unable to load %d Hyperliquid instruments: %s",
                len(missing),
                ", ".join(i.value for i in missing),
            )

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _definition_to_instrument(self, definition: HyperliquidInstrumentDef) -> Instrument:
        symbol = str(definition.symbol)
        instrument_id = InstrumentId(Symbol(symbol), HYPERLIQUID_VENUE)

        price_increment = Price.from_str(definition.tick_size)
        size_increment = Quantity.from_str(definition.lot_size)

        price_precision = int(definition.price_decimals)
        size_precision = int(definition.size_decimals)

        base_currency = Currency.from_str(definition.base)
        quote_currency = Currency.from_str(definition.quote)

        self.add_currency(base_currency)
        self.add_currency(quote_currency)

        info = self._build_info(definition)

        if definition.market_type == "perp":
            # Perpetual swaps settle in quote currency (USDC on Hyperliquid).
            settlement_currency = quote_currency
            self.add_currency(settlement_currency)

            return CryptoPerpetual(
                instrument_id=instrument_id,
                raw_symbol=Symbol(symbol),
                base_currency=base_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                is_inverse=False,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                ts_event=0,
                ts_init=0,
                info=info,
            )

        # Default to spot pair
        return CurrencyPair(
            instrument_id=instrument_id,
            raw_symbol=Symbol(symbol),
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            ts_event=0,
            ts_init=0,
            lot_size=size_increment,
            info=info,
        )

    def _accept_definition(
        self,
        definition: HyperliquidInstrumentDef,
        filters: dict | None,
    ) -> bool:
        if not filters:
            return True

        def _normalize(value: Any, *, to_lower: bool = False) -> set[str]:
            if value is None:
                return set()
            if isinstance(value, str):
                values: Iterable[str] = [value]
            else:
                values = value
            return {
                (item.lower() if to_lower else item.upper())
                for item in values
                if isinstance(item, str)
            }

        kinds = _normalize(filters.get("market_types") or filters.get("kinds"), to_lower=True)
        if kinds and definition.market_type.lower() not in kinds:
            return False

        bases = _normalize(filters.get("bases"))
        if bases and definition.base.upper() not in bases:
            return False

        quotes = _normalize(filters.get("quotes"))
        if quotes and definition.quote.upper() not in quotes:
            return False

        symbols = _normalize(filters.get("symbols"))
        if symbols and definition.symbol.upper() not in symbols:
            return False

        return True

    @staticmethod
    def _build_info(definition: HyperliquidInstrumentDef) -> dict[str, Any]:
        info: dict[str, Any] = {
            "hl_market_type": definition.market_type,
            "hl_only_isolated": definition.only_isolated,
        }

        if definition.max_leverage is not None:
            info["hl_max_leverage"] = Decimal(definition.max_leverage)

        if getattr(definition, "raw_data", None):
            info["hl_raw"] = definition.raw_data

        return info
