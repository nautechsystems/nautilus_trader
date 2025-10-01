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
from __future__ import annotations

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.schwab.config import SchwabClientConfig
from nautilus_trader.adapters.schwab.config import SchwabDataClientConfig
from nautilus_trader.adapters.schwab.config import SchwabExecClientConfig
from nautilus_trader.adapters.schwab.config import SchwabInstrumentProviderConfig
from nautilus_trader.adapters.schwab.data import SchwabDataClient
from nautilus_trader.adapters.schwab.execution import SchwabExecutionClient
from nautilus_trader.adapters.schwab.http.client import SchwabHttpClient
from nautilus_trader.adapters.schwab.providers import SchwabInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_schwab_http_client(config: SchwabClientConfig) -> SchwabHttpClient:
    return SchwabHttpClient(config)


@lru_cache(1)
def get_schwab_instrument_provider(
    http_client: SchwabHttpClient,
    clock: LiveClock,
    config: SchwabInstrumentProviderConfig,
) -> SchwabInstrumentProvider:
    return SchwabInstrumentProvider(http_client, clock, config)


class SchwabLiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for Schwab live data clients.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: SchwabDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> SchwabDataClient:
        if config.http_client is None:
            raise ValueError("SchwabDataClientConfig.rest_client must be provided")

        http_client = get_schwab_http_client(config.http_client)
        provider = get_schwab_instrument_provider(http_client, clock, config.instrument_provider)
        return SchwabDataClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            http_client=http_client,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class SchwabLiveExecClientFactory(LiveExecClientFactory):
    """
    Factory for Schwab live execution clients.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: SchwabExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> SchwabExecutionClient:
        if config.http_client is None:
            raise ValueError("SchwabExecClientConfig.http_client must be provided")

        http_client = get_schwab_http_client(config.http_client)
        provider = get_schwab_instrument_provider(http_client, clock, config.instrument_provider)
        return SchwabExecutionClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            http_client=http_client,
            instrument_provider=provider,
            config=config,
            name=name,
        )


__all__ = [
    "SchwabLiveDataClientFactory",
    "SchwabLiveExecClientFactory",
]
