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

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter.data import LighterDataClient
from nautilus_trader.adapters.lighter.execution import LighterExecutionClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


class LighterLiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for creating Lighter data clients.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: LighterDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> LighterDataClient:
        raise NotImplementedError("Data client wiring will be implemented in PR2.")


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
