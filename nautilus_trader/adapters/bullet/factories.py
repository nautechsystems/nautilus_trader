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
import os
from functools import lru_cache

from nautilus_trader.adapters.bullet.config import BulletDataClientConfig
from nautilus_trader.adapters.bullet.config import BulletExecClientConfig
from nautilus_trader.adapters.bullet.data import BulletDataClient
from nautilus_trader.adapters.bullet.execution import BulletExecutionClient
from nautilus_trader.adapters.bullet.providers import BulletInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import BulletEnvironment
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import ClientId


def _http_base_url(environment: BulletEnvironment, override: str | None = None) -> str:
    if override:
        return override
    host_map = {
        BulletEnvironment.Mainnet: "tradingapi.bullet.xyz",
        BulletEnvironment.Testnet: "tradingapi.testnet.bullet.xyz",
        BulletEnvironment.Staging: "tradingapi.staging.bullet.xyz",
    }
    host = host_map.get(environment, "tradingapi.bullet.xyz")
    return f"https://{host}"


def _ws_url(environment: BulletEnvironment, override: str | None = None) -> str:
    if override:
        return override
    host_map = {
        BulletEnvironment.Mainnet: "tradingapi.bullet.xyz",
        BulletEnvironment.Testnet: "tradingapi.testnet.bullet.xyz",
        BulletEnvironment.Staging: "tradingapi.staging.bullet.xyz",
    }
    host = host_map.get(environment, "tradingapi.bullet.xyz")
    return f"wss://{host}/ws"


@lru_cache(maxsize=4)
def get_cached_bullet_http_client(
    base_url: str,
    timeout_secs: int = 60,
    proxy_url: str | None = None,
) -> nautilus_pyo3.BulletHttpClient:
    """
    Cache and return a Bullet HTTP client with the given parameters.

    Parameters
    ----------
    base_url : str
        The HTTP base URL.
    timeout_secs : int, default 60
        The timeout (seconds) for HTTP requests.
    proxy_url : str, optional
        Optional HTTP proxy URL.

    Returns
    -------
    nautilus_pyo3.BulletHttpClient

    """
    return nautilus_pyo3.BulletHttpClient(
        base_url=base_url,
        timeout_secs=timeout_secs,
        proxy_url=proxy_url,
    )


@lru_cache(maxsize=4)
def get_cached_bullet_instrument_provider(
    client: nautilus_pyo3.BulletHttpClient,
    client_id: ClientId,
    config: InstrumentProviderConfig | None = None,
) -> BulletInstrumentProvider:
    """
    Cache and return a Bullet instrument provider.

    Parameters
    ----------
    client : nautilus_pyo3.BulletHttpClient
        The Bullet HTTP client.
    client_id : ClientId
        The client ID for logging.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration.

    Returns
    -------
    BulletInstrumentProvider

    """
    return BulletInstrumentProvider(
        client=client,
        client_id=client_id,
        config=config,
    )


def _resolve_private_key(config: BulletExecClientConfig) -> str | None:
    return (
        config.private_key
        or os.environ.get("BULLET_PRIVATE_KEY")
    )


def _resolve_key_file(config: BulletExecClientConfig) -> str | None:
    return (
        config.key_file
        or os.environ.get("BULLET_KEY_FILE")
    )


def _resolve_account_address(config: BulletExecClientConfig) -> str | None:
    return (
        config.account_address
        or os.environ.get("BULLET_ACCOUNT_ADDRESS")
    )


class BulletLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Bullet.xyz live data client factory.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BulletDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BulletDataClient:
        """
        Create a new Bullet data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BulletDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BulletDataClient

        """
        base_url = _http_base_url(config.environment, config.base_url_http)
        http_client = get_cached_bullet_http_client(
            base_url=base_url,
            timeout_secs=config.http_timeout_secs,
            proxy_url=config.proxy_url,
        )
        client_id = ClientId(name or "BULLET")
        provider = get_cached_bullet_instrument_provider(
            client=http_client,
            client_id=client_id,
            config=config.instrument_provider,
        )

        ws_client = nautilus_pyo3.BulletWebSocketClient(
            url=_ws_url(config.environment, config.base_url_ws),
        )

        return BulletDataClient(
            loop=loop,
            http_client=http_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class BulletLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Bullet.xyz live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BulletExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BulletExecutionClient:
        """
        Create a new Bullet execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BulletExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BulletExecutionClient

        """
        base_url = _http_base_url(config.environment, config.base_url_http)
        http_client = get_cached_bullet_http_client(
            base_url=base_url,
            timeout_secs=config.http_timeout_secs,
            proxy_url=config.proxy_url,
        )
        client_id = ClientId(name or "BULLET")
        provider = get_cached_bullet_instrument_provider(
            client=http_client,
            client_id=client_id,
            config=config.instrument_provider,
        )

        order_client = nautilus_pyo3.BulletOrderClient(
            base_url=base_url,
            timeout_secs=config.http_timeout_secs,
            proxy_url=config.proxy_url,
            private_key=_resolve_private_key(config),
            key_file=_resolve_key_file(config),
            account_address=_resolve_account_address(config),
        )

        ws_client = nautilus_pyo3.BulletWebSocketClient(
            url=_ws_url(config.environment, config.base_url_ws),
        )

        return BulletExecutionClient(
            loop=loop,
            http_client=http_client,
            order_client=order_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
