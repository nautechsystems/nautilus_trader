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
from typing import TYPE_CHECKING

from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.execution_client import LiveExecutionClient

if TYPE_CHECKING:
    from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider


class LighterExecutionClient(LiveExecutionClient):
    """
    Placeholder Lighter execution client.

    The full implementation will be added in PR3 after signing/auth flows are validated.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: object,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: LighterInstrumentProvider,
        config: LighterExecClientConfig,
        name: str,
    ) -> None:
        super().__init__(loop=loop, name=name, config=config, msgbus=msgbus, cache=cache)
        raise NotImplementedError("LighterExecutionClient will be implemented in PR3.")
