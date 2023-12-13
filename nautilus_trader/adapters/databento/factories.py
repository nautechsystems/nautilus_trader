# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import databento

from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.data import DatabentoDataClient
from nautilus_trader.adapters.env import get_env_key
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.logging import Logger
from nautilus_trader.live.factories import LiveDataClientFactory


DATABENTO_HTTP_CLIENTS: dict[str, databento.Historical] = {}


@lru_cache(1)
def get_cached_databento_http_client(
    key: str | None = None,
    gateway: str | None = None,
) -> databento.Historical:
    """
    Cache and return a Databento historical HTTP client with the given key and gateway.

    If a cached client with matching key and gateway already exists, then that
    cached client will be returned.

    Parameters
    ----------
    key : str, optional
        The Databento API secret key for the client.
    gateway : str, optional
        The Databento historical HTTP client gateway override.

    Returns
    -------
    databento.Historical

    """
    global BINANCE_HTTP_CLIENTS

    key = key or get_env_key("DATABENTO_API_KEY")

    client_key: str = "|".join((key, gateway or ""))
    if client_key not in DATABENTO_HTTP_CLIENTS:
        client = databento.Historical(key=key, gateway=gateway or databento.HistoricalGateway.BO1)
        DATABENTO_HTTP_CLIENTS[client_key] = client
    return DATABENTO_HTTP_CLIENTS[client_key]


class DatabentoLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `Binance` live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DatabentoDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
    ) -> DatabentoDataClient:
        """
        Create a new Databento data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client name.
        config : DatabentoDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        Returns
        -------
        DatabentoDataClient

        """
        # Get HTTP client singleton
        http_client: databento.Historical = get_cached_databento_http_client(
            key=config.api_key,
            gateway=config.http_gateway,
        )

        return DatabentoDataClient(
            loop=loop,
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )
