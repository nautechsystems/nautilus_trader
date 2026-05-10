# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
import warnings
from functools import lru_cache

from nautilus_trader.adapters.binance.common.credentials import get_api_key
from nautilus_trader.adapters.binance.common.credentials import get_api_secret
from nautilus_trader.adapters.binance.common.credentials import is_ed25519_private_key
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.common.enums import BinanceKeyType
from nautilus_trader.adapters.binance.common.urls import get_http_base_url
from nautilus_trader.adapters.binance.common.urls import get_ws_base_url
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.config import BinanceInstrumentProviderConfig
from nautilus_trader.adapters.binance.futures.data import BinanceFuturesDataClient
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.data import BinanceSpotDataClient
from nautilus_trader.adapters.binance.spot.execution import BinanceSpotExecutionClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import Quota
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import Venue


def _resolve_environment(
    environment: BinanceEnvironment | None,
    testnet: bool,
) -> BinanceEnvironment:
    if environment is not None and testnet:
        raise ValueError(
            "Cannot set both `environment` and `testnet`. "
            "Use `environment` only (`testnet` is deprecated).",
        )

    if testnet:
        warnings.warn(
            "`testnet` is deprecated, use `environment=BinanceEnvironment.TESTNET` instead.",
            DeprecationWarning,
            stacklevel=3,
        )
        return BinanceEnvironment.TESTNET

    return environment or BinanceEnvironment.LIVE


@lru_cache(1)
def get_cached_binance_http_client(
    clock: LiveClock,
    account_type: BinanceAccountType,
    api_key: str | None = None,
    api_secret: str | None = None,
    key_type: BinanceKeyType = BinanceKeyType.HMAC,
    base_url: str | None = None,
    environment: BinanceEnvironment = BinanceEnvironment.LIVE,
    is_us: bool = False,
    proxy_url: str | None = None,
) -> BinanceHttpClient:
    """
    Cache and return a Binance HTTP client with the given key and secret.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    account_type : BinanceAccountType
        The account type for the client.
    api_key : str, optional
        The API key for the client.
        If ``None``, the client will work for public market data only.
    api_secret : str, optional
        The API secret for the client.
        If ``None``, the client will work for public market data only.
    key_type : BinanceKeyType, default 'HMAC'
        The private key cryptographic algorithm type.
    base_url : str, optional
        The base URL for the API endpoints.
    environment : BinanceEnvironment, default LIVE
        The Binance environment.
    is_us : bool, default False
        If the client is connecting to Binance US.
    proxy_url : str, optional
        The proxy URL for HTTP requests.

    Returns
    -------
    BinanceHttpClient

    """
    default_http_base_url = get_http_base_url(account_type, environment, is_us)

    # Determine key type: honor explicit RSA/ED25519, otherwise auto-detect
    rsa_private_key = None
    ed25519_private_key = None

    if key_type == BinanceKeyType.RSA:
        rsa_private_key = api_secret
    elif key_type == BinanceKeyType.ED25519 or (api_secret and is_ed25519_private_key(api_secret)):
        ed25519_private_key = api_secret

    # Set up rate limit quotas
    global_key = "binance:global"

    if account_type.is_spot:
        # Spot
        global_quota = Quota.rate_per_minute(6000)
        ratelimiter_default_quota = global_quota
        ratelimiter_quotas: list[tuple[str, Quota]] = [
            (global_key, global_quota),
            ("binance:api/v3/order", Quota.rate_per_minute(3000)),
            ("binance:api/v3/allOrders", Quota.rate_per_minute(int(3000 / 20))),
            ("binance:api/v3/klines", Quota.rate_per_minute(600)),
        ]
    else:
        # Futures
        global_quota = Quota.rate_per_minute(2400)
        ratelimiter_default_quota = global_quota
        ratelimiter_quotas = [
            (global_key, global_quota),
            ("binance:fapi/v1/order", Quota.rate_per_minute(1200)),
            ("binance:fapi/v1/allOrders", Quota.rate_per_minute(int(1200 / 20))),
            ("binance:fapi/v1/commissionRate", Quota.rate_per_minute(int(2400 / 20))),
            ("binance:fapi/v1/klines", Quota.rate_per_minute(600)),
        ]

    return BinanceHttpClient(
        clock=clock,
        api_key=api_key,
        api_secret=api_secret,
        rsa_private_key=rsa_private_key,
        ed25519_private_key=ed25519_private_key,
        base_url=base_url or default_http_base_url,
        ratelimiter_quotas=ratelimiter_quotas,
        ratelimiter_default_quota=ratelimiter_default_quota,
        proxy_url=proxy_url,
    )


@lru_cache(1)
def get_cached_binance_spot_instrument_provider(
    client: BinanceHttpClient,
    clock: LiveClock,
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    config: InstrumentProviderConfig,
    venue: Venue,
) -> BinanceSpotInstrumentProvider:
    """
    Cache and return an instrument provider for the Binance Spot/Margin exchange.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    clock : LiveClock
        The clock for the instrument provider.
    account_type : BinanceAccountType
        The Binance account type for the instrument provider.
    environment : BinanceEnvironment
        The Binance environment.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.
    venue : Venue
        The venue for the instrument provider.

    Returns
    -------
    BinanceSpotInstrumentProvider

    """
    return BinanceSpotInstrumentProvider(
        client=client,
        clock=clock,
        account_type=account_type,
        environment=environment,
        config=config,
        venue=venue,
    )


@lru_cache(1)
def get_cached_binance_futures_instrument_provider(
    client: BinanceHttpClient,
    clock: LiveClock,
    account_type: BinanceAccountType,
    config: InstrumentProviderConfig | BinanceInstrumentProviderConfig,
    venue: Venue,
) -> BinanceFuturesInstrumentProvider:
    """
    Cache and return an instrument provider for the Binance Futures exchange.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the instrument provider.
    clock : LiveClock
        The clock for the instrument provider.
    account_type : BinanceAccountType
        The Binance account type for the instrument provider.
    config : InstrumentProviderConfig | BinanceInstrumentProviderConfig
        The configuration for the instrument provider.
    venue : Venue
        The venue for the instrument provider.

    Returns
    -------
    BinanceFuturesInstrumentProvider

    """
    return BinanceFuturesInstrumentProvider(
        client=client,
        clock=clock,
        account_type=account_type,
        config=config,
        venue=venue,
    )


class BinanceLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Binance live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BinanceDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BinanceSpotDataClient | BinanceFuturesDataClient:
        """
        Create a new Binance data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BinanceDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BinanceSpotDataClient or BinanceFuturesDataClient

        Raises
        ------
        ValueError
            If `config.account_type` is not a valid `BinanceAccountType`.

        """
        environment = _resolve_environment(config.environment, config.testnet)

        # Get HTTP client singleton
        client: BinanceHttpClient = get_cached_binance_http_client(
            clock=clock,
            account_type=config.account_type,
            api_key=config.api_key,
            api_secret=config.api_secret,
            key_type=config.key_type,
            base_url=config.base_url_http,
            environment=environment,
            is_us=config.us,
            proxy_url=config.proxy_url,
        )

        default_base_url_ws: str = get_ws_base_url(
            account_type=config.account_type,
            environment=environment,
            is_us=config.us,
        )

        provider: BinanceSpotInstrumentProvider | BinanceFuturesInstrumentProvider
        if config.account_type.is_spot_or_margin:
            # Get instrument provider singleton
            provider = get_cached_binance_spot_instrument_provider(
                client=client,
                clock=clock,
                account_type=config.account_type,
                environment=environment,
                config=config.instrument_provider,
                venue=config.venue,
            )

            return BinanceSpotDataClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                instrument_provider=provider,
                account_type=config.account_type,
                base_url_ws=config.base_url_ws or default_base_url_ws,
                name=name,
                config=config,
            )
        else:
            # Get instrument provider singleton
            provider = get_cached_binance_futures_instrument_provider(
                client=client,
                clock=clock,
                account_type=config.account_type,
                config=config.instrument_provider,
                venue=config.venue,
            )

            return BinanceFuturesDataClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                instrument_provider=provider,
                account_type=config.account_type,
                base_url_ws=config.base_url_ws or default_base_url_ws,
                name=name,
                config=config,
            )


class BinanceLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Binance live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BinanceExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BinanceSpotExecutionClient | BinanceFuturesExecutionClient:
        """
        Create a new Binance execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BinanceExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BinanceExecutionClient

        Raises
        ------
        ValueError
            If `config.account_type` is not a valid `BinanceAccountType`.

        """
        if config.key_type == BinanceKeyType.RSA:
            raise ValueError(
                "RSA keys are not supported for Binance execution clients. "
                "Use Ed25519 or HMAC keys instead.",
            )

        environment = _resolve_environment(config.environment, config.testnet)

        api_key = config.api_key or get_api_key(config.account_type, environment)
        api_secret = config.api_secret or get_api_secret(config.account_type, environment)

        # Get HTTP client singleton
        client: BinanceHttpClient = get_cached_binance_http_client(
            clock=clock,
            account_type=config.account_type,
            api_key=api_key,
            api_secret=api_secret,
            key_type=config.key_type,
            base_url=config.base_url_http,
            environment=environment,
            is_us=config.us,
            proxy_url=config.proxy_url,
        )

        default_base_url_ws: str = get_ws_base_url(
            account_type=config.account_type,
            environment=environment,
            is_us=config.us,
        )

        provider: BinanceSpotInstrumentProvider | BinanceFuturesInstrumentProvider
        if config.account_type.is_spot or config.account_type.is_margin:
            # Get instrument provider singleton
            provider = get_cached_binance_spot_instrument_provider(
                client=client,
                clock=clock,
                account_type=config.account_type,
                environment=environment,
                config=config.instrument_provider,
                venue=config.venue,
            )

            return BinanceSpotExecutionClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                instrument_provider=provider,
                base_url_ws=config.base_url_ws or default_base_url_ws,
                account_type=config.account_type,
                name=name,
                config=config,
                environment=environment,
                api_key=api_key,
                api_secret=api_secret,
            )
        else:
            # Get instrument provider singleton
            provider = get_cached_binance_futures_instrument_provider(
                client=client,
                clock=clock,
                account_type=config.account_type,
                config=config.instrument_provider,
                venue=config.venue,
            )

            return BinanceFuturesExecutionClient(
                loop=loop,
                client=client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                instrument_provider=provider,
                base_url_ws=config.base_url_ws or default_base_url_ws,
                account_type=config.account_type,
                name=name,
                config=config,
                environment=environment,
                api_key=api_key,
                api_secret=api_secret,
            )
