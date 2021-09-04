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

import asyncio
import os

from nautilus_trader.adapters.ccxt.data cimport CCXTDataClient
from nautilus_trader.adapters.ccxt.execution cimport BinanceCCXTExecutionClient
from nautilus_trader.adapters.ccxt.execution cimport BitmexCCXTExecutionClient
from nautilus_trader.adapters.ccxt.execution cimport CCXTExecutionClient
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.live.data_client cimport LiveDataClientFactory
from nautilus_trader.live.execution_client cimport LiveExecutionClientFactory
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class CCXTDataClientFactory(LiveDataClientFactory):
    """
    Provides data and execution clients for the unified CCXT Pro API.
    """

    @staticmethod
    def create(
        loop not None: asyncio.AbstractEventLoop,
        str name not None,
        dict config not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        LiveLogger logger not None,
        client_cls=None,
    ):
        """
        Create new CCXT unified data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : dict
            The configuration dictionary.
        msgbus : MessageBus
            The message bus for the clients.
        cache : Cache
            The cache for the clients.
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
            "password": os.getenv(config.get("api_password", ""), ""),
            "timeout": 10000,         # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
            "asyncio_loop": loop,
            "options": {
                "defaultType": config.get("defaultType", "spot"),
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
            except ImportError:  # pragma: no cover
                raise ImportError(
                    "ccxtpro is not installed, "
                    "installation instructions can be found at https://ccxt.pro"
                )
            client_cls: ccxtpro.Exchange = getattr(ccxtpro, name.partition("-")[2].lower())

        client = client_cls(internal_config)
        client.set_sandbox_mode(config.get("sandbox_mode", False))

        # Create client
        return CCXTDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )


cdef class CCXTExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides data and execution clients for the unified CCXT Pro API.
    """

    @staticmethod
    def create(
        loop not None: asyncio.AbstractEventLoop,
        str name not None,
        dict config not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        LiveLogger logger not None,
        client_cls=None,
    ):
        """
        Create new CCXT unified execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the clients.
        name : str
            The client name.
        config : dict
            The configuration dictionary.
        msgbus : MessageBus
            The message bus for the clients.
        cache : Cache
            The cache for the clients.
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
        cdef str account_type_str = config.get("defaultType", "spot")
        cdef dict internal_config = {
            "apiKey": os.getenv(config.get("api_key", ""), ""),
            "secret": os.getenv(config.get("api_secret", ""), ""),
            "password": os.getenv(config.get("api_password", ""), ""),
            "timeout": 10000,         # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
            "asyncio_loop": loop,
            "options": {
                "defaultType": account_type_str,
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
            except ImportError:  # pragma: no cover
                raise ImportError(
                    "ccxtpro is not installed, "
                    "installation instructions can be found at https://ccxt.pro"
                )
            client_cls: ccxtpro.Exchange = getattr(ccxtpro, name.partition("-")[2].lower())

        client = client_cls(internal_config)
        client.set_sandbox_mode(config.get("sandbox_mode", False))

        # Check required CCXT methods are available
        if not client.has.get("fetchTrades", False):  # pragma: no cover
            raise RuntimeError(f"CCXT `fetch_trades` not available for {client.name}")
        if not client.has.get("watchTrades", False):  # pragma: no cover
            raise RuntimeError(f"CCXT `watch_trades` not available for {client.name}")

        # Get account ID env variable or set default
        account_id_env_var = os.getenv(config.get("account_id", ""), "001")

        # Set exchange name
        exchange_name = client.name.upper()

        # Set account ID
        account_id = AccountId(issuer=exchange_name, number=account_id_env_var)
        account_type = AccountType.CASH if account_type_str == "spot" else AccountType.MARGIN

        # Create client
        if exchange_name == "BINANCE":
            return BinanceCCXTExecutionClient(
                loop=loop,
                client=client,
                account_id=account_id,
                account_type=account_type,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
            )
        elif exchange_name == "BITMEX":
            return BitmexCCXTExecutionClient(
                loop=loop,
                client=client,
                account_id=account_id,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
            )
        else:
            return CCXTExecutionClient(
                loop=loop,
                client=client,
                account_id=account_id,
                account_type=account_type,
                base_currency=None,  # Multi-currency account
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
            )
