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
import sys

from nautilus_trader.adapters.ccxt.data cimport CCXTDataClient
from nautilus_trader.adapters.ccxt.execution cimport CCXTExecutionClient
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.identifiers cimport AccountId


try:
    import ccxtpro
except ImportError:
    if "pytest" in sys.modules:
        # Currently under test so continue
        import ccxt as ccxtpro
    else:
        raise ImportError("ccxtpro is not installed, "
                          "installation instructions can be found at https://ccxt.pro")


cdef class CCXTClientsFactory:
    """
    Provides data and execution clients for the unified CCXT Pro API.
    """

    @staticmethod
    def create(
        client_cls not None,
        dict config not None,
        LiveDataEngine data_engine not None,
        LiveExecutionEngine exec_engine not None,
        LiveClock clock not None,
        LiveLogger logger not None,
    ):
        """
        Create new CCXT unified clients.

        Parameters
        ----------
        client_cls : class
            The class to call to return a new CCXT Pro client.
        config : dict
            The configuration dictionary.
        data_engine : LiveDataEngine
            The data engine for the Nautilus clients.
        exec_engine : LiveDataEngine
            The execution engine for the Nautilus clients.
        clock : LiveClock
            The clock for the clients.
        logger : LiveLogger
            The logger for the clients.

        Returns
        -------
        CCXTDataClient, CCXTExecClient

        """
        # Create client
        client: ccxtpro.Exchange = client_cls({
            "apiKey": os.getenv(config.get("api_key", ""), ""),
            "secret": os.getenv(config.get("api_secret", ""), ""),
            "timeout": 10000,         # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
            "asyncio_loop": data_engine.get_event_loop(),

            # Set cache limits
            "options": {
                "OHLCVLimit": 1,
                "balancesLimit": 1,
                "tradesLimit": 1,
                "ordersLimit": 1,
            },
        })

        if config.get("sandbox_mode", False):
            client.set_sandbox_mode(True)

        if config.get("data_client", True):
            # Check required CCXT methods are available
            if not client.has.get("fetchTrades", False):
                raise RuntimeError(f"CCXT `fetch_trades` not available "
                                   f"for {client.name}.")
            if not client.has.get("fetchOHLCV", False):
                raise RuntimeError(f"CCXT `fetch_ohlcv` not available "
                                   f"for {client.name}.")
            if not client.has.get("watchOrderBook", False):
                raise RuntimeError(f"CCXT `watch_order_book` not available "
                                   f"for {client.name}.")
            if not client.has.get("watchTrades", False):
                raise RuntimeError(f"CCXT `watch_trades` not available "
                                   f"for {client.name}.")
            if not client.has.get("watchOHLCV", False):
                raise RuntimeError(f"CCXT `watch_ohlcv` not available "
                                   f"for {client.name}.")

            # Create client
            data_client = CCXTDataClient(
                client=client,
                engine=data_engine,
                clock=clock,
                logger=logger,
            )
        else:
            # The data client was not enabled
            data_client = None

        if config.get("exec_client", True):
            # Check required CCXT methods are available
            if not client.has.get("fetchBalance", False):
                raise RuntimeError(f"CCXT `fetch_balance` not available "
                                   f"for {client.name}.")
            if not client.has.get("watchBalance", False):
                raise RuntimeError(f"CCXT `watch_balance` not available "
                                   f"for {client.name}.")
            if not client.has.get("watchOrders", False):
                raise RuntimeError(f"CCXT `watch_orders` not available "
                                   f"for {client.name}.")
            if not client.has.get("watchMyTrades", False):
                raise RuntimeError("CCXT `watch_my_trades` not available "
                                   f"for {client.name}.")

            # Get account identifier env variable or set default
            account_id_env_var = os.getenv(config.get("account_id", ""), "001")

            # Set account identifier
            account_id = AccountId(client.name.upper(), account_id_env_var)

            # Create client
            exec_client = CCXTExecutionClient(
                client=client,
                account_id=account_id,
                engine=exec_engine,
                clock=clock,
                logger=logger,
            )
        else:
            # The execution client not enabled
            exec_client = None

        return data_client, exec_client
