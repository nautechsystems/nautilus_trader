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
from typing import Dict, Optional

from nautilus_trader.adapters.ftx.config import FTXDataClientConfig
from nautilus_trader.adapters.ftx.config import FTXExecClientConfig
from nautilus_trader.adapters.ftx.data import FTXDataClient
from nautilus_trader.adapters.ftx.execution import FTXExecutionClient
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.msgbus.bus import MessageBus


HTTP_CLIENTS: Dict[str, FTXHttpClient] = {}


def get_cached_ftx_http_client(
    loop: asyncio.AbstractEventLoop,
    clock: LiveClock,
    logger: Logger,
    key: Optional[str] = None,
    secret: Optional[str] = None,
    subaccount: Optional[str] = None,
    us: bool = False,
) -> FTXHttpClient:
    """
    Cache and return a FTX HTTP client with the given key or secret.

    If a cached client with matching key and secret already exists, then that
    cached client will be returned.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    key : str, optional
        The API key for the client.
        If None then will source from the `FTX_API_KEY` env var.
    secret : str, optional
        The API secret for the client.
        If None then will source from the `FTX_API_SECRET` env var.
    subaccount : str, optional
        The subaccount name.
        If None then will source from the `FTX_SUBACCOUNT` env var.
    us : bool, default False
        If the client is for FTX US.

    Returns
    -------
    FTXHttpClient

    """
    global HTTP_CLIENTS

    key = key or os.environ["FTX_API_KEY"]
    secret = secret or os.environ["FTX_API_SECRET"]
    subaccount = subaccount or os.environ.get("FTX_SUBACCOUNT")

    client_key: str = "|".join((key, secret, subaccount or "None"))
    if client_key not in HTTP_CLIENTS:
        client = FTXHttpClient(
            loop=loop,
            clock=clock,
            logger=logger,
            key=key,
            secret=secret,
            subaccount=subaccount,
            us=us,
        )
        HTTP_CLIENTS[client_key] = client
    return HTTP_CLIENTS[client_key]


@lru_cache(1)
def get_cached_ftx_instrument_provider(
    client: FTXHttpClient,
    logger: Logger,
    config: InstrumentProviderConfig,
    override_usd: bool = False,
) -> FTXInstrumentProvider:
    """
    Cache and return an FTXInstrumentProvider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : FTXHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.
    override_usd : bool, default False
        If the built-in USD currency should be overridden with the FTX version
        which uses a precision of 8.

    Returns
    -------
    FTXInstrumentProvider

    """
    return FTXInstrumentProvider(
        client=client,
        logger=logger,
        config=config,
        override_usd=override_usd,
    )


class FTXLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides an `FTX` live data client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: FTXDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
    ) -> FTXDataClient:
        """
        Create a new FTX data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : FTXDataClientConfig
            The client configuration.
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
        FTXDataClient

        """
        client = get_cached_ftx_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            key=config.api_key,
            secret=config.api_secret,
            subaccount=config.subaccount,
            us=config.us,
        )

        # Get instrument provider singleton
        provider = get_cached_ftx_instrument_provider(
            client=client,
            logger=logger,
            config=config.instrument_provider,
            override_usd=config.override_usd,
        )

        # Create client
        data_client = FTXDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
            us=config.us,
        )
        return data_client


class FTXLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides an `FTX` live execution client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: FTXExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
    ) -> FTXExecutionClient:
        """
        Create a new FTX execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : FTXExecClientConfig
            The client configuration.
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
        FTXExecutionClient

        """
        client = get_cached_ftx_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            key=config.api_key,
            secret=config.api_secret,
            subaccount=config.subaccount,
            us=config.us,
        )

        # Get instrument provider singleton
        provider = get_cached_ftx_instrument_provider(
            client=client,
            logger=logger,
            config=config.instrument_provider,
            override_usd=config.override_usd,
        )

        # Create client
        exec_client = FTXExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
            us=config.us,
            account_polling_interval=config.account_polling_interval,
            calculated_account=config.calculated_account,
        )
        return exec_client
