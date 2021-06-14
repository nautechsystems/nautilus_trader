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

import oandapyV20

from nautilus_trader.adapters.oanda.data cimport OandaDataClient
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.live.data_engine cimport LiveDataEngine


cdef class OandaDataClientFactory(LiveDataClientFactory):
    """
    Provides data clients for the Oanda brokerage.
    """

    @staticmethod
    def create(
        str name,
        dict config,
        LiveDataEngine engine,
        LiveClock clock,
        LiveLogger logger,
        client_cls=None,
    ):
        """
        Create a new data client.

        Parameters
        ----------
        name : str
            The name for the client.
        config : dict
            The configuration dictionary.
        engine : DataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : LiveLogger
            The logger for the client.
        client_cls : class, optional
            The class to build an internal client from.

        Returns
        -------
        OandaDataClient

        """
        # Get credentials
        oanda_api_token = os.getenv("api_token", config.get("api_token", ""))
        oanda_account_id = os.getenv("account_id", config.get("account_id", "001"))

        # Create client
        client = oandapyV20.API(access_token=oanda_api_token)

        return OandaDataClient(
            client=client,
            account_id=oanda_account_id,
            engine=engine,
            clock=clock,
            logger=logger,
        )
