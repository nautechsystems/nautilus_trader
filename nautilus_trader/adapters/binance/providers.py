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

import asyncio
import time
from typing import Any, Dict, List, Optional

from nautilus_trader.adapters.binance.core.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.core.enums import BinanceAccountType
from nautilus_trader.adapters.binance.core.enums import BinanceContractType
from nautilus_trader.adapters.binance.http.api.market import BinanceMarketHttpAPI
from nautilus_trader.adapters.binance.http.api.wallet import BinanceWalletHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.parsing.http import parse_future_instrument_http
from nautilus_trader.adapters.binance.parsing.http import parse_perpetual_instrument_http
from nautilus_trader.adapters.binance.parsing.http import parse_spot_instrument_http
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.datetime import millis_to_nanos


class BinanceInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` from the Binance API.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    logger : Logger
        The logger for the provider.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        logger: Logger,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
    ):
        super().__init__()

        self.venue = BINANCE_VENUE
        self._client = client
        self._account_type = account_type
        self._log = LoggerAdapter(type(self).__name__, logger)

        self._wallet = BinanceWalletHttpAPI(self._client)
        self._market = BinanceMarketHttpAPI(self._client, account_type=account_type)

        # Async loading flags
        self._loaded = False
        self._loading = False

    async def load_all_or_wait_async(self) -> None:
        """
        Load the latest Binance instruments into the provider asynchronously, or
        await loading.

        If `load_async` has been previously called then will immediately return.
        """
        if self._loaded:
            return  # Already loaded

        if not self._loading:
            self._log.debug("Loading instruments...")
            await self.load_all_async()
            self._log.info(f"Loaded {self.count} instruments.")
        else:
            self._log.debug("Awaiting loading...")
            while self._loading:
                # Wait 100ms
                await asyncio.sleep(0.1)

    async def load_all_async(self) -> None:
        """
        Load the latest Binance instruments into the provider asynchronously.

        """
        # Set async loading flag
        self._loading = True

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
            contract_type_str = data.get("contractType")
            if contract_type_str is None:  # SPOT
                instrument = parse_spot_instrument_http(
                    data=data,
                    fees=fees,
                    ts_event=server_time_ns,
                    ts_init=time.time_ns(),
                )
                self.add_currency(currency=instrument.base_currency)
            else:
                if contract_type_str == "" and data.get("status") == "PENDING_TRADING":
                    continue  # Not yet defined

                contract_type = BinanceContractType(contract_type_str)
                if contract_type == BinanceContractType.PERPETUAL:
                    instrument = parse_perpetual_instrument_http(
                        data=data,
                        ts_event=server_time_ns,
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
                        ts_event=server_time_ns,
                        ts_init=time.time_ns(),
                    )
                    self.add_currency(currency=instrument.underlying)
                else:  # pragma: no cover (design-time error)
                    raise RuntimeError(
                        f"invalid BinanceContractType, was {contract_type}",
                    )

            self.add_currency(currency=instrument.quote_currency)
            self.add(instrument=instrument)

        # Set async loading flags
        self._loading = False
        self._loaded = True
