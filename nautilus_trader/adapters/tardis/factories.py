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

from nautilus_trader.adapters.tardis.config import TardisDataClientConfig
from nautilus_trader.adapters.tardis.data import TardisDataClient
from nautilus_trader.adapters.tardis.providers import TardisInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import TardisHttpClient
from nautilus_trader.live.factories import LiveDataClientFactory


@lru_cache(1)
def get_tardis_http_client(
    api_key: str | None = None,
    base_url: str | None = None,
    timeout_secs: int = 60,
) -> TardisHttpClient:
    """
    Cache and return a Tardis HTTP client with the given key and secret.

    If a cached client with matching key and secret already exists, then that cached
    client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The Tardis API key for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    timeout_secs : int, default 60
        The timeout (seconds) for HTTP requests to Tardis.

    Returns
    -------
    TardisHttpClient

    """
    return TardisHttpClient(
        api_key=api_key,
        base_url=base_url,
        timeout_secs=timeout_secs,
    )


@lru_cache(1)
def get_tardis_instrument_provider(
    client: TardisHttpClient,
    config: InstrumentProviderConfig,
) -> TardisInstrumentProvider:
    """
    Cache and return a Tardis instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : TardisHttpClient
        The client for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.

    Returns
    -------
    TardisInstrumentProvider

    """
    return TardisInstrumentProvider(
        client=client,
        config=config,
    )


class TardisLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Tardis live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: TardisDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> TardisDataClient:
        """
        Create a new Tardis data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config :TardisDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        TardisDataClient

        """
        client: TardisHttpClient = get_tardis_http_client(
            api_key=config.api_key,
            base_url=config.base_url_http,
        )
        provider = get_tardis_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )
        return TardisDataClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
