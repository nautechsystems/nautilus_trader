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
from typing import Dict, List, Optional, Union

from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.core.enums import BinanceAccountType
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
    is_testnet: bool = False,
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
    is_testnet : bool, default False
        If the client is connecting to the testnet API.

    Returns
    -------
    BinanceHttpClient

    """
    global HTTP_CLIENTS

    if is_testnet:
        key = key or os.environ["BINANCE_TESTNET_API_KEY"]
        secret = secret or os.environ["BINANCE_TESTNET_API_SECRET"]
    else:
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
    account_type: BinanceAccountType,
    load_all_on_start: bool = True,
    load_ids_on_start: Optional[List[str]] = None,
    filters: Optional[Dict] = None,
) -> BinanceInstrumentProvider:
    """
    Cache and return a BinanceInstrumentProvider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.
    account_type : BinanceAccountType
        The Binance account type for the instrument provider.
    load_all_on_start : bool, default False
        If all venue instruments should be loaded on start.
    load_ids_on_start : List[str], optional
        The list of instrument IDs to be loaded on start (if `load_all_instruments` is False).
    filters : Dict, optional
        The venue specific instrument loading filters to apply.

    Returns
    -------
    BinanceInstrumentProvider

    """
    return BinanceInstrumentProvider(
        client=client,
        logger=logger,
        account_type=account_type,
        load_all_on_start=load_all_on_start,
        load_ids_on_start=load_ids_on_start,
        filters=filters,
    )


class BinanceLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `Binance` live data client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BinanceDataClientConfig,
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
        config : BinanceDataClientConfig
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
        BinanceDataClient

        Raises
        ------
        ValueError
            If `config.account_type` is not a valid `BinanceAccountType`.

        """
        base_url_http_default: str = _get_http_base_url(config)
        base_url_ws_default: str = _get_ws_base_url(config)

        client: BinanceHttpClient = get_cached_binance_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http or base_url_http_default,
            is_testnet=config.testnet,
        )

        # Get instrument provider singleton
        provider: BinanceInstrumentProvider = get_cached_binance_instrument_provider(
            client=client,
            logger=logger,
            account_type=config.account_type,
            load_all_on_start=config.instrument_provider.load_all,
            load_ids_on_start=config.instrument_provider.load_ids,
            filters=config.instrument_provider.filters,
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
            account_type=config.account_type,
            base_url_ws=config.base_url_ws or base_url_ws_default,
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
        config: BinanceExecClientConfig,
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
        config : BinanceExecClientConfig
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
        base_url_http_default: str = _get_http_base_url(config)
        base_url_ws_default: str = _get_ws_base_url(config)

        client: BinanceHttpClient = get_cached_binance_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http or base_url_http_default,
            is_testnet=config.testnet,
        )

        # Get instrument provider singleton
        provider: BinanceInstrumentProvider = get_cached_binance_instrument_provider(
            client=client,
            logger=logger,
            account_type=config.account_type,
            load_all_on_start=config.instrument_provider.load_all,
            load_ids_on_start=config.instrument_provider.load_ids,
            filters=config.instrument_provider.filters,
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
            account_type=config.account_type,
            base_url_ws=config.base_url_ws or base_url_ws_default,
        )
        return exec_client


def _get_http_base_url(config: Union[BinanceDataClientConfig, BinanceExecClientConfig]) -> str:
    # Testnet base URLs
    if config.testnet:
        if config.account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
            return "https://testnet.binance.vision/api"
        elif config.account_type == BinanceAccountType.FUTURES_USDT:
            return "https://testnet.binancefuture.com"
        elif config.account_type == BinanceAccountType.FUTURES_COIN:
            raise ValueError("no testnet for COIN-M futures")
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid Binance account type, was {config.account_type}")

    # Live base URLs
    top_level_domain: str = "us" if config.us else "com"
    if config.account_type == BinanceAccountType.SPOT:
        return f"https://api.binance.{top_level_domain}"
    elif config.account_type == BinanceAccountType.MARGIN:
        return f"https://sapi.binance.{top_level_domain}"
    elif config.account_type == BinanceAccountType.FUTURES_USDT:
        return f"https://fapi.binance.{top_level_domain}"
    elif config.account_type == BinanceAccountType.FUTURES_COIN:
        return f"https://dapi.binance.{top_level_domain}"
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"invalid Binance account type, was {config.account_type}")


def _get_ws_base_url(config: Union[BinanceDataClientConfig, BinanceExecClientConfig]) -> str:
    # Testnet base URLs
    if config.testnet:
        if config.account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
            return "wss://testnet.binance.vision/ws"
        elif config.account_type == BinanceAccountType.FUTURES_USDT:
            return "wss://stream.binancefuture.com"
        elif config.account_type == BinanceAccountType.FUTURES_COIN:
            raise ValueError("no testnet for COIN-M futures")
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid Binance account type, was {config.account_type}")

    # Live base URLs
    top_level_domain: str = "us" if config.us else "com"
    if config.account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
        return f"wss://stream.binance.{top_level_domain}:9443"
    elif config.account_type == BinanceAccountType.FUTURES_USDT:
        return f"wss://fstream.binance.{top_level_domain}"
    elif config.account_type == BinanceAccountType.FUTURES_COIN:
        return f"wss://dstream.binance.{top_level_domain}"
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"invalid Binance account type, was {config.account_type}")
