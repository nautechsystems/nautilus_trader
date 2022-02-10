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
import os
from functools import lru_cache
from typing import Any, Dict

import ib_insync

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.data import InteractiveBrokersDataClient
from nautilus_trader.adapters.interactive_brokers.execution import InteractiveBrokersExecutionClient
from nautilus_trader.adapters.interactive_brokers.gateway import InteractiveBrokersGateway
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecutionClientFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus


GATEWAY = None
IB_INSYNC_CLIENTS: Dict[tuple, ib_insync.IB] = {}


def get_cached_ib_client(
    username: str,
    password: str,
    host: str = "127.0.0.1",
    port: int = 4001,
    connect=True,
    timeout=15,
) -> ib_insync.IB:
    """
    Cache and return a InteractiveBrokers HTTP client with the given key and secret.

    If a cached client with matching key and secret already exists, then that
    cached client will be returned.

    Parameters
    ----------
    username : str
        Interactive Brokers account username
    password : str
        Interactive Brokers account password
    host : str, optional
        The IB host to connect to
    port : int, optional
        The IB port to connect to
    connect: bool, optional
        Whether to connect to IB.
    timeout: int, optional
        The timeout for trying to establish a connection

    Returns
    -------
    ib_insync.IB

    """
    global IB_INSYNC_CLIENTS, GATEWAY

    # Start gateway
    if GATEWAY is None:
        GATEWAY = InteractiveBrokersGateway(username=username, password=password)
        GATEWAY.safe_start()

    client_key: tuple = (host, port)

    if client_key not in IB_INSYNC_CLIENTS:
        client = ib_insync.IB()
        if connect:
            try:
                client.connect(host=host, port=port, timeout=timeout)
            except TimeoutError:
                raise TimeoutError(f"Failed to connect to gateway in {timeout}s")

        IB_INSYNC_CLIENTS[client_key] = client
    return IB_INSYNC_CLIENTS[client_key]


@lru_cache(1)
def get_cached_interactive_brokers_instrument_provider(
    client: ib_insync.IB,
    logger: Logger,
) -> InteractiveBrokersInstrumentProvider:
    """
    Cache and return a InteractiveBrokersInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.

    Parameters
    ----------
    client : InteractiveBrokersHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.

    Returns
    -------
    InteractiveBrokersInstrumentProvider

    """
    return InteractiveBrokersInstrumentProvider(client=client, logger=logger)


class InteractiveBrokersLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `InteractiveBrokers` live data client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: Dict[str, Any],
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        client_cls=None,
    ) -> InteractiveBrokersDataClient:
        """
        Create a new InteractiveBrokers data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : dict
            The configuration dictionary.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : LiveLogger
            The logger for the client.
        client_cls : class, optional
            The class to call to return a new internal client.

        Returns
        -------
        InteractiveBrokersDataClient

        """
        client = get_cached_ib_client(
            username=config.get("username") or os.environ["TWS_USERNAME"],
            password=config.get("password") or os.environ["TWS_PASSWORD"],
            host=config.get("host") or "127.0.0.1",
            port=config.get("port") or 4001,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(client=client, logger=logger)

        # Create client
        data_client = InteractiveBrokersDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
        )
        return data_client


class InteractiveBrokersLiveExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides a `InteractiveBrokers` live execution client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: Dict[str, Any],
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        client_cls=None,
    ) -> InteractiveBrokersExecutionClient:
        """
        Create a new InteractiveBrokers execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
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
        InteractiveBrokersSpotExecutionClient

        """
        client = get_cached_ib_client(
            username=config.get("username") or os.environ["TWS_USERNAME"],
            password=config.get("password") or os.environ["TWS_PASSWORD"],
            host=config.get("host") or "127.0.0.1",
            port=config.get("port") or 4001,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(client=client, logger=logger)

        # Get account ID env variable or set default
        account_id_env_var = os.getenv(config.get("account_id", ""), "001")

        # Set account ID
        account_id = AccountId(IB_VENUE.value, account_id_env_var)

        # Create client
        exec_client = InteractiveBrokersExecutionClient(
            loop=loop,
            client=client,
            account_id=account_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
        )
        return exec_client
