# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
The `LiveDataClient` class is responsible for interfacing with a particular API
which may be presented directly by an exchange, or broker intermediary. It
could also be possible to write clients for specialized data publishers.
"""

import asyncio
from typing import Any, Dict, Optional

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus


class LiveDataClient(DataClient):
    """
    The abstract base class for all live data clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The client venue. If multi-venue then can be ``None``.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client_id: ClientId,
        venue: Optional[Venue],
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        config: Optional[Dict[str, Any]] = None,
    ):
        super().__init__(
            client_id=client_id,
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop

    def connect(self) -> None:
        """Connect the client."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        """Disconnect the client."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def run_after_delay(self, delay: float, coro) -> None:
        await asyncio.sleep(delay)
        return await coro


class LiveMarketDataClient(MarketDataClient):
    """
    The abstract base class for all live data clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The client venue. If multi-venue then can be ``None``.
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client_id: ClientId,
        venue: Optional[Venue],
        instrument_provider: InstrumentProvider,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        config: Optional[Dict[str, Any]] = None,
    ):
        PyCondition.type(instrument_provider, InstrumentProvider, "instrument_provider")

        super().__init__(
            client_id=client_id,
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._instrument_provider = instrument_provider

    def connect(self) -> None:
        """Connect the client."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        """Disconnect the client."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def run_after_delay(self, delay, coro) -> None:
        await asyncio.sleep(delay)
        return await coro
