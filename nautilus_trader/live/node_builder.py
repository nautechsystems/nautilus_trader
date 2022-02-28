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
from typing import Dict

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus


class TradingNodeBuilder:
    """
    Provides building services for a trading node.

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

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        data_engine: LiveDataEngine,
        exec_engine: LiveExecutionEngine,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        log: LoggerAdapter,
    ):
        self._msgbus = msgbus
        self._cache = cache
        self._clock = clock
        self._logger = logger
        self._log = log

        self._loop = loop
        self._data_engine = data_engine
        self._exec_engine = exec_engine

        self._data_factories: Dict[str, LiveDataClientFactory] = {}
        self._exec_factories: Dict[str, LiveExecClientFactory] = {}

    def add_data_client_factory(self, name: str, factory):
        """
        Add the given data client factory to the builder.

        Parameters
        ----------
        name : str
            The name of the client.
        factory : LiveDataClientFactory or LiveExecClientFactory
            The factory to add.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` has already been added.

        """
        PyCondition.valid_string(name, "name")
        PyCondition.not_none(factory, "factory")
        PyCondition.not_in(name, self._data_factories, "name", "_data_factories")

        if not issubclass(factory, LiveDataClientFactory):
            self._log.error(f"Factory was not of type `LiveDataClientFactory` " f"was {factory}.")
            return

        self._data_factories[name] = factory

    def add_exec_client_factory(self, name: str, factory):
        """
        Add the given client factory to the builder.

        Parameters
        ----------
        name : str
            The name of the client.
        factory : LiveDataClientFactory or LiveExecClientFactory
            The factory to add.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` has already been added.

        """
        PyCondition.valid_string(name, "name")
        PyCondition.not_none(factory, "factory")
        PyCondition.not_in(name, self._exec_factories, "name", "_exec_factories")

        if not issubclass(factory, LiveExecClientFactory):
            self._log.error(f"Factory was not of type `LiveExecClientFactory` " f"was {factory}.")
            return

        self._exec_factories[name] = factory

    def build_data_clients(self, config: Dict):
        """
        Build the data clients with the given configuration.

        Parameters
        ----------
        config : dict[str, object]
            The data clients configuration.

        """
        PyCondition.not_none(config, "config")

        if not config:
            self._log.warning("No `data_clients` configuration found.")

        for name, client_config in config.items():
            pieces = name.partition("-")
            factory = self._data_factories[pieces[0]]

            client = factory.create(
                loop=self._loop,
                name=name,
                config=client_config,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
            )

            self._data_engine.register_client(client)

            # Default client config
            if client_config.routing.default:
                self._data_engine.register_default_client(client)

            # Venue routing config
            venues = client_config.routing.venues or []
            for venue in venues:
                if not isinstance(venue, Venue):
                    venue = Venue(venue)
                self._data_engine.register_venue_routing(client, venue)

    def build_exec_clients(self, config: Dict):
        """
        Build the execution clients with the given configuration.

        Parameters
        ----------
        config : dict[str, object]
            The execution clients configuration.

        """
        PyCondition.not_none(config, "config")

        if not config:
            self._log.warning("No `exec_clients` configuration found.")

        for name, client_config in config.items():
            pieces = name.partition("-")
            factory = self._exec_factories[pieces[0]]

            client = factory.create(
                loop=self._loop,
                name=name,
                config=client_config,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
            )

            self._exec_engine.register_client(client)

            # Default client config
            if client_config.routing.default:
                self._exec_engine.register_default_client(client)

            # Venue routing config
            venues = client_config.routing.venues or []
            for venue in venues:
                if not isinstance(venue, Venue):
                    venue = Venue(venue)
                self._exec_engine.register_venue_routing(client, venue)
