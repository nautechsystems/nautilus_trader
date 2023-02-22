# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersGatewayConfig
from nautilus_trader.adapters.interactive_brokers.config import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers.data import InteractiveBrokersDataClient
from nautilus_trader.adapters.interactive_brokers.execution import InteractiveBrokersExecutionClient
from nautilus_trader.adapters.interactive_brokers.gateway import InteractiveBrokersGateway
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus


GATEWAY = None
IB_CLIENTS: dict[tuple, InteractiveBrokersClient] = {}


def get_cached_ib_client(
    loop: asyncio.AbstractEventLoop,
    msgbus: MessageBus,
    cache: Cache,
    clock: LiveClock,
    logger: Logger,
    host: str = "127.0.0.1",
    port: Optional[int] = None,
    client_id: int = 1,
    gateway: InteractiveBrokersGatewayConfig = InteractiveBrokersGatewayConfig(),
) -> InteractiveBrokersClient:
    """
    Cache and return a InteractiveBrokers HTTP client with the given key and secret.

    If a cached client with matching key and secret already exists, then that
    cached client will be returned.

    Parameters
    ----------
    loop: asyncio.AbstractEventLoop,
        loop
    msgbus: MessageBus,
        msgbus
    cache: Cache,
        cache
    clock: LiveClock,
        clock
    logger: Logger,
        logger
    host : str, optional
        The IB host to connect to
    port : int, optional
        The IB port to connect to
    client_id: int, optional
        The client_id to connect with
    gateway: InteractiveBrokersGatewayConfig
        Configuration for the gateway.

    Returns
    -------
    InteractiveBrokersClient

    """
    global IB_CLIENTS, GATEWAY
    if gateway.start:
        # Start gateway
        if GATEWAY is None:
            GATEWAY = InteractiveBrokersGateway(**gateway.dict())
            # GATEWAY.safe_start(wait=config.timeout)
            port = port or GATEWAY.port
    port = port or InteractiveBrokersGateway.PORTS[gateway.trading_mode]

    client_key: tuple = (host, port, client_id)

    if client_key not in IB_CLIENTS:
        client = InteractiveBrokersClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            host=host,
            port=port,
            client_id=client_id,
        )
        IB_CLIENTS[client_key] = client
    return IB_CLIENTS[client_key]


@lru_cache(1)
def get_cached_interactive_brokers_instrument_provider(
    client: InteractiveBrokersClient,
    config: InteractiveBrokersInstrumentProviderConfig,
    logger: Logger,
) -> InteractiveBrokersInstrumentProvider:
    """
    Cache and return a InteractiveBrokersInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.

    Parameters
    ----------
    client : InteractiveBrokersClient
        The client for the instrument provider.
    config: InteractiveBrokersInstrumentProviderConfig
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
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
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
        logger : Logger
            The logger for the client.

        Returns
        -------
        InteractiveBrokersDataClient

        """
        client = get_cached_ib_client(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            host=config.ibg_host,
            port=config.ibg_port,
            client_id=config.ibg_client_id,
            gateway=config.gateway,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(
            client=client,
            config=config.instrument_provider,
            logger=logger,
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
            ibg_client_id=config.ibg_client_id,
            config=config,
        )
        return data_client


class InteractiveBrokersLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a `InteractiveBrokers` live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
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
        logger : Logger
            The logger for the client.

        Returns
        -------
        InteractiveBrokersSpotExecutionClient

        """
        client = get_cached_ib_client(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            host=config.ibg_host,
            port=config.ibg_port,
            client_id=config.ibg_client_id,
            gateway=config.gateway,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(
            client=client,
            config=config.instrument_provider,
            logger=logger,
        )

        # Set account ID
        ib_account = config.account_id or os.environ.get("TWS_ACCOUNT")
        assert (
            ib_account
        ), f"Must pass `{config.__class__.__name__}.account_id` or set `TWS_ACCOUNT` env var."

        account_id = AccountId(f"{IB_VENUE.value}-{ib_account}")

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
            ibg_client_id=config.ibg_client_id,
        )
        return exec_client
