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

import os

from nautilus_trader.adapters.ccxt.data cimport CCXTDataClient
from nautilus_trader.adapters.ccxt.execution cimport BinanceCCXTExecutionClient
from nautilus_trader.adapters.ccxt.execution cimport BitmexCCXTExecutionClient
from nautilus_trader.adapters.ccxt.execution cimport CCXTExecutionClient
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.live.data_client cimport LiveDataClientFactory
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_client cimport LiveExecutionClientFactory
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.identifiers cimport AccountId


cdef class CCXTDataClientFactory(LiveDataClientFactory):
    """
    Provides data and execution clients for the unified CCXT Pro API.
    """

    @staticmethod
    def create(
        str name not None,
        dict config not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        LiveLogger logger not None,
        client_cls=None,
    ):
        """
        Create new CCXT unified data client.

        Parameters
        ----------
        name : str
            The client name.
        config : dict
            The configuration dictionary.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the clients.
        logger : LiveLogger
            The logger for the clients.
        client_cls : class
            The class to call to return a new internal client.

        Returns
        -------
        CCXTDataClient, CCXTExecClient

        """
        # Build internal configuration
        cdef dict internal_config = {
            "apiKey": os.getenv(config.get("api_key", ""), ""),
            "secret": os.getenv(config.get("api_secret", ""), ""),
            "timeout": 10000,         # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
            "asyncio_loop": engine.get_event_loop(),

            # Set cache limits
            "options": {
                "OHLCVLimit": 1,
                "balancesLimit": 1,
                "tradesLimit": 1,
                "ordersLimit": 1,
            },
        }

        # Create client
        if client_cls is None:
            try:
                import ccxtpro
            except ImportError:
                raise ImportError(
                    "ccxtpro is not installed, "
                    "installation instructions can be found at https://ccxt.pro"
                )
            client_cls: ccxtpro.Exchange = getattr(ccxtpro, name.partition("-")[2].lower())

        client = client_cls(internal_config)
        client.set_sandbox_mode(config.get("sandbox_mode", False))

        # Check required CCXT methods are available
        if not client.has.get("fetchTrades", False):
            raise RuntimeError(f"CCXT `fetch_trades` not available for {client.name}")
        if not client.has.get("fetchOHLCV", False):
            raise RuntimeError(f"CCXT `fetch_ohlcv` not available for {client.name}")
        if not client.has.get("watchOrderBook", False):
            raise RuntimeError(f"CCXT `watch_order_book` not available for {client.name}")
        if not client.has.get("watchTrades", False):
            raise RuntimeError(f"CCXT `watch_trades` not available for {client.name}")
        if not client.has.get("watchOHLCV", False):
            raise RuntimeError(f"CCXT `watch_ohlcv` not available for {client.name}")

        # Create client
        return CCXTDataClient(
            client=client,
            engine=engine,
            clock=clock,
            logger=logger,
        )


cdef class CCXTExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides data and execution clients for the unified CCXT Pro API.
    """

    @staticmethod
    def create(
        str name not None,
        dict config not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        LiveLogger logger not None,
        client_cls=None,
    ):
        """
        Create new CCXT unified execution client.

        Parameters
        ----------
        name : str
            The client name.
        config : dict
            The configuration dictionary.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the clients.
        logger : LiveLogger
            The logger for the clients.
        client_cls : class
            The class to call to return a new CCXT Pro client.

        Returns
        -------
        CCXTExecutionClient

        """
        # Build internal configuration
        cdef dict internal_config = {
            "apiKey": os.getenv(config.get("api_key", ""), ""),
            "secret": os.getenv(config.get("api_secret", ""), ""),
            "timeout": 10000,         # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
            "asyncio_loop": engine.get_event_loop(),

            # Set cache limits
            "options": {
                "OHLCVLimit": 1,
                "balancesLimit": 1,
                "tradesLimit": 1,
                "ordersLimit": 1,
            },
        }

        # Create client
        if client_cls is None:
            try:
                import ccxtpro
            except ImportError:
                raise ImportError(
                    "ccxtpro is not installed, "
                    "installation instructions can be found at https://ccxt.pro"
                )
            client_cls: ccxtpro.Exchange = getattr(ccxtpro, name.partition("-")[2].lower())

        client = client_cls(internal_config)
        client.set_sandbox_mode(config.get("sandbox_mode", False))

        # Check required CCXT methods are available
        if not client.has.get("fetchTrades", False):
            raise RuntimeError(f"CCXT `fetch_trades` not available for {client.name}")
        if not client.has.get("fetchOHLCV", False):
            raise RuntimeError(f"CCXT `fetch_ohlcv` not available for {client.name}")
        if not client.has.get("watchOrderBook", False):
            raise RuntimeError(f"CCXT `watch_order_book` not available for {client.name}")
        if not client.has.get("watchTrades", False):
            raise RuntimeError(f"CCXT `watch_trades` not available for {client.name}")
        if not client.has.get("watchOHLCV", False):
            raise RuntimeError(f"CCXT `watch_ohlcv` not available for {client.name}")

        # Get account identifier env variable or set default
        account_id_env_var = os.getenv(config.get("account_id", ""), "001")

        # Set account identifier
        account_id = AccountId(client.name.upper(), account_id_env_var)

        # Create client
        if client.name.upper() == "BINANCE":
            return BinanceCCXTExecutionClient(
                client=client,
                account_id=account_id,
                engine=engine,
                clock=clock,
                logger=logger,
            )
        elif client.name.upper() == "BITMEX":
            return BitmexCCXTExecutionClient(
                client=client,
                account_id=account_id,
                engine=engine,
                clock=clock,
                logger=logger,
            )
        else:
            return CCXTExecutionClient(
                client=client,
                account_id=account_id,
                engine=engine,
                clock=clock,
                logger=logger,
            )
