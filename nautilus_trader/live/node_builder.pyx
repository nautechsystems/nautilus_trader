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

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.live.data_client cimport LiveDataClientFactory
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_client cimport LiveExecutionClientFactory
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine


cdef class TradingNodeBuilder:
    """
    Provides building services for a trading node.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        LiveDataEngine data_engine not None,
        LiveExecutionEngine exec_engine not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        LiveLogger logger not None,
        LoggerAdapter log not None,
    ):
        """
        Initialize a new instance of the TradingNodeBuilder class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the clients.
        data_engine : LiveDataEngine
            The data engine for the trading node.
        exec_engine : LiveExecutionEngine
            The execution engine for the trading node.
        msgbus : MessageBus
            The message bus for the trading node.
        cache : Cache
            The cache for building clients.
        clock : LiveClock
            The clock for building clients.
        logger : LiveLogger
            The logger for building clients.
        log : LoggerAdapter
            The trading nodes logger.

        """
        self._msgbus = msgbus
        self._cache = cache
        self._clock = clock
        self._logger = logger
        self._log = log

        self._loop = loop
        self._data_engine = data_engine
        self._exec_engine = exec_engine

        self._data_factories = {}  # type: dict[str, LiveDataClientFactory]
        self._exec_factories = {}  # type: dict[str, LiveExecutionClientFactory]

    cpdef void add_data_client_factory(self, str name, factory) except *:
        """
        Add the given data client factory to the builder.

        Parameters
        ----------
        name : str
            The name of the client.
        factory : LiveDataClientFactory or LiveExecutionClientFactory
            The factory to add.

        Raises
        ------
        ValueError
            If name is not a valid string.
        KeyError
            If name has already been added.

        """
        Condition.valid_string(name, "name")
        Condition.not_none(factory, "factory")
        Condition.not_in(name, self._data_factories, "name", "self._data_factories")

        if not issubclass(factory, LiveDataClientFactory):
            self._log.error(f"Factory was not of type `LiveDataClientFactory` "
                            f"was {factory}.")
            return

        self._data_factories[name] = factory

    cpdef void add_exec_client_factory(self, str name, factory) except *:
        """
        Add the given client factory to the builder.

        Parameters
        ----------
        name : str
            The name of the client.
        factory : LiveDataClientFactory or LiveExecutionClientFactory
            The factory to add.

        Raises
        ------
        ValueError
            If name is not a valid string.
        KeyError
            If name has already been added.

        """
        Condition.valid_string(name, "name")
        Condition.not_none(factory, "factory")
        Condition.not_in(name, self._exec_factories, "name", "self._exec_factories")

        if not issubclass(factory, LiveExecutionClientFactory):
            self._log.error(f"Factory was not of type `LiveExecutionClientFactory` "
                            f"was {factory}.")
            return

        self._exec_factories[name] = factory

    cpdef void build_data_clients(self, dict config) except *:
        """
        Build the data clients with the given configuration.

        Parameters
        ----------
        config : dict[str, object]
            The data clients configuration.

        """
        Condition.not_none(config, "config")

        if not config:
            self._log.warning("No `data_clients` configuration found.")

        for name, options in config.items():
            pieces = name.partition("-")
            factory = self._data_factories[pieces[0]]

            client = factory.create(
                loop=self._loop,
                name=name,
                config=options,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
            )

            self._data_engine.register_client(client)

    cpdef void build_exec_clients(self, dict config) except *:
        """
        Build the execution clients with the given configuration.

        Parameters
        ----------
        config : dict[str, object]
            The execution clients configuration.

        """
        Condition.not_none(config, "config")

        if not config:
            self._log.warning("No `exec_clients` configuration found.")

        for name, options in config.items():
            pieces = name.partition("-")
            factory = self._exec_factories[pieces[0]]

            client = factory.create(
                loop=self._loop,
                name=name,
                config=options,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
            )

            self._exec_engine.register_client(client)
