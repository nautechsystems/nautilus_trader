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

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesContractStatus
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesContractType
from nautilus_trader.adapters.binance.futures.http.market import BinanceFuturesMarketHttpAPI
from nautilus_trader.adapters.binance.futures.http.wallet import BinanceFuturesWalletHttpAPI
from nautilus_trader.adapters.binance.futures.parsing.data import parse_futures_instrument_http
from nautilus_trader.adapters.binance.futures.parsing.data import parse_perpetual_instrument_http
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesExchangeInfo
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesSymbolInfo
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId


class BinanceFuturesInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading instruments from the `Binance Futures` exchange.

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
        account_type: BinanceAccountType = BinanceAccountType.FUTURES_USDT,
        config: Optional[InstrumentProviderConfig] = None,
    ):
        super().__init__(
            venue=BINANCE_VENUE,
            logger=logger,
            config=config,
        )

        self._client = client
        self._account_type = account_type

        self._http_wallet = BinanceFuturesWalletHttpAPI(self._client)
        self._http_market = BinanceFuturesMarketHttpAPI(self._client, account_type=account_type)

        self._log_warnings = config.log_warnings if config else True

    async def load_all_async(self, filters: Optional[Dict] = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # Get exchange info for all assets
        exchange_info: BinanceFuturesExchangeInfo = await self._http_market.exchange_info()
        for symbol_info in exchange_info.symbols:
            self._parse_instrument(
                symbol_info=symbol_info,
                fees=None,
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

        # Extract all symbol strings
        symbols: List[str] = [instrument_id.symbol.value for instrument_id in instrument_ids]

        # Get exchange info for all assets
        exchange_info: BinanceFuturesExchangeInfo = await self._http_market.exchange_info(
            symbols=symbols
        )
        for symbol_info in exchange_info.symbols:
            self._parse_instrument(
                symbol_info=symbol_info,
                fees=None,
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[Dict] = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.debug(f"Loading instrument {instrument_id}{filters_str}.")

        symbol = instrument_id.symbol.value

        # Get exchange info for all assets
        exchange_info: BinanceFuturesExchangeInfo = await self._http_market.exchange_info(
            symbol=symbol
        )
        for symbol_info in exchange_info.symbols:
            self._parse_instrument(
                symbol_info=symbol_info,
                fees=None,
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    def _parse_instrument(
        self,
        symbol_info: BinanceFuturesSymbolInfo,
        fees: Optional[Dict[str, Any]],
        ts_event: int,
    ) -> None:
        contract_type_str = symbol_info.contractType

        if (
            contract_type_str == ""
            or symbol_info.status == BinanceFuturesContractStatus.PENDING_TRADING
        ):
            return  # Not yet defined

        try:
            contract_type = BinanceFuturesContractType(contract_type_str)
            if contract_type == BinanceFuturesContractType.PERPETUAL:
                instrument = parse_perpetual_instrument_http(
                    symbol_info=symbol_info,
                    ts_event=ts_event,
                    ts_init=time.time_ns(),
                )
                self.add_currency(currency=instrument.base_currency)
            elif contract_type in (
                BinanceFuturesContractType.CURRENT_MONTH,
                BinanceFuturesContractType.CURRENT_QUARTER,
                BinanceFuturesContractType.NEXT_MONTH,
                BinanceFuturesContractType.NEXT_QUARTER,
            ):
                instrument = parse_futures_instrument_http(
                    symbol_info=symbol_info,
                    ts_event=ts_event,
                    ts_init=time.time_ns(),
                )
                self.add_currency(currency=instrument.underlying)
            else:  # pragma: no cover (design-time error)
                raise RuntimeError(
                    f"invalid BinanceFuturesContractType, was {contract_type}",
                )

            self.add_currency(currency=instrument.quote_currency)
            self.add(instrument=instrument)

            self._log.debug(f"Added instrument {instrument.id}.")
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {symbol_info.symbol}, {e}.")
