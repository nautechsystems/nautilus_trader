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
from typing import Any

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter.data import LighterDataClient
from nautilus_trader.adapters.lighter.execution import LighterExecutionClient
from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


class LighterLiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for creating Lighter data clients.
    """

    @staticmethod
    @lru_cache(1)
    def _http_client(
        testnet: bool,
        base_url_http: str | None,
        http_timeout_secs: int,
        proxy_url: str | None,
    ) -> Any:
        lighter_mod = getattr(nautilus_pyo3, "lighter")
        return lighter_mod.LighterHttpClient(
            is_testnet=testnet,
            base_url_override=base_url_http,
            timeout_secs=http_timeout_secs,
            proxy_url=proxy_url,
        )

    @staticmethod
    @lru_cache(1)
    def _instrument_provider(client: Any, config: InstrumentProviderConfig | None) -> LighterInstrumentProvider:
        return LighterInstrumentProvider(client, config or InstrumentProviderConfig())

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: LighterDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> LighterDataClient:
        lighter_mod = getattr(nautilus_pyo3, "lighter")
        http_client = LighterLiveDataClientFactory._http_client(
            testnet=config.testnet,
            base_url_http=config.base_url_http,
            http_timeout_secs=config.http_timeout_secs,
            proxy_url=config.http_proxy_url,
        )
        instrument_provider = LighterLiveDataClientFactory._instrument_provider(
            http_client,
            InstrumentProviderConfig(),
        )
        ws_client = lighter_mod.LighterWebSocketClient(
            is_testnet=config.testnet,
            base_url_override=config.base_url_ws,
            http_client=http_client,
        )

        return LighterDataClient(
            loop=loop,
            http_client=http_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
            name=name,
        )


class LighterLiveExecClientFactory(LiveExecClientFactory):
    """
    Factory for creating Lighter execution clients.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: LighterExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> LighterExecutionClient:
        raise NotImplementedError("Execution client wiring will be implemented in PR3.")
