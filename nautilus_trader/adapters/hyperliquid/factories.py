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

from __future__ import annotations

import asyncio
from functools import lru_cache
from typing import TYPE_CHECKING

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.data import HyperliquidDataClient
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.execution import HyperliquidExecutionClient
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


if TYPE_CHECKING:
    from collections.abc import Iterable


def _resolve_environment(
    environment: HyperliquidEnvironment | None,
    testnet: bool,
) -> HyperliquidEnvironment:
    if environment is not None:
        return environment
    return HyperliquidEnvironment.TESTNET if testnet else HyperliquidEnvironment.MAINNET


@lru_cache(1)
def get_cached_hyperliquid_http_client(
    private_key: str | None = None,
    vault_address: str | None = None,
    account_address: str | None = None,
    timeout_secs: int | None = None,
    environment: HyperliquidEnvironment = HyperliquidEnvironment.MAINNET,
    proxy_url: str | None = None,
    normalize_prices: bool = True,
) -> nautilus_pyo3.HyperliquidHttpClient:
    """
    Cache and return a Hyperliquid HTTP client with the given parameters.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    private_key : str, optional
        The EVM private key for the client.
        If ``None`` then will source the `HYPERLIQUID_PK` or `HYPERLIQUID_TESTNET_PK`
        environment variable (depending on the environment setting).
        Note: The PyO3 client handles credentials internally.
    vault_address : str, optional
        The vault address for vault trading.
        If ``None`` then will source the `HYPERLIQUID_VAULT` or `HYPERLIQUID_TESTNET_VAULT`
        environment variable (depending on the environment setting).
        Note: The PyO3 client handles credentials internally.
    account_address : str, optional
        The main account address when using an agent wallet (API sub-key).
        If ``None`` then will source the `HYPERLIQUID_ACCOUNT_ADDRESS` env var.
    timeout_secs : int, optional
        The timeout (seconds) for HTTP requests to Hyperliquid.
    environment : HyperliquidEnvironment, default MAINNET
        The Hyperliquid environment (MAINNET or TESTNET).
    proxy_url : str, optional
        Optional HTTP proxy URL.
    normalize_prices : bool, default True
        If order prices should be normalized to 5 significant figures.

    Returns
    -------
    nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client instance.

    """
    kwargs: dict = {
        "private_key": private_key,
        "vault_address": vault_address,
        "account_address": account_address,
        "environment": environment,
        "proxy_url": proxy_url,
        "normalize_prices": normalize_prices,
    }

    if timeout_secs is not None:
        kwargs["timeout_secs"] = timeout_secs

    return nautilus_pyo3.HyperliquidHttpClient(**kwargs)


@lru_cache(1)
def get_cached_hyperliquid_instrument_provider(
    client: nautilus_pyo3.HyperliquidHttpClient,
    config: InstrumentProviderConfig | None = None,
    product_types: tuple[HyperliquidProductType, ...] | None = None,
) -> HyperliquidInstrumentProvider:
    """
    Cache and return a Hyperliquid instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.
    product_types : tuple[HyperliquidProductType, ...], optional
        The Hyperliquid product types to enable for the provider.

    Returns
    -------
    HyperliquidInstrumentProvider

    """
    return HyperliquidInstrumentProvider(
        client=client,
        config=config,
        product_types=product_types,
    )


def _resolve_product_types(
    product_types: Iterable[HyperliquidProductType] | None,
) -> tuple[HyperliquidProductType, ...] | None:
    if product_types is None:
        return None

    return tuple(HyperliquidProductType(pt) for pt in product_types)


class HyperliquidLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Hyperliquid live data client factory.
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
            The custom client ID.
        config : HyperliquidDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        HyperliquidDataClient

        """
        environment = _resolve_environment(config.environment, config.testnet)
        client = get_cached_hyperliquid_http_client(
            timeout_secs=config.http_timeout_secs,
            environment=environment,
            proxy_url=config.proxy_url,
        )
        provider = get_cached_hyperliquid_instrument_provider(
            client=client,
            config=config.instrument_provider,
            product_types=_resolve_product_types(config.product_types),
        )
        return HyperliquidDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class HyperliquidLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Hyperliquid live execution client factory.
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
            The custom client ID.
        config : HyperliquidExecClientConfig
            The client configuration.
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
        environment = _resolve_environment(config.environment, config.testnet)
        client = get_cached_hyperliquid_http_client(
            private_key=config.private_key,
            vault_address=config.vault_address,
            account_address=config.account_address,
            timeout_secs=config.http_timeout_secs,
            environment=environment,
            proxy_url=config.proxy_url,
            normalize_prices=config.normalize_prices,
        )
        provider = get_cached_hyperliquid_instrument_provider(
            client=client,
            config=config.instrument_provider,
            product_types=_resolve_product_types(config.product_types),
        )
        return HyperliquidExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
