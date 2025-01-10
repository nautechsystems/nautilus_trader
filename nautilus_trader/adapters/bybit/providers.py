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

from typing import TYPE_CHECKING

import msgspec

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.asset import BybitAssetHttpAPI
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrument
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentInverse
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentLinear
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentList
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentOption
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentSpot
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
    from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.config import InstrumentProviderConfig
    from nautilus_trader.model.identifiers import InstrumentId


class BybitInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Bybit.

    Parameters
    ----------
    client : BybitHttpClient
        The Bybit HTTP client.
    clock : LiveClock
        The clock instance.
    product_types : list[BybitProductType]
        The product types to load.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
        product_types: list[BybitProductType],
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._clock = clock
        self._client = client
        self._product_types = product_types

        self._http_asset = BybitAssetHttpAPI(
            client=client,
            clock=clock,
        )

        self._http_market = BybitMarketHttpAPI(
            client=client,
            clock=clock,
        )
        self._http_account = BybitAccountHttpAPI(
            client=client,
            clock=clock,
        )

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        await self._load_coins()

        instrument_infos: dict[BybitProductType, BybitInstrumentList] = {}
        fee_rates: dict[BybitProductType, list[BybitFeeRate]] = {}

        for product_type in self._product_types:
            instrument_infos[product_type] = await self._http_market.fetch_all_instruments(
                product_type,
            )
            fee_rates[product_type] = await self._http_account.fetch_fee_rate(
                product_type,
            )

        for product_type, instruments in instrument_infos.items():
            for instrument in instruments:
                target_fee_rate = next(
                    (
                        item
                        for item in fee_rates[product_type]
                        if item.symbol == instrument.symbol
                        or (
                            product_type == BybitProductType.OPTION
                            and instrument.baseCoin == item.baseCoin
                        )
                    ),
                    None,
                )
                if target_fee_rate:
                    self._parse_instrument(instrument, target_fee_rate)
                else:
                    self._log.warning(
                        f"Unable to find fee rate for instrument {instrument}",
                    )
        self._log.info(f"Loaded {len(self._instruments)} instruments")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        await self._load_coins()

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, BYBIT_VENUE, "instrument_id.venue", "BYBIT")

        instrument_infos: dict[BybitProductType, BybitInstrumentList] = {}
        fee_rates: dict[BybitProductType, list[BybitFeeRate]] = {}

        for product_type in self._product_types:
            instrument_infos[product_type] = await self._http_market.fetch_all_instruments(
                product_type,
            )
            fee_rates[product_type] = await self._http_account.fetch_fee_rate(
                product_type,
            )

            # Extract symbol strings and product types
            for instrument_id in instrument_ids:
                bybit_symbol = BybitSymbol(instrument_id.symbol.value)
                instrument = await self._http_market.fetch_instrument(
                    bybit_symbol.product_type,
                    bybit_symbol.raw_symbol,
                )
                target_fee_rate = next(
                    (item for item in fee_rates[product_type] if item.symbol == instrument.symbol),
                    None,
                )
                if target_fee_rate:
                    self._parse_instrument(instrument, target_fee_rate)
                else:
                    self._log.warning(
                        f"Unable to find fee rate for instrument {instrument}",
                    )

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)

    async def _load_coins(self) -> None:
        coin_infos = await self._http_asset.fetch_coin_info()

        for coin_info in coin_infos:
            if coin_info.coin == "EVERY":
                # Has precision 18 (exceeds max 9) and not used for any instrument?
                continue
            try:
                currency = coin_info.parse_to_currency()
            except ValueError as e:
                self._log.warning(f"Unable to parse currency {coin_info}: {e}")
                continue

            self.add_currency(currency)

    def _parse_instrument(
        self,
        instrument: BybitInstrument,
        fee_rate: BybitFeeRate,
    ) -> None:
        if isinstance(instrument, BybitInstrumentSpot):
            self._parse_spot_instrument(instrument, fee_rate)
        elif isinstance(instrument, BybitInstrumentLinear):
            # Perpetual and futures
            self._parse_linear_instrument(instrument, fee_rate)
        elif isinstance(instrument, BybitInstrumentInverse):
            # Perpetual and futures (inverse)
            self._parse_inverse_instrument(instrument, fee_rate)
        elif isinstance(instrument, BybitInstrumentOption):
            self._parse_option_instrument(instrument, fee_rate)
        else:
            raise TypeError(f"Unsupported Bybit instrument, was {instrument}")

    def _parse_to_instrument(
        self,
        instrument: BybitInstrument,
        fee_rate: BybitFeeRate,
    ) -> None:
        if isinstance(instrument, BybitInstrumentOption):
            self._log.warning("Parsing of instrument Options is currently not supported")
            return

        try:
            base_currency = self.currency(instrument.baseCoin)
            quote_currency = self.currency(instrument.quoteCoin)
            ts_event = self._clock.timestamp_ns()
            ts_init = self._clock.timestamp_ns()
            instrument = instrument.parse_to_instrument(
                base_currency=base_currency,
                quote_currency=quote_currency,
                fee_rate=fee_rate,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            self.add(instrument=instrument)
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(
                    f"Unable to parse {instrument.__class__.__name__} instrument {instrument.symbol}: {e}",
                )

    def _parse_spot_instrument(
        self,
        data: BybitInstrumentSpot,
        fee_rate: BybitFeeRate,
    ) -> None:
        self._parse_to_instrument(data, fee_rate)

    def _parse_linear_instrument(
        self,
        data: BybitInstrumentLinear,
        fee_rate: BybitFeeRate,
    ) -> None:
        self._parse_to_instrument(data, fee_rate)

    def _parse_inverse_instrument(
        self,
        data: BybitInstrumentInverse,
        fee_rate: BybitFeeRate,
    ) -> None:
        self._parse_to_instrument(data, fee_rate)

    def _parse_option_instrument(
        self,
        instrument: BybitInstrumentOption,
        fee_rate: BybitFeeRate,
    ) -> None:
        self._log.warning("Parsing of instrument Options is currently not supported")
