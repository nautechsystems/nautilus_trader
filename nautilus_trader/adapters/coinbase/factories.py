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
"""Factories for Coinbase clients."""

import asyncio
import os

import nautilus_pyo3
from nautilus_trader.adapters.coinbase.config import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase.config import CoinbaseExecClientConfig
from nautilus_trader.adapters.coinbase.data import CoinbaseDataClient
from nautilus_trader.adapters.coinbase.execution import CoinbaseExecutionClient
from nautilus_trader.adapters.coinbase.providers import CoinbaseInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


def get_coinbase_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    timeout_secs: int | None = None,
) -> nautilus_pyo3.CoinbaseHttpClient:
    """
    Create a Coinbase HTTP client.

    Parameters
    ----------
    api_key : str, optional
        The API key. If None, will use COINBASE_API_KEY environment variable.
    api_secret : str, optional
        The API secret. If None, will use COINBASE_API_SECRET environment variable.
    base_url : str, optional
        The base URL for the API.
    timeout_secs : int, optional
        The request timeout in seconds.

    Returns
    -------
    nautilus_pyo3.CoinbaseHttpClient

    """
    api_key = api_key or os.getenv("COINBASE_API_KEY")
    api_secret = api_secret or os.getenv("COINBASE_API_SECRET")

    if not api_key:
        raise ValueError("API key is required (set COINBASE_API_KEY environment variable)")
    if not api_secret:
        raise ValueError("API secret is required (set COINBASE_API_SECRET environment variable)")

    return nautilus_pyo3.CoinbaseHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        timeout_secs=timeout_secs,
    )


def get_coinbase_websocket_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    ws_url: str | None = None,
) -> nautilus_pyo3.CoinbaseWebSocketClient:
    """
    Create a Coinbase WebSocket client.

    Parameters
    ----------
    api_key : str, optional
        The API key. If None, will use COINBASE_API_KEY environment variable.
    api_secret : str, optional
        The API secret. If None, will use COINBASE_API_SECRET environment variable.
    ws_url : str, optional
        The WebSocket URL.

    Returns
    -------
    nautilus_pyo3.CoinbaseWebSocketClient

    """
    api_key = api_key or os.getenv("COINBASE_API_KEY")
    api_secret = api_secret or os.getenv("COINBASE_API_SECRET")

    if not api_key:
        raise ValueError("API key is required (set COINBASE_API_KEY environment variable)")
    if not api_secret:
        raise ValueError("API secret is required (set COINBASE_API_SECRET environment variable)")

    return nautilus_pyo3.CoinbaseWebSocketClient(
        api_key=api_key,
        api_secret=api_secret,
        ws_url=ws_url,
    )


def get_coinbase_instrument_provider(
    client: nautilus_pyo3.CoinbaseHttpClient,
) -> CoinbaseInstrumentProvider:
    """
    Create a Coinbase instrument provider.

    Parameters
    ----------
    client : nautilus_pyo3.CoinbaseHttpClient
        The HTTP client.

    Returns
    -------
    CoinbaseInstrumentProvider

    """
    return CoinbaseInstrumentProvider(client=client)


class CoinbaseLiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for creating Coinbase data clients.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: CoinbaseDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> CoinbaseDataClient:
        """
        Create a Coinbase data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop.
        name : str
            The client name.
        config : CoinbaseDataClientConfig
            The configuration.
        msgbus : MessageBus
            The message bus.
        cache : Cache
            The cache.
        clock : LiveClock
            The clock.

        Returns
        -------
        CoinbaseDataClient

        """
        http_client = get_coinbase_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            timeout_secs=config.http_timeout_secs,
        )

        ws_client = get_coinbase_websocket_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            ws_url=config.base_url_ws,
        )

        provider = get_coinbase_instrument_provider(client=http_client)

        return CoinbaseDataClient(
            loop=loop,
            client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            ws_client=ws_client,
        )


class CoinbaseLiveExecClientFactory(LiveExecClientFactory):
    """
    Factory for creating Coinbase execution clients.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: CoinbaseExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> CoinbaseExecutionClient:
        """
        Create a Coinbase execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop.
        name : str
            The client name.
        config : CoinbaseExecClientConfig
            The configuration.
        msgbus : MessageBus
            The message bus.
        cache : Cache
            The cache.
        clock : LiveClock
            The clock.

        Returns
        -------
        CoinbaseExecutionClient

        """
        http_client = get_coinbase_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            timeout_secs=config.http_timeout_secs,
        )

        ws_client = get_coinbase_websocket_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            ws_url=config.base_url_ws,
        )

        provider = get_coinbase_instrument_provider(client=http_client)

        return CoinbaseExecutionClient(
            loop=loop,
            client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            ws_client=ws_client,
        )

