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
from typing import Dict, Optional, Tuple

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.currency import Currency
from nautilus_trader.msgbus.bus import MessageBus


CLIENTS: Dict[str, BetfairClient] = {}
INSTRUMENT_PROVIDER = None


@lru_cache(1)
def get_cached_betfair_client(
    loop: asyncio.AbstractEventLoop,
    logger: Logger,
    username: Optional[str] = None,
    password: Optional[str] = None,
    app_key: Optional[str] = None,
    cert_dir: Optional[str] = None,
) -> BetfairClient:
    """
    Cache and return a Betfair HTTP client with the given credentials.

    If a cached client with matching credentials already exists, then that
    cached client will be returned.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
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
    cert_dir : str, optional
        The API SSL certificate directory for the client.
        If None then will source from the `BETFAIR_CERT_DIR` env var.

    Returns
    -------
    BetfairClient

    """
    global CLIENTS

    username = username or os.environ["BETFAIR_USERNAME"]
    password = password or os.environ["BETFAIR_PASSWORD"]
    app_key = app_key or os.environ["BETFAIR_APP_KEY"]
    cert_dir = cert_dir or os.environ["BETFAIR_CERT_DIR"]

    key: str = "|".join((username, password, app_key, cert_dir))
    if key not in CLIENTS:
        LoggerAdapter("BetfairFactory", logger).warning(
            "Creating new instance of BetfairClient",
        )
        client = BetfairClient(
            username=username,
            password=password,
            app_key=app_key,
            cert_dir=cert_dir,
            loop=loop,
            logger=logger,
        )
        CLIENTS[key] = client
    return CLIENTS[key]


@lru_cache(1)
def get_cached_betfair_instrument_provider(
    client: BetfairClient,
    logger: Logger,
    market_filter: tuple,
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
    market_filter : tuple
        The market filter to load into the instrument provider.

    Returns
    -------
    BinanceInstrumentProvider

    """
    global INSTRUMENT_PROVIDER
    if INSTRUMENT_PROVIDER is None:
        LoggerAdapter("BetfairFactory", logger).warning(
            "Creating new instance of BetfairInstrumentProvider"
        )
        INSTRUMENT_PROVIDER = BetfairInstrumentProvider(
            client=client, logger=logger, filters=dict(market_filter)
        )
    return INSTRUMENT_PROVIDER


class BetfairLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `Betfair` live data client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BetfairDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
    ) -> BetfairDataClient:
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
        logger : LiveLogger
            The logger for the client.

        Returns
        -------
        BetfairDataClient

        """
        market_filter: Tuple = config.market_filter or ()

        # Create client
        client = get_cached_betfair_client(
            username=config.username,
            password=config.password,
            app_key=config.app_key,
            cert_dir=config.cert_dir,
            loop=loop,
            logger=logger,
        )
        provider = get_cached_betfair_instrument_provider(
            client=client,
            logger=logger,
            market_filter=market_filter,
        )

        data_client = BetfairDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            market_filter=dict(market_filter),
            instrument_provider=provider,
        )
        return data_client


class BetfairLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides data and execution clients for Betfair.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BetfairExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
    ) -> BetfairExecutionClient:
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
        logger : LiveLogger
            The logger for the client.

        Returns
        -------
        BetfairExecutionClient

        """
        market_filter: Tuple = config.market_filter or ()

        client = get_cached_betfair_client(
            username=config.username,
            password=config.password,
            app_key=config.app_key,
            cert_dir=config.cert_dir,
            loop=loop,
            logger=logger,
        )
        provider = get_cached_betfair_instrument_provider(
            client=client, logger=logger, market_filter=market_filter
        )

        # Create client
        exec_client = BetfairExecutionClient(
            loop=loop,
            client=client,
            base_currency=Currency.from_str(config.base_currency),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            market_filter=dict(market_filter),
            instrument_provider=provider,
        )
        return exec_client
