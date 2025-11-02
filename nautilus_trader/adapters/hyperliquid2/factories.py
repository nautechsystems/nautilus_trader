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

from nautilus_trader.adapters.hyperliquid2.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid2.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid2.data import HyperliquidDataClient
from nautilus_trader.adapters.hyperliquid2.execution import HyperliquidExecutionClient
from nautilus_trader.adapters.hyperliquid2.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_hyperliquid_http_client(
    private_key: str | None = None,
    wallet_address: str | None = None,
    base_url: str | None = None,
    testnet: bool = False,
) -> nautilus_pyo3.HyperliquidHttpClient:
    """
    Cache and return a Hyperliquid HTTP client with the given private key.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    private_key : str, optional
        The Hyperliquid private key for authentication.
    wallet_address : str, optional
        The Hyperliquid wallet address.
    base_url : str, optional
        The base URL for the Hyperliquid HTTP API.
    testnet : bool, default False
        If client should connect to testnet.

    Returns
    -------
    nautilus_pyo3.HyperliquidHttpClient

    """
    return nautilus_pyo3.HyperliquidHttpClient(
        base_url=base_url,
        private_key=private_key,
        wallet_address=wallet_address,
        testnet=testnet,
    )


@lru_cache(1)
def get_cached_hyperliquid_websocket_client(
    private_key: str | None = None,
    wallet_address: str | None = None,
    base_url: str | None = None,
    testnet: bool = False,
) -> nautilus_pyo3.HyperliquidWebSocketClient:
    """
    Cache and return a Hyperliquid WebSocket client with the given private key.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    private_key : str, optional
        The Hyperliquid private key for authentication.
    wallet_address : str, optional
        The Hyperliquid wallet address.
    base_url : str, optional
        The base URL for the Hyperliquid WebSocket API.
    testnet : bool, default False
        If client should connect to testnet.

    Returns
    -------
    nautilus_pyo3.HyperliquidWebSocketClient

    """
    return nautilus_pyo3.HyperliquidWebSocketClient(
        url=base_url,
        private_key=private_key,
        wallet_address=wallet_address,
        testnet=testnet,
    )


class HyperliquidLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a ``HyperliquidDataClient`` factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: HyperliquidDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> HyperliquidDataClient:
        """
        Create a new Hyperliquid data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : HyperliquidDataClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        HyperliquidDataClient

        """
        client = get_cached_hyperliquid_http_client(
            private_key=config.private_key,
            wallet_address=config.wallet_address,
            base_url=config.base_url_http,
            testnet=config.testnet,
        )

        ws_client = get_cached_hyperliquid_websocket_client(
            private_key=config.private_key,
            wallet_address=config.wallet_address,
            base_url=config.base_url_ws,
            testnet=config.testnet,
        )

        # Create and load the instrument provider
        provider = HyperliquidInstrumentProvider(
            client=client,
            config=InstrumentProviderConfig(
                load_all=True,
                load_ids=None,
                filters=None,
            ),
        )

        return HyperliquidDataClient(
            loop=loop,
            client=client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            base_url_http=config.base_url_http,
            base_url_ws=config.base_url_ws,
            update_instruments_interval_mins=config.update_instruments_interval_mins,
        )


class HyperliquidLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a ``HyperliquidExecutionClient`` factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: HyperliquidExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> HyperliquidExecutionClient:
        """
        Create a new Hyperliquid execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : HyperliquidExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        HyperliquidExecutionClient

        """
        client = get_cached_hyperliquid_http_client(
            private_key=config.private_key,
            wallet_address=config.wallet_address,
            base_url=config.base_url_http,
            testnet=config.testnet,
        )

        ws_client = get_cached_hyperliquid_websocket_client(
            private_key=config.private_key,
            wallet_address=config.wallet_address,
            base_url=config.base_url_ws,
            testnet=config.testnet,
        )

        # Create and load the instrument provider
        provider = HyperliquidInstrumentProvider(
            client=client,
            config=InstrumentProviderConfig(
                load_all=True,
                load_ids=None,
                filters=None,
            ),
        )

        return HyperliquidExecutionClient(
            loop=loop,
            client=client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            base_url_http=config.base_url_http,
            base_url_ws=config.base_url_ws,
        )
