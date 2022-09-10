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
import types
from typing import Optional

from nautilus_trader.common.providers import InstrumentProvider

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class LiveDataClient(DataClient):
    """
    The abstract base class for all live data clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    venue : Venue, optional (no default, ``None`` must be passed explicitly)
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
        loop not None: asyncio.AbstractEventLoop,
        ClientId client_id not None,
        Venue venue: Optional[Venue],
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config = None,
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

    @types.coroutine
    def sleep0(self) -> None:
        # Skip one event loop run cycle.
        #
        # This is equivalent to `asyncio.sleep(0)` however avoids the overhead
        # of the pure Python function call and integer comparison <= 0.
        #
        # Uses a bare 'yield' expression (which Task.__step knows how to handle)
        # instead of creating a Future object.
        yield

    async def run_after_delay(self, delay: float, coro) -> None:
        await asyncio.sleep(delay)
        return await coro


cdef class LiveMarketDataClient(MarketDataClient):
    """
    The abstract base class for all live data clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    venue : Venue, optional (no default, ``None`` must be passed explicitly)
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
        loop not None: asyncio.AbstractEventLoop,
        ClientId client_id not None,
        Venue venue: Optional[Venue],
        instrument_provider not None: InstrumentProvider,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config = None,
    ):
        Condition.type(instrument_provider, InstrumentProvider, "instrument_provider")

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

    @types.coroutine
    def sleep0(self) -> None:
        # Skip one event loop run cycle.
        #
        # This is equivalent to `asyncio.sleep(0)` however avoids the overhead
        # of the pure Python function call and integer comparison <= 0.
        #
        # Uses a bare 'yield' expression (which Task.__step knows how to handle)
        # instead of creating a Future object.
        yield

    async def run_after_delay(self, delay, coro) -> None:
        await asyncio.sleep(delay)
        return await coro
