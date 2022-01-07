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
from typing import Any, Dict, Optional

from nautilus_trader.adapters.ftx.common import FTX_VENUE
from nautilus_trader.adapters.ftx.data import FTXDataClient
from nautilus_trader.adapters.ftx.execution import FTXExecutionClient
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecutionClientFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus


HTTP_CLIENTS: Dict[str, FTXHttpClient] = {}


def get_cached_ftx_http_client(
    key: Optional[str],
    secret: Optional[str],
    subaccount_name: Optional[str],
    loop: asyncio.AbstractEventLoop,
    clock: LiveClock,
    logger: Logger,
) -> FTXHttpClient:
    """
    Cache and return a FTX HTTP client with the given key or secret.

    If a cached client with matching key and secret already exists, then that
    cached client will be returned.

    Parameters
    ----------
    key : str, optional
        The API key for the client.
        If None then will source from the `FTX_API_KEY` env var.
    secret : str, optional
        The API secret for the client.
        If None then will source from the `FTX_API_SECRET` env var.
    subaccount_name : str, optional
        The sub-account name.
        If None then will source from the `FTX_SUB_ACCOUNT` env var.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.

    Returns
    -------
    FTXHttpClient

    """
    global HTTP_CLIENTS

    key = key or os.environ["FTX_API_KEY"]
    secret = secret or os.environ["FTX_API_SECRET"]
    subaccount_name = subaccount_name or os.environ["FTX_SUB_ACCOUNT"]

    client_key: str = "|".join((key, secret))
    if client_key not in HTTP_CLIENTS:
        client = FTXHttpClient(
            loop=loop,
            clock=clock,
            logger=logger,
            key=key,
            secret=secret,
            subaccount_name=subaccount_name,
        )
        HTTP_CLIENTS[client_key] = client
    return HTTP_CLIENTS[client_key]


@lru_cache(1)
def get_cached_ftx_instrument_provider(
    client: FTXHttpClient,
    logger: Logger,
) -> FTXInstrumentProvider:
    """
    Cache and return an FTXInstrumentProvider.

    If a cached provider already exists, then that cached provider will be returned.

    Parameters
    ----------
    client : FTXHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.

    Returns
    -------
    FTXInstrumentProvider

    """
    return FTXInstrumentProvider(
        client=client,
        logger=logger,
    )


class FTXLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides an `FTX` live data client factory.
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
    ) -> FTXDataClient:
        """
        Create a new FTX data client.

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

        Returns
        -------
        FTXDataClient

        """
        client = get_cached_ftx_http_client(
            key=config.get("api_key"),
            secret=config.get("api_secret"),
            subaccount_name=config.get("sub_account"),
            loop=loop,
            clock=clock,
            logger=logger,
        )

        # Get instrument provider singleton
        provider = get_cached_ftx_instrument_provider(client=client, logger=logger)

        # Create client
        data_client = FTXDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
        )
        return data_client


class FTXLiveExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides an `FTX` live execution client factory.
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
    ) -> FTXExecutionClient:
        """
        Create a new FTX execution client.

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

        Returns
        -------
        FTXExecutionClient

        """
        client = get_cached_ftx_http_client(
            key=config.get("api_key"),
            secret=config.get("api_secret"),
            subaccount_name=config.get("sub_account"),
            loop=loop,
            clock=clock,
            logger=logger,
        )

        # Get instrument provider singleton
        provider = get_cached_ftx_instrument_provider(client=client, logger=logger)

        # Get account ID env variable or set default
        account_id_env_var = os.getenv(config.get("account_id", ""), "001")

        # Set account ID
        account_id = AccountId(FTX_VENUE.value, account_id_env_var)

        # Create client
        exec_client = FTXExecutionClient(
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
