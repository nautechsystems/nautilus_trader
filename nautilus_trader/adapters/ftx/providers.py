# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import time
from typing import Any, Dict, List, Optional

from nautilus_trader.adapters.ftx.core.constants import FTX_VENUE
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.http.error import FTXClientError
from nautilus_trader.adapters.ftx.parsing.common import parse_instrument
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument


class FTXInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument`s from the FTX API.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    logger : Logger
        The logger for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.
    override_usd : bool, default False
        If the built-in USD currency should be overridden with the FTX version
        which uses a precision of 8.
    """

    def __init__(
        self,
        client: FTXHttpClient,
        logger: Logger,
        config: Optional[InstrumentProviderConfig] = None,
        override_usd: bool = False,
    ):
        super().__init__(
            venue=FTX_VENUE,
            logger=logger,
            config=config,
        )

        self._client = client

        if override_usd:
            self._log.warning("Overriding default USD for FTX accounting with precision 7.")
            ftx_usd = Currency(
                code=USD.code,
                precision=8,  # For FTX accounting
                iso4217=USD.iso4217,
                name=USD.name,
                currency_type=USD.currency_type,
            )
            Currency.register(currency=ftx_usd, overwrite=True)

        self._log_warnings = config.log_warnings if config else True

    async def load_all_async(self, filters: Optional[Dict] = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        try:
            # Get current commission rates
            account_info: Dict[str, Any] = await self._client.get_account_info()
        except FTXClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to fetch the applicable account fee tier). {e}",
            )
            return

        assets_res: List[Dict[str, Any]] = await self._client.list_markets()

        for data in assets_res:
            self._parse_instrument(data, account_info)

    async def load_ids_async(
        self,
        instrument_ids: List[InstrumentId],
        filters: Optional[Dict] = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading.")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        try:
            # Get current commission rates
            account_info: Dict[str, Any] = await self._client.get_account_info()
        except FTXClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to fetch the applicable account fee tier). {e}",
            )
            return

        assets_res: List[Dict[str, Any]] = await self._client.list_markets()

        # Extract all symbol strings
        symbols: List[str] = [instrument_id.symbol.value for instrument_id in instrument_ids]

        for data in assets_res:
            asset_name = data["name"]
            if asset_name not in symbols:
                continue
            self._parse_instrument(data, account_info)

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[Dict] = None):
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.debug(f"Loading instrument {instrument_id}{filters_str}.")

        try:
            # Get current commission rates
            account_info: Dict[str, Any] = await self._client.get_account_info()
        except FTXClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to fetch the applicable account fee tier). {e}",
            )
            return

        data: Dict[str, Any] = await self._client.get_market(instrument_id.symbol.value)
        self._parse_instrument(data, account_info)

    def _parse_instrument(
        self,
        data: Dict[str, Any],
        account_info: Dict[str, Any],
    ) -> None:
        try:
            asset_type = data["type"]

            instrument: Instrument = parse_instrument(
                account_info=account_info,
                data=data,
                ts_init=time.time_ns(),
            )

            if asset_type == "future":
                if instrument.native_symbol.value.endswith("-PERP"):
                    self.add_currency(currency=instrument.get_base_currency())
            elif asset_type == "spot":
                self.add_currency(
                    currency=instrument.get_base_currency()
                )  # TODO: Temporary until tokenized equity
                # if not instrument.info.get("tokenizedEquity"):
                #     self.add_currency(currency=instrument.get_base_currency())

            self.add_currency(currency=instrument.quote_currency)
            self.add(instrument=instrument)
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {data['name']}, {e}.")
