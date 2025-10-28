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

from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.data import OKXDataClient
from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_okx_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    api_passphrase: str | None = None,
    base_url: str | None = None,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
    is_demo: bool = False,
) -> nautilus_pyo3.OKXHttpClient:
    """
    Cache and return a OKX HTTP client with the given key and secret.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The API key for the client.
    api_secret : str, optional
        The API secret for the client.
    api_passphrase : str, optional
        The passphrase used to create the API key.
    base_url : str, optional
        The base URL for the API endpoints.
    timeout_secs : int, optional
        The timeout (seconds) for HTTP requests to OKX.
    max_retries : int, optional
        The maximum retry attempts for requests.
    retry_delay_ms : int, optional
        The initial delay (milliseconds) for retries.
    retry_delay_max_ms : int, optional
        The maximum delay (milliseconds) for exponential backoff.
    is_demo : bool, default False
        If the client is for the OKX demo API.

    Returns
    -------
    OKXHttpClient

    """
    return nautilus_pyo3.OKXHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        api_passphrase=api_passphrase,
        base_url=base_url,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
        is_demo=is_demo,
    )


@lru_cache(1)
def get_cached_okx_instrument_provider(
    client: nautilus_pyo3.OKXHttpClient,
    instrument_types: tuple[OKXInstrumentType, ...],
    contract_types: tuple[OKXContractType, ...] | None = None,
    instrument_families: tuple[str, ...] | None = None,
    config: InstrumentProviderConfig | None = None,
) -> OKXInstrumentProvider:
    """
    Cache and return a OKX instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : OKXHttpClient
        The OKX HTTP client.
    instrument_types : tuple[OKXInstrumentType, ...]
        The product types to load.
    contract_types : tuple[OKXInstrumentType, ...], optional
        The contract types of instruments to load.
    instrument_families : tuple[str, ...], optional
        The instrument families to load (e.g., "BTC-USD", "ETH-USD").
        Required for OPTIONS. Optional for FUTURES/SWAP.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    Returns
    -------
    OKXInstrumentProvider

    """
    return OKXInstrumentProvider(
        client=client,
        instrument_types=instrument_types,
        contract_types=contract_types,
        instrument_families=instrument_families,
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
        client: nautilus_pyo3.OKXHttpClient = get_cached_okx_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            base_url=config.base_url_http,
            is_demo=config.is_demo,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
        )
        provider = get_cached_okx_instrument_provider(
            client=client,
            instrument_types=config.instrument_types,
            contract_types=config.contract_types,
            instrument_families=config.instrument_families,
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
        client: nautilus_pyo3.OKXHttpClient = get_cached_okx_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            base_url=config.base_url_http,
            is_demo=config.is_demo,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
        )
        provider = get_cached_okx_instrument_provider(
            client=client,
            instrument_types=config.instrument_types,
            contract_types=config.contract_types,
            instrument_families=config.instrument_families,
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
