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
import hashlib
import os
from functools import lru_cache
from typing import Any, Dict, Optional

from nautilus_trader.adapters.binance.common import BINANCE_VENUE
from nautilus_trader.adapters.binance.data import BinanceDataClient
from nautilus_trader.adapters.binance.execution import BinanceSpotExecutionClient
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecutionClientFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus


HTTP_CLIENTS: Dict[str, BinanceHttpClient] = {}


def get_cached_binance_http_client(
    key: Optional[str],
    secret: Optional[str],
    loop: asyncio.AbstractEventLoop,
    clock: LiveClock,
    logger: Logger,
) -> BinanceHttpClient:
    """
    Cache and return a Binance HTTP client with the given key or secret.

    If a cached client with matching key and secret already exists, then that
    cached client will be returned.

    Parameters
    ----------
    key : str, optional
        The API key for the client.
    secret : str, optional
        The API secret for the client.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.

    Returns
    -------
    BinanceHttpClient

    """
    global HTTP_CLIENTS

    if key is None:
        key = os.environ["BINANCE_API_KEY"]
    if secret is None:
        secret = os.environ["BINANCE_API_SECRET"]

    client_key: str = hashlib.sha256("|".join((key, secret)).encode()).hexdigest()
    if client_key not in HTTP_CLIENTS:
        print("Creating new instance of BinanceHttpClient")  # TODO(cs): debugging
        client = BinanceHttpClient(
            loop=loop,
            clock=clock,
            logger=logger,
            key=key,
            secret=secret,
        )
        HTTP_CLIENTS[client_key] = client
    return HTTP_CLIENTS[client_key]


@lru_cache(1)
def get_cached_binance_instrument_provider(
    client: BinanceHttpClient,
    logger: Logger,
) -> BinanceInstrumentProvider:
    """
    Cache and return a BinanceInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.

    Returns
    -------
    BinanceInstrumentProvider

    """
    return BinanceInstrumentProvider(
        client=client,
        logger=logger,
    )


class BinanceLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `Betfair` live data client factory.
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
    ) -> BinanceDataClient:
        """
        Create a new Binance data client.

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
        BinanceDataClient

        """
        client = get_cached_binance_http_client(
            key=config.get("api_key"),
            secret=config.get("api_secret"),
            loop=loop,
            clock=clock,
            logger=logger,
        )

        # Get instrument provider singleton
        provider = get_cached_binance_instrument_provider(client=client, logger=logger)

        # Create client
        data_client = BinanceDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
        )
        return data_client


class BinanceLiveExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides data and execution clients for Betfair.
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
    ) -> BinanceSpotExecutionClient:
        """
        Create a new Binance execution client.

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
        BinanceSpotExecutionClient

        """
        client = get_cached_binance_http_client(
            key=config.get("api_key"),
            secret=config.get("api_secret"),
            loop=loop,
            clock=clock,
            logger=logger,
        )

        # Get instrument provider singleton
        provider = get_cached_binance_instrument_provider(client=client, logger=logger)

        # Get account ID env variable or set default
        account_id_env_var = os.getenv(config.get("account_id", ""), "001")

        # Set account ID
        account_id = AccountId(BINANCE_VENUE.value, account_id_env_var)

        # Create client
        exec_client = BinanceSpotExecutionClient(
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
