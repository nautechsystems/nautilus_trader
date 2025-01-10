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
from functools import lru_cache

from nautilus_trader.adapters.okx.common.credentials import get_api_key
from nautilus_trader.adapters.okx.common.credentials import get_api_secret
from nautilus_trader.adapters.okx.common.credentials import get_passphrase
from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.urls import get_http_base_url
from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.data import OKXDataClient
from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import Quota
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_okx_http_client(
    clock: LiveClock,
    key: str | None = None,
    secret: str | None = None,
    passphrase: str | None = None,
    base_url: str | None = None,
    is_demo: bool = False,
) -> OKXHttpClient:
    """
    Cache and return a OKX HTTP client with the given key and secret.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    key : str, optional
        The API key for the client.
    secret : str, optional
        The API secret for the client.
    passphrase : str, optional
        The passphrase used to create the API key.
    base_url : str, optional
        The base URL for the API endpoints.
    is_demo : bool, default False
        If the client is connecting to the demo API.

    Returns
    -------
    OKXHttpClient

    """
    key = key or get_api_key(is_demo)
    secret = secret or get_api_secret(is_demo)
    passphrase = passphrase or get_passphrase(is_demo)
    base_url = base_url or get_http_base_url()

    # Setup rate limit quotas
    # OXX rate limits vary by endpoint, but rough average seems to be about 10 requests/second
    # https://www.okx.com/docs-v5/en/#overview-rate-limits
    ratelimiter_default_quota = Quota.rate_per_second(10)
    # ratelimiter_quotas: list[tuple[str, Quota]] = [
    #     ("api/v5/account/balance", Quota.rate_per_second(10 // 2)),
    #     ("api/v5/account/positions", Quota.rate_per_second(10 // 2)),
    #     ("api/v5/account/trade-fee", Quota.rate_per_second(5 // 2)),
    #     ("api/v5/market/books", Quota.rate_per_second(40 // 2)),
    #     ("api/v5/public/instruments", Quota.rate_per_second(20 // 2)),
    #     ("api/v5/public/position-tiers", Quota.rate_per_second(10 // 2)),
    #     ("api/v5/trade/amend-order", Quota.rate_per_second(60 // 2)),
    #     ("api/v5/trade/cancel-order", Quota.rate_per_second(60 // 2)),
    #     ("api/v5/trade/close-position", Quota.rate_per_second(20 // 2)),
    #     ("api/v5/trade/fills-history", Quota.rate_per_second(10 // 2)),
    #     ("api/v5/trade/fills", Quota.rate_per_second(60 // 2)),
    #     ("api/v5/trade/order", Quota.rate_per_second(60 // 2)),  # order-details
    #     ("api/v5/trade/orders-history", Quota.rate_per_second(40 // 2)),
    #     ("api/v5/trade/orders-pending", Quota.rate_per_second(60 // 2)),
    #     ("api/v5/trade/place-order", Quota.rate_per_second(60 // 2)),
    # ]
    ratelimiter_quotas = None

    return OKXHttpClient(
        clock=clock,
        api_key=key,
        api_secret=secret,
        passphrase=passphrase,
        base_url=base_url,
        is_demo=is_demo,
        default_timeout_secs=5,
        ratelimiter_quotas=ratelimiter_quotas,
        ratelimiter_default_quota=ratelimiter_default_quota,
    )


@lru_cache(1)
def get_cached_okx_instrument_provider(
    client: OKXHttpClient,
    clock: LiveClock,
    instrument_types: tuple[OKXInstrumentType],
    contract_types: tuple[OKXContractType],
    config: InstrumentProviderConfig,
) -> OKXInstrumentProvider:
    """
    Cache and return a OKX instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : OKXHttpClient
        The OKX HTTP client.
    clock : LiveClock
        The clock instance.
    instrument_types : tuple[OKXInstrumentType]
        The product types to load.
    contract_types : tuple[OKXInstrumentType]
        The contract types of instruments to load.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    Returns
    -------
    OKXInstrumentProvider

    """
    return OKXInstrumentProvider(
        client=client,
        clock=clock,
        instrument_types=tuple(instrument_types),  # type: ignore
        contract_types=tuple(contract_types),  # type: ignore
        config=config,
    )


class OKXLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a OKX live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: OKXDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> OKXDataClient:
        """
        Create a new OKX data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : OKXDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        OKXDataClient

        """
        instrument_types = config.instrument_types or tuple(OKXInstrumentType)
        contract_types = config.contract_types or tuple(OKXContractType)
        client: OKXHttpClient = get_cached_okx_http_client(
            clock=clock,
            key=config.api_key,
            secret=config.api_secret,
            passphrase=config.passphrase,
            base_url=config.base_url_http,
            is_demo=config.is_demo,
        )
        provider = get_cached_okx_instrument_provider(
            client=client,
            clock=clock,
            instrument_types=instrument_types,
            contract_types=contract_types,
            config=config.instrument_provider,
        )
        return OKXDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class OKXLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a OKX live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: OKXExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> OKXExecutionClient:
        """
        Create a new OKX execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : OKXExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        OKXExecutionClient

        """
        instrument_types = config.instrument_types or tuple(OKXInstrumentType)
        contract_types = config.contract_types or tuple(OKXContractType)
        client: OKXHttpClient = get_cached_okx_http_client(
            clock=clock,
            key=config.api_key,
            secret=config.api_secret,
            passphrase=config.passphrase,
            base_url=config.base_url_http,
            is_demo=config.is_demo,
        )
        provider = get_cached_okx_instrument_provider(
            client=client,
            clock=clock,
            instrument_types=instrument_types,
            contract_types=contract_types,
            config=config.instrument_provider,
        )
        return OKXExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
