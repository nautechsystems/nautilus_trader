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

from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxDataClientConfig
from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxExecClientConfig
from nautilus_trader.adapters.coinbase_intx.data import CoinbaseIntxDataClient
from nautilus_trader.adapters.coinbase_intx.execution import CoinbaseIntxExecutionClient
from nautilus_trader.adapters.coinbase_intx.providers import CoinbaseIntxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_coinbase_intx_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    api_passphrase: str | None = None,
    base_url: str | None = None,
    timeout_secs: int = 60,
) -> nautilus_pyo3.CoinbaseIntxHttpClient:
    """
    Cache and return a Coinbase International HTTP client with the given key and secret.

    If a cached client with matching key and secret already exists, then that cached
    client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The Coinbase International API key for the client.
    api_secret : str, optional
        The Coinbase International API secret for the client.
    api_passphrase : str, optional
        The Coinbase International API passphrase for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    timeout_secs : int, default 60
        The timeout (seconds) for HTTP requests to Coinbase Intx.

    Returns
    -------
    CoinbaseIntxHttpClient

    """
    return nautilus_pyo3.CoinbaseIntxHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        api_passphrase=api_passphrase,
        base_url=base_url,
        timeout_secs=timeout_secs,
    )


@lru_cache(1)
def get_coinbase_intx_instrument_provider(
    client: nautilus_pyo3.CoinbaseIntxHttpClient,
    config: InstrumentProviderConfig,
) -> CoinbaseIntxInstrumentProvider:
    """
    Cache and return a Coinbase International instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : CoinbaseIntxHttpClient
        The client for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.

    Returns
    -------
    CoinbaseIntxInstrumentProvider

    """
    return CoinbaseIntxInstrumentProvider(
        client=client,
        config=config,
    )


class CoinbaseIntxLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Coinbase International live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: CoinbaseIntxDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> CoinbaseIntxDataClient:
        """
        Create a new Coinbase International data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : CoinbaseIntxDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        CoinbaseIntxHttpClient

        """
        client: nautilus_pyo3.CoinbaseIntxHttpClient = get_coinbase_intx_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            base_url=config.base_url_http,
        )
        provider = get_coinbase_intx_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )

        return CoinbaseIntxDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class CoinbaseIntxLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Coinbase International live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: CoinbaseIntxExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> CoinbaseIntxExecutionClient:
        """
        Create a new Coinbase International execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : CoinbaseIntxExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        CoinbaseIntxExecutionClient

        """
        client: nautilus_pyo3.CoinbaseIntxHttpClient = get_coinbase_intx_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            base_url=config.base_url_http,
        )
        provider = get_coinbase_intx_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )

        return CoinbaseIntxExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
