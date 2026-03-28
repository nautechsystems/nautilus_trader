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

"""Factory classes for creating Rithmic clients."""

from __future__ import annotations

from typing import TYPE_CHECKING

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import RithmicExecClientConfig
from nautilus_trader.adapters.rithmic.data import RithmicLiveDataClient
from nautilus_trader.adapters.rithmic.execution import RithmicLiveExecutionClient
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


if TYPE_CHECKING:
    from nautilus_trader.cache import Cache
    from nautilus_trader.common.component import MessageBus


class RithmicLiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for creating Rithmic live data clients.

    Provides a factory for constructing `RithmicLiveDataClient` instances.
    """

    @staticmethod
    def create(
        loop,
        name: str,
        config: RithmicDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock,
    ) -> RithmicLiveDataClient:
        """
        Create a new Rithmic data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : RithmicDataClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        RithmicLiveDataClient
        """
        return RithmicLiveDataClient(
            loop=loop,
            client_id=name,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )


class RithmicLiveExecClientFactory(LiveExecClientFactory):
    """
    Factory for creating Rithmic live execution clients.

    Provides a factory for constructing `RithmicLiveExecutionClient` instances.
    """

    @staticmethod
    def create(
        loop,
        name: str,
        config: RithmicExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock,
    ) -> RithmicLiveExecutionClient:
        """
        Create a new Rithmic execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : RithmicExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        RithmicLiveExecutionClient
        """
        return RithmicLiveExecutionClient(
            loop=loop,
            client_id=name,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )
