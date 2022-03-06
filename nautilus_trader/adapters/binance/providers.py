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

from nautilus_trader.adapters.binance.core.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.core.enums import BinanceAccountType
from nautilus_trader.adapters.binance.core.enums import BinanceContractType
from nautilus_trader.adapters.binance.http.api.market import BinanceMarketHttpAPI
from nautilus_trader.adapters.binance.http.api.wallet import BinanceWalletHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.parsing.http_data import parse_future_instrument_http
from nautilus_trader.adapters.binance.parsing.http_data import parse_perpetual_instrument_http
from nautilus_trader.adapters.binance.parsing.http_data import parse_spot_instrument_http
from nautilus_trader.common.config import InstrumentProviderConfig
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId


class BinanceInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument`s from the Binance API.

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

        self._wallet = BinanceWalletHttpAPI(self._client)
        self._market = BinanceMarketHttpAPI(self._client, account_type=account_type)

    async def load_all_async(self, filters: Optional[Dict] = None) -> None:
        """
        Load the latest instruments into the provider asynchronously, optionally
        applying the given filters.

        Parameters
        ----------
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        """
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # Get current commission rates
        try:
            fees: Optional[Dict[str, Dict[str, str]]] = None
            if self._account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
                fee_res: List[Dict[str, str]] = await self._wallet.trade_fee_spot()
                fees = {s["symbol"]: s for s in fee_res}
        except BinanceClientError:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                "(this is needed to fetch the applicable account fee tier).",
            )
            return

        # Get exchange info for all assets
        response: Dict[str, Any] = await self._market.exchange_info()
        server_time_ns: int = millis_to_nanos(response["serverTime"])

        for data in response["symbols"]:
            self._parse_instrument(data, fees, server_time_ns)

    async def load_ids_async(
        self,
        instrument_ids: List[InstrumentId],
        filters: Optional[Dict] = None,
    ) -> None:
        """
        Load the instruments for the given IDs into the provider, optionally
        applying the given filters.

        Parameters
        ----------
        instrument_ids: List[InstrumentId]
            The instrument IDs to load.
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        Raises
        ------
        ValueError
            If any `instrument_id.venue` is not equal to `self.venue`.

        """
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
            fees: Optional[Dict[str, Dict[str, str]]] = None
            if self._account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
                fee_res: List[Dict[str, str]] = await self._wallet.trade_fee_spot()  # type: ignore
                fees = {s["symbol"]: s for s in fee_res}
        except BinanceClientError:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                "(this is needed to fetch the applicable account fee tier).",
            )
            return

        # Extract all symbol strings
        symbols: List[str] = [instrument_id.symbol.value for instrument_id in instrument_ids]

        # Get exchange info for all assets
        response: Dict[str, Any] = await self._market.exchange_info(symbols=symbols)
        server_time_ns: int = millis_to_nanos(response["serverTime"])

        for data in response["symbols"]:
            self._parse_instrument(data, fees, server_time_ns)

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[Dict] = None):
        """
        Load the instrument for the given ID into the provider asynchronously, optionally
        applying the given filters.

        Parameters
        ----------
        instrument_id: InstrumentId
            The instrument ID to load.
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        Raises
        ------
        ValueError
            If `instrument_id.venue` is not equal to `self.venue`.

        """
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.debug(f"Loading instrument {instrument_id}{filters_str}.")

        symbol = instrument_id.symbol.value

        # Get current commission rates
        try:
            fees: Optional[Dict[str, str]] = None
            if self._account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
                fee_res: Dict[str, Any] = await self._wallet.trade_fee_spot(symbol=symbol)  # type: ignore
                fees = fee_res["symbol"]
        except BinanceClientError:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                "(this is needed to fetch the applicable account fee tier).",
            )
            return

        # Get exchange info for all assets
        response: Dict[str, Any] = await self._market.exchange_info(symbol=symbol)
        server_time_ns: int = millis_to_nanos(response["serverTime"])

        for data in response["symbols"]:
            self._parse_instrument(data, fees, server_time_ns)

    def _parse_instrument(
        self,
        data: Dict[str, Any],
        fees: Dict[str, Any],
        ts_event: int,
    ) -> None:
        contract_type_str = data.get("contractType")
        if contract_type_str is None:  # SPOT
            instrument = parse_spot_instrument_http(
                data=data,
                fees=fees,
                ts_event=ts_event,
                ts_init=time.time_ns(),
            )
            self.add_currency(currency=instrument.base_currency)
        else:
            if contract_type_str == "" and data.get("status") == "PENDING_TRADING":
                return  # Not yet defined

            contract_type = BinanceContractType(contract_type_str)
            if contract_type == BinanceContractType.PERPETUAL:
                instrument = parse_perpetual_instrument_http(
                    data=data,
                    ts_event=ts_event,
                    ts_init=time.time_ns(),
                )
                self.add_currency(currency=instrument.base_currency)
            elif contract_type in (
                BinanceContractType.CURRENT_MONTH,
                BinanceContractType.CURRENT_QUARTER,
                BinanceContractType.NEXT_MONTH,
                BinanceContractType.NEXT_QUARTER,
            ):
                instrument = parse_future_instrument_http(
                    data=data,
                    ts_event=ts_event,
                    ts_init=time.time_ns(),
                )
                self.add_currency(currency=instrument.underlying)
            else:  # pragma: no cover (design-time error)
                raise RuntimeError(
                    f"invalid BinanceContractType, was {contract_type}",
                )

        self.add_currency(currency=instrument.quote_currency)
        self.add(instrument=instrument)
