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
import os

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.data import InteractiveBrokersDataClient
from nautilus_trader.adapters.interactive_brokers.execution import InteractiveBrokersExecutionClient
from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import AccountId


# fmt: on

GATEWAYS: dict[tuple, DockerizedIBGateway] = {}
IB_CLIENTS: dict[tuple, InteractiveBrokersClient] = {}
IB_INSTRUMENT_PROVIDERS: dict[tuple, InteractiveBrokersInstrumentProvider] = {}


def get_cached_ib_client(
    loop: asyncio.AbstractEventLoop,
    msgbus: MessageBus,
    cache: Cache,
    clock: LiveClock,
    host: str = "127.0.0.1",
    port: int | None = None,
    client_id: int = 1,
    dockerized_gateway: DockerizedIBGatewayConfig | None = None,
    fetch_all_open_orders: bool = False,
) -> InteractiveBrokersClient:
    """
    Retrieve or create a cached InteractiveBrokersClient using the provided key.

    Should a keyed client already exist within the cache, the function will return this instance. It's important
    to note that the key comprises a combination of the host, port, and client_id.

    When using DockerizedIBGatewayConfig, multiple gateways can be created and cached based on their trading_mode.

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
    host: str
        The IB host to connect to. This is optional if using DockerizedIBGatewayConfig, but is required otherwise.
    port: int
        The IB port to connect to. This is optional if using DockerizedIBGatewayConfig, but is required otherwise.
    client_id: int
        The unique session identifier for the TWS or Gateway.A single host can support multiple connections;
        however, each must use a different client_id.
    dockerized_gateway: DockerizedIBGatewayConfig, optional
        The configuration for the dockerized gateway.If this is provided, Nautilus will oversee the docker
        environment, facilitating the operation of the IB Gateway within. Multiple gateways can be created
        based on trading_mode.
    fetch_all_open_orders : bool, default False
        If True, uses reqAllOpenOrders to fetch orders from all API clients and TWS GUI.
        If False, uses reqOpenOrders to fetch only orders from current client ID session.

    Returns
    -------
    InteractiveBrokersClient

    """
    if dockerized_gateway:
        PyCondition.equal(host, "127.0.0.1", "host", "127.0.0.1")
        PyCondition.none(port, "Ensure `port` is set to None when using DockerizedIBGatewayConfig.")

        # Create a unique key for the gateway based on its trading_mode
        gateway_key = (dockerized_gateway.trading_mode,)

        if gateway_key not in GATEWAYS:
            gateway = DockerizedIBGateway(dockerized_gateway)
            gateway.safe_start(wait=dockerized_gateway.timeout)
            GATEWAYS[gateway_key] = gateway
            port = gateway.port
        else:
            port = GATEWAYS[gateway_key].port
    else:
        PyCondition.not_none(
            host,
            "Please provide the `host` IP address for the IB TWS or Gateway.",
        )
        PyCondition.not_none(port, "Please provide the `port` for the IB TWS or Gateway.")

    client_key: tuple = (host, port, client_id, fetch_all_open_orders)

    if client_key not in IB_CLIENTS:
        client = InteractiveBrokersClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            host=host,
            port=port,
            client_id=client_id,
            fetch_all_open_orders=fetch_all_open_orders,
        )
        client.start()
        IB_CLIENTS[client_key] = client

    return IB_CLIENTS[client_key]


def get_cached_interactive_brokers_instrument_provider(
    client: InteractiveBrokersClient,
    clock: LiveClock,
    config: InteractiveBrokersInstrumentProviderConfig,
) -> InteractiveBrokersInstrumentProvider:
    """
    Cache and return a InteractiveBrokersInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.
    The cache key is based on the client connection parameters and config hash.

    Parameters
    ----------
    client : InteractiveBrokersClient
        The client for the instrument provider.
    clock : LiveClock
        The clock for the provider.
    config: InteractiveBrokersInstrumentProviderConfig
        The instrument provider config

    Returns
    -------
    InteractiveBrokersInstrumentProvider

    """
    global IB_INSTRUMENT_PROVIDERS

    # Create a cache key based on client connection info and config
    # We use the client's connection parameters rather than the client object itself
    # to ensure consistent caching across different client instances with same connection
    client_key = (client._host, client._port, client._client_id)
    provider_key = (client_key, hash(config))

    if provider_key not in IB_INSTRUMENT_PROVIDERS:
        provider = InteractiveBrokersInstrumentProvider(client=client, clock=clock, config=config)
        IB_INSTRUMENT_PROVIDERS[provider_key] = provider

    return IB_INSTRUMENT_PROVIDERS[provider_key]


class InteractiveBrokersLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a InteractiveBrokers live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> InteractiveBrokersDataClient:
        """
        Create a new InteractiveBrokers data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : dict
            The configuration dictionary.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        InteractiveBrokersDataClient

        """
        client = get_cached_ib_client(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            host=config.ibg_host,
            port=config.ibg_port,
            client_id=config.ibg_client_id,
            dockerized_gateway=config.dockerized_gateway,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(
            client=client,
            clock=clock,
            config=config.instrument_provider,
        )

        # Create client
        data_client = InteractiveBrokersDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            ibg_client_id=config.ibg_client_id,
            config=config,
            name=name,
            connection_timeout=config.connection_timeout,
            request_timeout=config.request_timeout,
        )

        return data_client


class InteractiveBrokersLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a InteractiveBrokers live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> InteractiveBrokersExecutionClient:
        """
        Create a new InteractiveBrokers execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : dict[str, object]
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        InteractiveBrokersSpotExecutionClient

        """
        client = get_cached_ib_client(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            host=config.ibg_host,
            port=config.ibg_port,
            client_id=config.ibg_client_id,
            dockerized_gateway=config.dockerized_gateway,
            fetch_all_open_orders=config.fetch_all_open_orders,
        )

        # Get instrument provider singleton
        provider = get_cached_interactive_brokers_instrument_provider(
            client=client,
            clock=clock,
            config=config.instrument_provider,
        )

        # Set account ID
        ib_account = config.account_id or os.environ.get("TWS_ACCOUNT")
        assert (
            ib_account
        ), f"Must pass `{config.__class__.__name__}.account_id` or set `TWS_ACCOUNT` env var."

        account_id = AccountId(f"{name or IB_VENUE.value}-{ib_account}")

        # Create client
        exec_client = InteractiveBrokersExecutionClient(
            loop=loop,
            client=client,
            account_id=account_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
            connection_timeout=config.connection_timeout,
            track_option_exercise_from_position_update=config.track_option_exercise_from_position_update,
        )

        return exec_client
