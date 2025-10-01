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

import asyncio
from collections.abc import Sequence
from decimal import Decimal

from nautilus_trader.adapters.schwab.common import SCHWAB_OPTION_VENUE
from nautilus_trader.adapters.schwab.common import SCHWAB_VENUE
from nautilus_trader.adapters.schwab.common import ParsedOpraSymbol
from nautilus_trader.adapters.schwab.common import parse_opra_symbol
from nautilus_trader.adapters.schwab.config import SchwabInstrumentProviderConfig
from nautilus_trader.adapters.schwab.http.client import SchwabHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.currencies import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class SchwabInstrumentProvider(InstrumentProvider):
    """
    Loads basic instrument metadata using the Schwab REST API.
    """

    def __init__(
        self,
        client: SchwabHttpClient,
        clock: LiveClock,
        config: SchwabInstrumentProviderConfig | None = None,
    ) -> None:
        config = config or SchwabInstrumentProviderConfig()
        super().__init__(config)
        self._client = client
        self._clock = clock
        self._config = config
        self._load_ids_on_start = set(config.load_ids) if config.load_ids is not None else None

    async def load_all_async(self, filters=None) -> None:
        raise RuntimeError(
            "requesting all instrument definitions is not currently supported, "
            "as this would mean every instrument definition for every dataset "
            "(potentially millions)",
        )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters=None,
    ) -> None:
        await self._load_many(instrument_ids)

    async def load_async(self, instrument_id: InstrumentId, filters=None) -> None:
        await self._load_one(instrument_id)

    async def _load_many(self, instrument_ids: Sequence[InstrumentId]) -> None:
        if not instrument_ids:
            return
        await asyncio.gather(*(self._load_one(instrument_id) for instrument_id in instrument_ids))

    async def _load_one(self, instrument_id: InstrumentId) -> None:
        if instrument_id.venue == SCHWAB_OPTION_VENUE:
            option_meta = parse_opra_symbol(instrument_id.symbol.value)
            instrument = await self._build_option(instrument_id, option_meta)
        else:
            instrument = await self._build_equity(instrument_id)

        if instrument is None:
            self._log.warning(f"Unable to bootstrap {instrument_id}")
            return

        self.add(instrument)
        self._log.info(f"Loaded {instrument.id}")

    async def _build_equity(self, instrument_id: InstrumentId) -> Equity | None:
        symbol = instrument_id.symbol.value
        ts = self._clock.timestamp_ns()
        currency = Currency.from_str("USD")
        self.add_currency(currency)
        precision = 2
        tick_size_str = self._format_decimal(0.01, precision)
        instrument = Equity(
            instrument_id=instrument_id,
            raw_symbol=Symbol(symbol),
            currency=currency,
            price_precision=precision,
            price_increment=Price.from_str(tick_size_str),
            lot_size=Quantity.from_int(1),
            ts_event=ts,
            ts_init=ts,
        )
        return instrument

    async def _build_option(
        self,
        instrument_id: InstrumentId,
        option_meta: ParsedOpraSymbol,
    ) -> OptionContract | None:
        symbol = instrument_id.symbol.value
        ts = self._clock.timestamp_ns()
        precision = self._config.option_price_precision
        tick_size_str = self._format_decimal(self._config.option_tick_size, precision)
        strike_str = self._format_decimal(option_meta.strike, precision)

        try:
            expiration_ns = int(option_meta.expiration.timestamp() * 1_000_000_000)
        except AttributeError:
            expiration_ns = ts

        currency = Currency.from_str(self._config.option_currency)
        self.add_currency(currency)

        option = OptionContract(
            instrument_id=instrument_id,
            raw_symbol=Symbol(symbol),
            asset_class=AssetClass.EQUITY,
            exchange=self._config.option_exchange,
            currency=currency,
            price_precision=precision,
            price_increment=Price.from_str(tick_size_str),
            multiplier=Quantity.from_int(int(self._config.option_multiplier)),
            lot_size=Quantity.from_int(1),
            underlying=option_meta.underlying,
            option_kind=option_meta.option_kind,
            strike_price=Price.from_str(strike_str),
            activation_ns=ts,
            expiration_ns=expiration_ns,
            ts_event=ts,
            ts_init=ts,
        )
        return option

    def _resolve_venue(self, asset_type: str) -> Venue:
        if asset_type.upper() == "OPTION":
            return Venue(self._config.option_exchange or SCHWAB_OPTION_VENUE.value)
        return Venue(self._config.equity_exchange or SCHWAB_VENUE.value)

    @staticmethod
    def _format_decimal(value: float | Decimal, precision: int) -> str:
        quantized = Decimal(str(value)).quantize(Decimal(10) ** -precision)
        return format(quantized, f".{precision}f")


__all__ = ["SchwabInstrumentProvider"]
