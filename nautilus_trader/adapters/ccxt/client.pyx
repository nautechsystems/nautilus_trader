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

from nautilus_trader.adapters.ccxt.data cimport CCXTDataClient
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.live.data cimport LiveDataEngine
import ccxtpro
import os


cdef class CCXTDataClientFactory:
    """
    Provides data clients for the Binance exchange.
    """

    @staticmethod
    def create(
        str exchange_name not None,
        dict config not None,
        LiveDataEngine data_engine not None,
        LiveClock clock not None,
        LiveLogger logger not None,
    ):
        """
        Create a new data client for the Binance exchange.

        Parameters
        ----------
        exchange_name : str
            The name of the exchange
        config : dict
            The configuration dictionary.
        data_engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : LiveLogger
            The logger for the client.

        Returns
        -------
        CCXTDataClient

        """
        # Create client
        client: ccxtpro.Exchange = getattr(ccxtpro, exchange_name.lower())({
            "apiKey": os.getenv(config.get("api_key", ""), ""),
            "secret": os.getenv(config.get("api_secret", ""), ""),
            "timeout": 10000,         # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
            "asyncio_loop": data_engine.get_event_loop(),
        })

        return CCXTDataClient(
            client=client,
            engine=data_engine,
            clock=clock,
            logger=logger,
        )
