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

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.currency import Currency
from nautilus_trader.msgbus.bus import MessageBus


CLIENTS: dict[str, BetfairHttpClient] = {}
INSTRUMENT_PROVIDER = None


@lru_cache(1)
def get_cached_betfair_client(
    logger: Logger,
    username: Optional[str] = None,
    password: Optional[str] = None,
    app_key: Optional[str] = None,
) -> BetfairHttpClient:
    """
    Cache and return a Betfair HTTP client with the given credentials.

    If a cached client with matching credentials already exists, then that
    cached client will be returned.

    Parameters
    ----------
    logger : Logger
        The logger for the client.
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
    global CLIENTS

    username = username or os.environ["BETFAIR_USERNAME"]
    password = password or os.environ["BETFAIR_PASSWORD"]
    app_key = app_key or os.environ["BETFAIR_APP_KEY"]

    key: str = "|".join((username, password, app_key))
    if key not in CLIENTS:
        LoggerAdapter("BetfairFactory", logger).warning(
            "Creating new instance of BetfairHttpClient",
        )
        client = BetfairHttpClient(
            username=username,
            password=password,
            app_key=app_key,
            logger=logger,
        )
        CLIENTS[key] = client
    return CLIENTS[key]


@lru_cache(1)
def get_cached_betfair_instrument_provider(
    client: BetfairHttpClient,
    logger: Logger,
    config: BetfairInstrumentProviderConfig,
) -> BetfairInstrumentProvider:
    """
    Cache and return a BetfairInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.
    config : BetfairInstrumentProviderConfig
        The config for the instrument provider.

    Returns
    -------
    BetfairInstrumentProvider

    """
    global INSTRUMENT_PROVIDER
    if INSTRUMENT_PROVIDER is None:
        LoggerAdapter("BetfairFactory", logger).warning(
            "Creating new instance of BetfairInstrumentProvider",
        )
        INSTRUMENT_PROVIDER = BetfairInstrumentProvider(
            client=client,
            logger=logger,
            config=config,
        )
    return INSTRUMENT_PROVIDER


class BetfairLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `Betfair` live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BetfairDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
    ):
        """
        Create a new Betfair data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : dict[str, Any]
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
        BetfairDataClient

        """
        # Create client
        client = get_cached_betfair_client(
            username=config.username,
            password=config.password,
            app_key=config.app_key,
            logger=logger,
        )
        provider = get_cached_betfair_instrument_provider(
            client=client,
            logger=logger,
            config=config.instrument_config,
        )

        data_client = BetfairDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
            account_currency=config.account_currency,
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
        logger: Logger,
    ):
        """
        Create a new Betfair execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : dict[str, Any]
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
        BetfairExecutionClient

        """
        client = get_cached_betfair_client(
            username=config.username,
            password=config.password,
            app_key=config.app_key,
            logger=logger,
        )
        provider = get_cached_betfair_instrument_provider(
            client=client,
            logger=logger,
            config=config.instrument_config,
        )

        # Create client
        exec_client = BetfairExecutionClient(
            loop=loop,
            client=client,
            base_currency=Currency.from_str(config.account_currency),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
        )
        return exec_client
