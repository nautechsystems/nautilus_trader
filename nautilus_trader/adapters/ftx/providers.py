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
from typing import Any, Dict, List

from nautilus_trader.adapters.ftx.common import FTX_VENUE
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.http.error import FTXClientError
from nautilus_trader.adapters.ftx.parsing import parse_market
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.instruments.future import Future


class FTXInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` from the FTX API.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    logger : Logger
        The logger for the provider.
    """

    def __init__(
        self,
        client: FTXHttpClient,
        logger: Logger,
    ):
        super().__init__()

        self.venue = FTX_VENUE
        self._client = client
        self._log = LoggerAdapter(type(self).__name__, logger)

        # Async loading flags
        self._loaded = False
        self._loading = False

    async def load_all_or_wait_async(self) -> None:
        """
        Load the latest FTX instruments into the provider asynchronously, or
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
        Load the latest FTX instruments into the provider asynchronously.

        """
        # Set async loading flag
        self._loading = True

        try:
            # Get current commission rates
            account_info: Dict[str, Any] = await self._client.get_account_info()
        except FTXClientError:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                "(this is needed to fetch the applicable account fee tier).",
            )
            return

        assets_res: List[Dict[str, Any]] = await self._client.list_markets()

        for data in assets_res:
            instrument: Instrument = parse_market(
                account_info=account_info,
                data=data,
                ts_init=time.time_ns(),
            )

            if isinstance(instrument, Future):
                if instrument.native_symbol.value.endswith("-PERP"):
                    self.add_currency(currency=instrument.get_base_currency())
            elif isinstance(instrument, CurrencySpot):
                self.add_currency(
                    currency=instrument.get_base_currency()
                )  # TODO: Temporary until tokenized equity
                # if not instrument.info.get("tokenizedEquity"):
                #     self.add_currency(currency=instrument.get_base_currency())

            self.add_currency(currency=instrument.quote_currency)
            self.add(instrument=instrument)

        # Set async loading flags
        self._loading = False
        self._loaded = True
