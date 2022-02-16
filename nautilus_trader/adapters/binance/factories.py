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

from nautilus_trader.adapters.binance.common import BinanceAccountType
from nautilus_trader.adapters.binance.data import BinanceDataClient
from nautilus_trader.adapters.binance.execution import BinanceExecutionClient
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecutionClientFactory
from nautilus_trader.msgbus.bus import MessageBus


HTTP_CLIENTS: Dict[str, BinanceHttpClient] = {}


def get_cached_binance_http_client(
    loop: asyncio.AbstractEventLoop,
    clock: LiveClock,
    logger: Logger,
    key: Optional[str] = None,
    secret: Optional[str] = None,
    base_url: Optional[str] = None,
) -> BinanceHttpClient:
    """
    Cache and return a Binance HTTP client with the given key and secret.

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
        If None then will source from the `BINANCE_API_KEY` env var.
    secret : str, optional
        The API secret for the client.
        If None then will source from the `BINANCE_API_SECRET` env var.
    base_url : str, optional
        The base URL for the API endpoints.

    Returns
    -------
    BinanceHttpClient

    """
    global HTTP_CLIENTS

    key = key or os.environ["BINANCE_API_KEY"]
    secret = secret or os.environ["BINANCE_API_SECRET"]

    client_key: str = "|".join((key, secret))
    if client_key not in HTTP_CLIENTS:
        client = BinanceHttpClient(
            loop=loop,
            clock=clock,
            logger=logger,
            key=key,
            secret=secret,
            base_url=base_url,
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
    Provides a `Binance` live data client factory.
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

        Returns
        -------
        BinanceDataClient

        Raises
        ------
        ValueError
            If `config.account_type` is not a valid `BinanceAccountType`.

        """
        account_type = BinanceAccountType(config.get("account_type", "SPOT").upper())
        base_url_http_default: str = _get_http_base_url(account_type, config.get("us", False))
        base_url_ws_default: str = _get_ws_base_url(account_type, config.get("us", False))

        client: BinanceHttpClient = get_cached_binance_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            key=config.get("api_key"),
            secret=config.get("api_secret"),
            base_url=config.get("base_url_http") or base_url_http_default,
        )

        # Get instrument provider singleton
        provider: BinanceInstrumentProvider = get_cached_binance_instrument_provider(
            client=client,
            logger=logger,
        )

        # Create client
        data_client = BinanceDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
            account_type=account_type,
            base_url_ws=config.get("base_url_ws") or base_url_ws_default,
        )
        return data_client


class BinanceLiveExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides a `Binance` live execution client factory.
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
    ) -> BinanceExecutionClient:
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

        Returns
        -------
        BinanceExecutionClient

        Raises
        ------
        ValueError
            If `config.account_type` is not a valid `BinanceAccountType`.

        """
        account_type = BinanceAccountType(config.get("account_type", "SPOT").upper())
        base_url_http_default: str = _get_http_base_url(account_type, config.get("us", False))
        base_url_ws_default: str = _get_ws_base_url(account_type, config.get("us", False))

        client: BinanceHttpClient = get_cached_binance_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            key=config.get("api_key"),
            secret=config.get("api_secret"),
            base_url=config.get("base_url_http") or base_url_http_default,
        )

        # Get instrument provider singleton
        provider: BinanceInstrumentProvider = get_cached_binance_instrument_provider(
            client=client,
            logger=logger,
        )

        # Create client
        exec_client = BinanceExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
            account_type=account_type,
            base_url_ws=config.get("base_url_ws") or base_url_ws_default,
        )
        return exec_client


def _get_http_base_url(account_type: BinanceAccountType, us: bool) -> str:
    top_level_domain: str = "us" if us else "com"
    if account_type == BinanceAccountType.MARGIN:
        return f"https://sapi.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.FUTURES_USDT:
        return f"https://fapi.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.FUTURES_COIN:
        return f"https://dapi.binance.{top_level_domain}"
    else:
        return f"https://api.binance.{top_level_domain}"  # SPOT


def _get_ws_base_url(account_type: BinanceAccountType, us: bool) -> str:
    top_level_domain: str = "us" if us else "com"
    if account_type == BinanceAccountType.MARGIN:
        return f"wss://stream.binance.{top_level_domain}:9443"  # SPOT
    elif account_type == BinanceAccountType.FUTURES_USDT:
        return f"wss://fstream.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.FUTURES_COIN:
        return f"wss://dstream.binance.{top_level_domain}"
    else:
        return f"wss://stream.binance.{top_level_domain}:9443"  # SPOT
