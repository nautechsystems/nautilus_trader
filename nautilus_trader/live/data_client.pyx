# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.live.data_engine cimport LiveDataEngine


cdef class LiveDataClient(DataClient):
    """
    The abstract base class for all live data clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
            self,
            Venue venue not None,
            LiveDataEngine engine not None,
            LiveClock clock not None,
            Logger logger not None,
            dict config=None,
    ):
        """
        Initialize a new instance of the `LiveDataClient` class.

        Parameters
        ----------
        venue : Venue
            The venue for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        config : dict[str, object], optional
            The configuration options.

        """
        super().__init__(
            venue,
            engine,
            clock,
            logger,
            config,
        )

        self._loop: asyncio.AbstractEventLoop = engine.get_event_loop()
