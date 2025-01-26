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
from functools import lru_cache

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_betfair_client(
    username: str | None = None,
    password: str | None = None,
    app_key: str | None = None,
) -> BetfairHttpClient:
    """
    Cache and return a Betfair HTTP client with the given credentials.

    If a cached client with matching credentials already exists, then that
    cached client will be returned.

    Parameters
    ----------
    username : str, optional
        The API username for the client.
        If None then will source from the `BETFAIR_USERNAME` env var.
    password : str, optional
        The API password for the client.
        If None then will source from the `BETFAIR_PASSWORD` env var.
    app_key : str, optional
        The API application key for the client.
        If None then will source from the `BETFAIR_APP_KEY` env var.

    Returns
    -------
    BetfairHttpClient

    """
    username = username or os.environ["BETFAIR_USERNAME"]
    password = password or os.environ["BETFAIR_PASSWORD"]
    app_key = app_key or os.environ["BETFAIR_APP_KEY"]

    Logger("BetfairFactory").debug("Creating new instance of `BetfairHttpClient`")

    return BetfairHttpClient(
        username=username,
        password=password,
        app_key=app_key,
    )


@lru_cache(1)
def get_cached_betfair_instrument_provider(
    client: BetfairHttpClient,
    config: BetfairInstrumentProviderConfig,
) -> BetfairInstrumentProvider:
    """
    Cache and return a BetfairInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    config : BetfairInstrumentProviderConfig
        The config for the instrument provider.

    Returns
    -------
    BetfairInstrumentProvider

    """
    Logger("BetfairFactory").debug("Creating new instance of `BetfairInstrumentProvider`")

    return BetfairInstrumentProvider(
        client=client,
        config=config,
    )


class BetfairLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Betfair live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BetfairDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ):
        """
        Create a new Betfair data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : dict[str, Any]
            The configuration dictionary.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BetfairDataClient

        """
        # Create client
        client = get_cached_betfair_client(
            username=config.username,
            password=config.password,
            app_key=config.app_key,
        )

        provider = get_cached_betfair_instrument_provider(
            client=client,
            config=config.instrument_config,
        )

        data_client = BetfairDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
        )
        return data_client


class BetfairLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides data and execution clients for Betfair.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BetfairExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ):
        """
        Create a new Betfair execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : dict[str, Any]
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BetfairExecutionClient

        """
        client = get_cached_betfair_client(
            username=config.username,
            password=config.password,
            app_key=config.app_key,
        )
        provider = get_cached_betfair_instrument_provider(
            client=client,
            config=config.instrument_config,
        )

        # Create client
        exec_client = BetfairExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
        )
        return exec_client
