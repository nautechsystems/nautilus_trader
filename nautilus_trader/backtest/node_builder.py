# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.component import Logger
from nautilus_trader.config import ImportableConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.live.config import LiveDataClientConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.model.identifiers import Venue


class BacktestNodeBuilder:
    """
    Provides building services for a backtest node.

    Parameters
    ----------
    engine : BacktestEngine
        The backtest engine for the node.
    logger : Logger
        The logger for building clients.

    """

    def __init__(
        self,
        engine: BacktestEngine,
        logger: Logger,
    ) -> None:
        self._engine = engine
        self._log = logger
        self._data_factories: dict[str, type[LiveDataClientFactory]] = {}

    def add_data_client_factory(self, name: str, factory: type[LiveDataClientFactory]) -> None:
        """
        Add the given data client factory to the builder.

        Parameters
        ----------
        name : str
            The name of the client.
        factory : type[LiveDataClientFactory]
            The factory to add.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` has already been added.

        """
        if not issubclass(factory, LiveDataClientFactory):
            self._log.error(f"Factory was not of type `LiveDataClientFactory`, was {factory}")
            return

        self._data_factories[name] = factory

    def build_data_clients(
        self,
        config: dict[str, type[LiveDataClientConfig]],
    ) -> None:
        """
        Build the data clients with the given configuration.

        Parameters
        ----------
        config : dict[str, ImportableConfig | LiveExecClientConfig]
            The execution clients configuration.

        """
        PyCondition.not_none(config, "config")

        if not config:
            self._log.warning("No `data_clients` configuration found")

        for parts, cfg in config.items():
            name = parts.partition("-")[0]
            self._log.info(f"Building data client for {name}")

            if isinstance(cfg, ImportableConfig):
                if name not in self._data_factories and cfg.factory is not None:
                    self._data_factories[name] = cfg.factory.create()

                client_config: LiveDataClientConfig = cfg.create()
            else:
                client_config: LiveDataClientConfig = cfg  # type: ignore

            if name not in self._data_factories:
                self._log.error(f"No `LiveDataClientFactory` registered for {name}")
                continue

            factory = self._data_factories[name]

            # We create an event loop here only to satisfy the linters, it won't be used
            client = factory.create(
                loop=asyncio.new_event_loop(),
                name=name,
                config=client_config,
                msgbus=self._engine.kernel.msgbus,
                cache=self._engine.kernel.cache,
                clock=self._engine.kernel.clock,
            )
            client._is_sync = True
            self._engine.kernel.data_engine.register_client(client)

            # Default client config
            if client_config.routing.default:
                self._engine.kernel.data_engine.register_default_client(client)

            # Venue routing config
            venues: frozenset[str] = client_config.routing.venues or frozenset()

            for venue in venues:
                if not isinstance(venue, Venue):
                    venue = Venue(venue)

                self._engine.kernel.data_engine.register_venue_routing(client, venue)
