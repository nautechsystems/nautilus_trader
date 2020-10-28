# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport Venue


cdef class ExecutionClient:
    """
    The base class for all execution clients.
    """

    def __init__(
            self,
            Venue venue not None,
            AccountId account_id not None,
            ExecutionEngine engine not None,
            Logger logger not None):
        """
        Initialize a new instance of the ExecutionClient class.

        Parameters
        ----------
        venue : Venue
            The trading venue identifier for the client.
        account_id : AccountId
            The account identifier for the client.
        engine : ExecutionEngine
            The execution engine to connect to the client.
        logger : Logger
            The logger for the component.

        """
        Condition.equal(venue, account_id.issuer_as_venue(), "venue", "account_id.issuer_as_venue()")

        self._engine = engine
        self._log = LoggerAdapter(type(self).__name__, logger)
        self._venue = venue
        self._account_id = account_id

        self._log.info(f"Initialized.")

    @property
    def venue(self):
        """
        The execution clients venue.

        Returns
        -------
        Venue

        """
        return self._venue

    @property
    def account_id(self):
        """
        The execution clients account identifier.

        Returns
        -------
        Venue

        """
        return self._account_id

    @property
    def is_connected(self):
        """
        If the execution client is currently connected.

        Returns
        -------
        bool
            True if connected, else False.

        """
        return self._is_connected

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void connect(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void disconnect(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void dispose(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void submit_order(self, SubmitOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void modify_order(self, ModifyOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void cancel_order(self, CancelOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

# --------------------------------------------------------------------------------------------------

    cpdef void handle_event(self, Event event) except *:
        """
        Handle the event by sending it to the execution engine for processing.

        Parameters
        ----------
        event : Event
            The event to handle.

        """
        self._engine.process(event)
