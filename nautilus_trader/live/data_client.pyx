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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.msgbus.message_bus cimport MessageBus


cdef class LiveDataClientFactory:
    """
    Provides a factory for creating `LiveDataClient` instances.
    """

    @staticmethod
    def create(
        str name not None,
        dict config not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        LiveLogger logger not None,
        client_cls=None,
    ):
        """
        Return a new data client from the given parameters.

        Parameters
        ----------
        name : str
            The client name.
        config : dict[str, object]
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : LiveLogger
            The logger for the client.
        client_cls : class, optional
            The internal client constructor. This allows external library and
            testing dependency injection.

        Returns
        -------
        LiveDataClient

        """
        raise NotImplementedError("method must be implemented in the subclass")


cdef class LiveDataClient(DataClient):
    """
    The abstract base class for all live data clients.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``LiveDataClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        client_id : ClientId
            The client ID.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        config : dict[str, object], optional
            The configuration options.

        """
        super().__init__(
            client_id=client_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop


cdef class LiveMarketDataClient(MarketDataClient):
    """
    The abstract base class for all live data clients.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``LiveMarketDataClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop,
            The event loop for the client.
        client_id : ClientId
            The client ID.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        config : dict[str, object], optional
            The configuration options.

        """
        super().__init__(
            client_id=client_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
