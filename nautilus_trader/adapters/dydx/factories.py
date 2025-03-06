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
"""
Provide factories to construct data and execution clients for dYdX.
"""

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.dydx.common.credentials import get_wallet_address
from nautilus_trader.adapters.dydx.common.urls import get_grpc_base_url
from nautilus_trader.adapters.dydx.common.urls import get_http_base_url
from nautilus_trader.adapters.dydx.common.urls import get_ws_base_url
from nautilus_trader.adapters.dydx.config import DYDXDataClientConfig
from nautilus_trader.adapters.dydx.config import DYDXExecClientConfig
from nautilus_trader.adapters.dydx.data import DYDXDataClient
from nautilus_trader.adapters.dydx.execution import DYDXExecutionClient
from nautilus_trader.adapters.dydx.grpc.account import DYDXAccountGRPCAPI
from nautilus_trader.adapters.dydx.grpc.account import TransactionBuilder
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.providers import DYDXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


def get_dydx_grcp_client(
    is_testnet: bool = False,
) -> DYDXAccountGRPCAPI:
    """
    Return a dYdX GRPC client.

    Parameters
    ----------
    is_testnet : bool, default False
        Whether to use the testnet or production endpoint.

    Returns
    -------
    DYDXAccountGRPCAPI
        The dYdX GRPC client.

    """
    channel_url = get_grpc_base_url(is_testnet=is_testnet)

    chain_id = "dydx-mainnet-1"
    usdc_denom = "ibc/8E27BA2D5493AF5636760E354E46004562C46AB7EC0CC4C1CA14E9E20E2545B5"

    if is_testnet:
        chain_id = "dydx-testnet-4"

    return DYDXAccountGRPCAPI(
        channel_url=channel_url,
        transaction_builder=TransactionBuilder(
            chain_id=chain_id,
            denomination=usdc_denom,
        ),
    )


@lru_cache(1)
def get_dydx_http_client(
    clock: LiveClock,
    base_url: str | None = None,
    is_testnet: bool = False,
) -> DYDXHttpClient:
    """
    Cache and return a dYdX HTTP client with the given key and secret.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    is_testnet : bool, default False
        If the client is connecting to the testnet API.

    Returns
    -------
    DYDXHttpClient

    """
    http_base_url = base_url or get_http_base_url(is_testnet)
    return DYDXHttpClient(
        clock=clock,
        base_url=http_base_url,
    )


@lru_cache(1)
def get_dydx_instrument_provider(
    client: DYDXHttpClient,
    clock: LiveClock,
    config: InstrumentProviderConfig,
    wallet_address: str,
    is_testnet: bool,
) -> DYDXInstrumentProvider:
    """
    Cache and return a dYdX instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : DYDXHttpClient
        The client for the instrument provider.
    clock : LiveClock
        The clock for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.
    wallet_address: str
        The wallet address containing the crypto assets.
    is_testnet: bool
        Whether the testnet of dYdX is used.

    Returns
    -------
    DYDXInstrumentProvider

    """
    return DYDXInstrumentProvider(
        client=client,
        grpc_account_client=get_dydx_grcp_client(is_testnet=is_testnet),
        config=config,
        clock=clock,
        wallet_address=wallet_address,
        is_testnet=is_testnet,
    )


class DYDXLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `dYdX` live data client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DYDXDataClientConfig,  # type: ignore[override]
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DYDXDataClient:
        """
        Create a new dYdX data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DYDXDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        DYDXDataClient

        """
        client: DYDXHttpClient = get_dydx_http_client(
            clock=clock,
            is_testnet=config.is_testnet,
        )
        wallet_address = config.wallet_address or get_wallet_address(is_testnet=config.is_testnet)
        provider = get_dydx_instrument_provider(
            client=client,
            clock=clock,
            config=config.instrument_provider,
            is_testnet=config.is_testnet,
            wallet_address=wallet_address,
        )
        ws_base_url = get_ws_base_url(is_testnet=config.is_testnet)
        return DYDXDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            ws_base_url=ws_base_url,
            config=config,
            name=name,
        )


class DYDXLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a dYdX live execution client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DYDXExecClientConfig,  # type: ignore[override]
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DYDXExecutionClient:
        """
        Create a new dYdX execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DYDXExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        DYDXExecutionClient

        """
        client: DYDXHttpClient = get_dydx_http_client(
            clock=clock,
            base_url=config.base_url_http,
            is_testnet=config.is_testnet,
        )
        wallet_address = config.wallet_address or get_wallet_address(is_testnet=config.is_testnet)
        provider = get_dydx_instrument_provider(
            client=client,
            clock=clock,
            config=config.instrument_provider,
            is_testnet=config.is_testnet,
            wallet_address=wallet_address,
        )
        ws_base_url = get_ws_base_url(is_testnet=config.is_testnet)
        return DYDXExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            grpc_account_client=get_dydx_grcp_client(is_testnet=config.is_testnet),
            base_url_ws=config.base_url_ws or ws_base_url,
            config=config,
            name=name,
        )
