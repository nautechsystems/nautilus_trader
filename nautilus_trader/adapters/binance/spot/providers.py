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
from typing import Dict, List, Optional

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.spot.http.market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.spot.http.wallet import BinanceSpotWalletHttpAPI
from nautilus_trader.adapters.binance.spot.parsing.data import parse_spot_instrument_http
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotExchangeInfo
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotSymbolInfo
from nautilus_trader.adapters.binance.spot.schemas.wallet import BinanceSpotTradeFees
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId


class BinanceSpotInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading instruments from the `Binance Spot/Margin` exchange.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    logger : Logger
        The logger for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        logger: Logger,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
        config: Optional[InstrumentProviderConfig] = None,
    ):
        super().__init__(
            venue=BINANCE_VENUE,
            logger=logger,
            config=config,
        )

        self._client = client
        self._account_type = account_type

        self._http_wallet = BinanceSpotWalletHttpAPI(self._client)
        self._http_market = BinanceSpotMarketHttpAPI(self._client)

        self._log_warnings = config.log_warnings if config else True

    async def load_all_async(self, filters: Optional[Dict] = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # Get current commission rates
        if self._client.base_url.__contains__("testnet.binance.vision"):
            fees: Dict[str, BinanceSpotTradeFees] = {}
        else:
            try:
                fee_res: List[BinanceSpotTradeFees] = await self._http_wallet.trade_fees()
                fees = {s.symbol: s for s in fee_res}
            except BinanceClientError as e:
                self._log.error(
                    "Cannot load instruments: API key authentication failed "
                    f"(this is needed to fetch the applicable account fee tier). {e.message}",
                )
                return

        # Get exchange info for all assets
        exchange_info: BinanceSpotExchangeInfo = await self._http_market.exchange_info()
        for symbol_info in exchange_info.symbols:
            self._parse_instrument(
                symbol_info=symbol_info,
                fees=fees.get(symbol_info.symbol),
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

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
        self._log.info(f"Loading instruments {instrument_ids}{filters_str}.")

        # Get current commission rates
        try:
            fee_res: List[BinanceSpotTradeFees] = await self._http_wallet.trade_fees()
            fees: Dict[str, BinanceSpotTradeFees] = {s.symbol: s for s in fee_res}
        except BinanceClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to fetch the applicable account fee tier). {e.message}",
            )
            return

        # Extract all symbol strings
        symbols: List[str] = [instrument_id.symbol.value for instrument_id in instrument_ids]

        # Get exchange info for all assets
        exchange_info: BinanceSpotExchangeInfo = await self._http_market.exchange_info(
            symbols=symbols
        )
        for symbol_info in exchange_info.symbols:
            self._parse_instrument(
                symbol_info=symbol_info,
                fees=fees[symbol_info.symbol],
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[Dict] = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.debug(f"Loading instrument {instrument_id}{filters_str}.")

        symbol = instrument_id.symbol.value

        # Get current commission rates
        try:
            fees: BinanceSpotTradeFees = await self._http_wallet.trade_fee(
                symbol=instrument_id.symbol.value
            )
        except BinanceClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to fetch the applicable account fee tier). {e}",
            )
            return

        # Get exchange info for asset
        exchange_info: BinanceSpotExchangeInfo = await self._http_market.exchange_info(
            symbol=symbol
        )
        for symbol_info in exchange_info.symbols:
            self._parse_instrument(
                symbol_info=symbol_info,
                fees=fees,
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    def _parse_instrument(
        self,
        symbol_info: BinanceSpotSymbolInfo,
        fees: Optional[BinanceSpotTradeFees],
        ts_event: int,
    ) -> None:
        ts_init = time.time_ns()
        try:
            instrument = parse_spot_instrument_http(
                symbol_info=symbol_info,
                fees=fees,
                ts_event=min(ts_event, ts_init),
                ts_init=ts_init,
            )
            self.add_currency(currency=instrument.base_currency)
            self.add_currency(currency=instrument.quote_currency)
            self.add(instrument=instrument)

            self._log.debug(f"Added instrument {instrument.id}.")
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {symbol_info.symbol}, {e}.")
