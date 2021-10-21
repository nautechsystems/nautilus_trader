# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pprint
from typing import Dict

import orjson

from nautilus_trader.adapters.binance.common import BINANCE_VENUE
from nautilus_trader.adapters.binance.http.api.spot import BinanceSpotHTTPAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId


class BinanceInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` from the Binance API.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        logger: Logger,
    ):
        """
        Initialize a new instance of the ``BetfairInstrumentProvider`` class.

        Parameters
        ----------
        client : APIClient
            The client for the provider.
        logger : Logger
            The logger for the provider.

        """
        super().__init__()

        self.venue = BINANCE_VENUE
        self._client = client
        self._log = LoggerAdapter(type(self).__name__, logger)

        self._spot = BinanceSpotHTTPAPI(self._client)

    async def load_all_async(self):
        """
        Load all Binance instruments into the provider asynchronously.

        """
        response = await self._spot.exchange_info()
        pprint.pprint(orjson.loads(response))

    def load_all(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def load(self, instrument_id: InstrumentId, details: Dict):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
