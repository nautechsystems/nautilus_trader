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
from functools import lru_cache
from typing import Dict, Literal, Optional

import ib_insync

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
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
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus


GATEWAY = None
IB_INSYNC_CLIENTS: Dict[tuple, ib_insync.IB] = {}


def get_cached_ib_client(
    username: str,
    password: str,
    host: str = "127.0.0.1",
    port: Optional[int] = None,
    trading_mode: Literal["paper", "live"] = "paper",
    connect: bool = True,
    timeout: int = 300,
    client_id: int = 1,
    start_gateway: bool = True,
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
    trading_mode: str
        paper or live
    host : str, optional
        The IB host to connect to
    port : int, optional
        The IB port to connect to
    connect: bool, optional
        Whether to connect to IB.
    timeout: int, optional
        The timeout for trying to establish a connection
    client_id: int, optional
        The client_id to connect with
    start_gateway: bool
        Start the IB Gateway docker container

    Returns
    -------
    ib_insync.IB

    """
    global IB_INSYNC_CLIENTS, GATEWAY
    if start_gateway:
        # Start gateway
        if GATEWAY is None:
            GATEWAY = InteractiveBrokersGateway(
                username=username, password=password, trading_mode=trading_mode
            )
            GATEWAY.safe_start(wait=timeout)
            port = port or GATEWAY.port

    client_key: tuple = (host, port)

    if client_key not in IB_INSYNC_CLIENTS:
        client = ib_insync.IB()
        if connect:
            for _ in range(10):
                try:
                    client.connect(host=host, port=port, timeout=1, clientId=client_id)
                    break
                except (TimeoutError, AttributeError, asyncio.TimeoutError):
                    continue
            else:
                raise TimeoutError(f"Failed to connect to gateway in {timeout}s")

        IB_INSYNC_CLIENTS[client_key] = client
    return IB_INSYNC_CLIENTS[client_key]


@lru_cache(1)
def get_cached_interactive_brokers_instrument_provider(
    client: ib_insync.IB,
    config: InstrumentProviderConfig,
    logger: Logger,
) -> InteractiveBrokersInstrumentProvider:
    """
    Cache and return a InteractiveBrokersInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.

    Parameters
    ----------
    client : InteractiveBrokersHttpClient
        The client for the instrument provider.
    config: InstrumentProviderConfig
        The instrument provider config
    logger : Logger
        The logger for the instrument provider.

    Returns
    -------
    InteractiveBrokersInstrumentProvider

    """
    return InteractiveBrokersInstrumentProvider(client=client, config=config, logger=logger)


class InteractiveBrokersLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `InteractiveBrokers` live data client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        client_cls: Optional[type] = None,
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
            username=config.username,
            password=config.password,
            host=config.gateway_host,
            port=config.gateway_port,
            trading_mode=config.trading_mode,
            client_id=config.client_id,
            start_gateway=config.start_gateway,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(
            client=client, config=config.instrument_provider, logger=logger
        )

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


class InteractiveBrokersLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a `InteractiveBrokers` live execution client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        client_cls: Optional[type] = None,
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
            username=config.username,
            password=config.password,
            host=config.gateway_host,
            port=config.gateway_port,
            client_id=config.client_id,
            start_gateway=config.start_gateway,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(
            client=client, config=config.instrument_provider, logger=logger
        )
        # Set account ID
        account_id = AccountId(f"{IB_VENUE.value}-{config.account_id}")

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
