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
from typing import Dict, Optional, Union

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.futures.data import BinanceFuturesDataClient
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.data import BinanceSpotDataClient
from nautilus_trader.adapters.binance.spot.execution import BinanceSpotExecutionClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.msgbus.bus import MessageBus


HTTP_CLIENTS: Dict[str, BinanceHttpClient] = {}


def get_cached_binance_http_client(
    loop: asyncio.AbstractEventLoop,
    clock: LiveClock,
    logger: Logger,
    account_type: BinanceAccountType,
    key: Optional[str] = None,
    secret: Optional[str] = None,
    base_url: Optional[str] = None,
    is_testnet: bool = False,
    is_us: bool = False,
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
    account_type : BinanceAccountType
        The account type for the client.
    key : str, optional
        The API key for the client.
    secret : str, optional
        The API secret for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    is_testnet : bool, default False
        If the client is connecting to the testnet API.
    is_us : bool, default False
        If the client is connecting to Binance US.

    Returns
    -------
    BinanceHttpClient

    """
    global HTTP_CLIENTS

    key = key or _get_api_key(account_type, is_testnet)
    secret = secret or _get_api_secret(account_type, is_testnet)
    default_http_base_url = _get_http_base_url(account_type, is_testnet, is_us)

    client_key: str = "|".join((key, secret))
    if client_key not in HTTP_CLIENTS:
        client = BinanceHttpClient(
            loop=loop,
            clock=clock,
            logger=logger,
            key=key,
            secret=secret,
            base_url=base_url or default_http_base_url,
        )
        HTTP_CLIENTS[client_key] = client
    return HTTP_CLIENTS[client_key]


@lru_cache(1)
def get_cached_binance_spot_instrument_provider(
    client: BinanceHttpClient,
    logger: Logger,
    account_type: BinanceAccountType,
    config: InstrumentProviderConfig,
) -> BinanceSpotInstrumentProvider:
    """
    Cache and return an instrument provider for the `Binance Spot/Margin` exchange.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.
    account_type : BinanceAccountType
        The Binance account type for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.

    Returns
    -------
    BinanceSpotInstrumentProvider

    """
    return BinanceSpotInstrumentProvider(
        client=client,
        logger=logger,
        account_type=account_type,
        config=config,
    )


@lru_cache(1)
def get_cached_binance_futures_instrument_provider(
    client: BinanceHttpClient,
    logger: Logger,
    account_type: BinanceAccountType,
    config: InstrumentProviderConfig,
) -> BinanceFuturesInstrumentProvider:
    """
    Cache and return an instrument provider for the `Binance Futures` exchange.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    logger : Logger
        The logger for the instrument provider.
    account_type : BinanceAccountType
        The Binance account type for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.

    Returns
    -------
    BinanceFuturesInstrumentProvider

    """
    return BinanceFuturesInstrumentProvider(
        client=client,
        logger=logger,
        account_type=account_type,
        config=config,
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
    ) -> Union[BinanceSpotDataClient, BinanceFuturesDataClient]:
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
        BinanceSpotDataClient or BinanceFuturesDataClient

        Raises
        ------
        ValueError
            If `config.account_type` is not a valid `BinanceAccountType`.

        """
        # Get HTTP client singleton
        client: BinanceHttpClient = get_cached_binance_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            account_type=config.account_type,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.testnet,
            is_us=config.us,
        )

        default_base_url_ws: str = _get_ws_base_url(
            account_type=config.account_type,
            is_testnet=config.testnet,
            is_us=config.us,
        )

        provider: Union[BinanceSpotInstrumentProvider, BinanceFuturesInstrumentProvider]
        if config.account_type.is_spot or config.account_type.is_margin:
            # Get instrument provider singleton
            provider = get_cached_binance_spot_instrument_provider(
                client=client,
                logger=logger,
                account_type=config.account_type,
                config=config.instrument_provider,
            )

            # Create client
            return BinanceSpotDataClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
                instrument_provider=provider,
                account_type=config.account_type,
                base_url_ws=config.base_url_ws or default_base_url_ws,
            )
        else:
            # Get instrument provider singleton
            provider = get_cached_binance_futures_instrument_provider(
                client=client,
                logger=logger,
                account_type=config.account_type,
                config=config.instrument_provider,
            )

            # Create client
            return BinanceFuturesDataClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
                instrument_provider=provider,
                account_type=config.account_type,
                base_url_ws=config.base_url_ws or default_base_url_ws,
            )


class BinanceLiveExecClientFactory(LiveExecClientFactory):
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
    ) -> Union[BinanceSpotExecutionClient, BinanceFuturesExecutionClient]:
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
        # Get HTTP client singleton
        client: BinanceHttpClient = get_cached_binance_http_client(
            loop=loop,
            clock=clock,
            logger=logger,
            account_type=config.account_type,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.testnet,
            is_us=config.us,
        )

        default_base_url_ws: str = _get_ws_base_url(
            account_type=config.account_type,
            is_testnet=config.testnet,
            is_us=config.us,
        )

        provider: Union[BinanceSpotInstrumentProvider, BinanceFuturesInstrumentProvider]
        if config.account_type.is_spot or config.account_type.is_margin:
            # Get instrument provider singleton
            provider = get_cached_binance_spot_instrument_provider(
                client=client,
                logger=logger,
                account_type=config.account_type,
                config=config.instrument_provider,
            )

            # Create client
            return BinanceSpotExecutionClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
                instrument_provider=provider,
                account_type=config.account_type,
                base_url_ws=config.base_url_ws or default_base_url_ws,
            )
        else:
            # Get instrument provider singleton
            provider = get_cached_binance_futures_instrument_provider(
                client=client,
                logger=logger,
                account_type=config.account_type,
                config=config.instrument_provider,
            )

            # Create client
            return BinanceFuturesExecutionClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
                instrument_provider=provider,
                account_type=config.account_type,
                base_url_ws=config.base_url_ws or default_base_url_ws,
            )


def _get_api_key(account_type: BinanceAccountType, is_testnet: bool) -> str:
    if is_testnet:
        if account_type.is_spot or account_type.is_margin:
            return os.environ["BINANCE_TESTNET_API_KEY"]
        else:
            return os.environ["BINANCE_FUTURES_TESTNET_API_KEY"]

    if account_type.is_spot or account_type.is_margin:
        return os.environ["BINANCE_API_KEY"]
    else:
        return os.environ["BINANCE_FUTURES_API_KEY"]


def _get_api_secret(account_type: BinanceAccountType, is_testnet: bool) -> str:
    if is_testnet:
        if account_type.is_spot or account_type.is_margin:
            return os.environ["BINANCE_TESTNET_API_SECRET"]
        else:
            return os.environ["BINANCE_FUTURES_TESTNET_API_SECRET"]

    if account_type.is_spot or account_type.is_margin:
        return os.environ["BINANCE_API_SECRET"]
    else:
        return os.environ["BINANCE_FUTURES_API_SECRET"]


def _get_http_base_url(account_type: BinanceAccountType, is_testnet: bool, is_us: bool) -> str:
    # Testnet base URLs
    if is_testnet:
        if account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
            return "https://testnet.binance.vision"
        elif account_type == BinanceAccountType.FUTURES_USDT:
            return "https://testnet.binancefuture.com"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            return "https://testnet.binancefuture.com"
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid Binance account type, was {account_type}")

    # Live base URLs
    top_level_domain: str = "us" if is_us else "com"
    if account_type == BinanceAccountType.SPOT:
        return f"https://api.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.MARGIN:
        return f"https://sapi.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.FUTURES_USDT:
        return f"https://fapi.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.FUTURES_COIN:
        return f"https://dapi.binance.{top_level_domain}"
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"invalid Binance account type, was {account_type}")


def _get_ws_base_url(account_type: BinanceAccountType, is_testnet: bool, is_us: bool) -> str:
    # Testnet base URLs
    if is_testnet:
        if account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
            return "wss://testnet.binance.vision"
        elif account_type == BinanceAccountType.FUTURES_USDT:
            return "wss://stream.binancefuture.com"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            raise ValueError("no testnet for COIN-M futures")
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid Binance account type, was {account_type}")

    # Live base URLs
    top_level_domain: str = "us" if is_us else "com"
    if account_type in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
        return f"wss://stream.binance.{top_level_domain}:9443"
    elif account_type == BinanceAccountType.FUTURES_USDT:
        return f"wss://fstream.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.FUTURES_COIN:
        return f"wss://dstream.binance.{top_level_domain}"
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"invalid Binance account type, was {account_type}")
