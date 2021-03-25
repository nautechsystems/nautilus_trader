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

import betfairlightweight

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.live.data_client cimport LiveDataClientFactory
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_client cimport LiveExecutionClientFactory
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.identifiers cimport AccountId

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE

from nautilus_trader.adapters.betfair.data cimport BetfairDataClient
from nautilus_trader.adapters.betfair.execution cimport BetfairExecutionClient


cdef class BetfairLiveDataClientFactory(LiveDataClientFactory):
    @staticmethod
    def create(
            str name not None,
            dict config not None,
            LiveDataEngine engine not None,
            LiveClock clock not None,
            LiveLogger logger not None,
    ):
        """
        Create new Betfair clients.

        Parameters
        ----------
        config : dict
            The configuration dictionary.
        data_engine : LiveDataEngine
            The data engine for the Nautilus clients.
        clock : LiveClock
            The clock for the clients.
        logger : LiveLogger
            The logger for the clients.

        Returns
        -------
        BetfairDataClient

        """
        client: betfairlightweight.APIClient = betfairlightweight.APIClient(
            username=os.getenv(config.get("username", ""), ""),
            password=os.getenv(config.get("password", ""), ""),
            app_key=os.getenv(config.get("app_key", ""), ""),
            certs=os.getenv(config.get("cert_dir", ""), ""),
            lightweight=True,
        )

        data_client = BetfairDataClient(
            client=client,
            engine=engine,
            clock=clock,
            logger=logger,
        )
        return data_client


cdef class BetfairLiveExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides data and execution clients for Betfair.
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
        Create new Betfair clients.

        Parameters
        ----------
        config : dict
            The configuration dictionary.
        exec_engine : LiveDataEngine
            The execution engine for the Nautilus clients.
        clock : LiveClock
            The clock for the clients.
        logger : LiveLogger
            The logger for the clients.

        Returns
        -------
        BetfairDataClient, BetfairExecClient

        """
        # Create client
        client: betfairlightweight.APIClient = betfairlightweight.APIClient(
            username=os.getenv(config.get("username", ""), ""),
            password=os.getenv(config.get("password", ""), ""),
            app_key=os.getenv(config.get("app_key", ""), ""),
            certs=os.getenv(config.get("cert_dir", ""), ""),
            lightweight=True,
        )

        # Get account identifier env variable or set default
        account_id_env_var = os.getenv(config.get("account_id", ""), "001")

        # Set account identifier
        account_id = AccountId(BETFAIR_VENUE.value, account_id_env_var)

        # Create client
        exec_client = BetfairExecutionClient(
            client=client,
            account_id=account_id,
            engine=engine,
            clock=clock,
            logger=logger,
        )
        return exec_client
