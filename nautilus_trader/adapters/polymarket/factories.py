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

from py_clob_client.client import ApiCreds
from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON

from nautilus_trader.adapters.polymarket.common.credentials import PolymarketWebSocketAuth
from nautilus_trader.adapters.polymarket.common.credentials import get_polymarket_api_key
from nautilus_trader.adapters.polymarket.common.credentials import get_polymarket_api_secret
from nautilus_trader.adapters.polymarket.common.credentials import get_polymarket_funder
from nautilus_trader.adapters.polymarket.common.credentials import get_polymarket_passphrase
from nautilus_trader.adapters.polymarket.common.credentials import get_polymarket_private_key
from nautilus_trader.adapters.polymarket.config import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket.config import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket.data import PolymarketDataClient
from nautilus_trader.adapters.polymarket.execution import PolymarketExecutionClient
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_polymarket_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    passphrase: str | None = None,
    base_url: str | None = None,
    chain_id: int = POLYGON,
    signature_type: int = 0,
    private_key: str | None = None,
    funder: str | None = None,
) -> ClobClient:
    """
    Cache and return a Polymarket CLOB client.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The API key for the client.
    api_secret : str, optional
        The API secret for the client.
    passphrase : str, optional
        The passphrase for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    chain_id : int, default POLYGON
        The chain ID for the client.
    signature_type : int, default 0 (EOA)
        The Polymarket signature type.
    private_key : str, optional
        The private key for the wallet on the **Polygon** network.
    funder : str, optional
        The wallet address (public key) on the **Polygon** network used for funding USDC.

    Returns
    -------
    ClobClient

    """
    creds = ApiCreds(
        api_key=api_key or get_polymarket_api_key(),
        api_secret=api_secret or get_polymarket_api_secret(),
        api_passphrase=passphrase or get_polymarket_passphrase(),
    )
    key = private_key or get_polymarket_private_key()
    funder = funder or get_polymarket_funder()
    return ClobClient(
        base_url or "https://clob.polymarket.com",
        chain_id=chain_id,
        signature_type=signature_type,
        creds=creds,
        key=key,
        funder=funder,
    )


@lru_cache(1)
def get_polymarket_instrument_provider(
    client: ClobClient,
    clock: LiveClock,
    config: InstrumentProviderConfig,
) -> PolymarketInstrumentProvider:
    """
    Cache and return a Polymarket instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : py_clob_client.client.ClobClient
        The client for the instrument provider.
    clock : LiveClock
        The clock for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.

    Returns
    -------
    PolymarketInstrumentProvider

    """
    return PolymarketInstrumentProvider(
        client=client,
        config=config,
        clock=clock,
    )


class PolymarketLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Polymarket live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: PolymarketDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> PolymarketDataClient:
        """
        Create a new Polymarket data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BybitDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        PolymarketDataClient

        """
        http_client = get_polymarket_http_client(
            private_key=config.private_key,
            signature_type=config.signature_type,
            funder=config.funder,
            api_key=config.api_key,
            api_secret=config.api_secret,
            passphrase=config.passphrase,
            base_url=config.base_url_http,
        )
        provider = get_polymarket_instrument_provider(
            client=http_client,
            clock=clock,
            config=config.instrument_provider,
        )
        return PolymarketDataClient(
            loop=loop,
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class PolymarketLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Polymarket live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: PolymarketExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> PolymarketDataClient:
        """
        Create a new Polymarket execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BybitDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        PolymarketDataClient

        """
        http_client = get_polymarket_http_client(
            private_key=config.private_key,
            signature_type=config.signature_type,
            funder=config.funder,
            api_key=config.api_key,
            api_secret=config.api_secret,
            passphrase=config.passphrase,
            base_url=config.base_url_http,
        )
        ws_auth = PolymarketWebSocketAuth(
            apiKey=config.api_key or get_polymarket_api_key(),
            secret=config.api_secret or get_polymarket_api_secret(),
            passphrase=config.passphrase or get_polymarket_passphrase(),
        )
        provider = get_polymarket_instrument_provider(
            client=http_client,
            clock=clock,
            config=config.instrument_provider,
        )
        return PolymarketExecutionClient(
            loop=loop,
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            ws_auth=ws_auth,
            name=name,
        )
