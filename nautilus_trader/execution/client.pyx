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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId


cdef class ExecutionClient:
    """
    The abstract base class for all execution clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        ClientId client_id not None,
        AccountId account_id not None,
        ExecutionEngine engine not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `ExecutionClient` class.

        Parameters
        ----------
        client_id : ClientId
            The client identifier. It is assumed that the client_id will equal
            the venue identifier.
        account_id : AccountId
            The account identifier for the client.
        engine : ExecutionEngine
            The execution engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        """
        Condition.equal(client_id.value, account_id.issuer_as_venue().value, "client_id.value", "account_id.issuer_as_venue().value")

        if config is None:
            config = {}

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(
            component=config.get("name", f"ExecClient-{client_id.value}"),
            logger=logger,
        )
        self._engine = engine
        self._config = config

        self.id = client_id
        self.venue = Venue(client_id.value)  # Assumption that ClientId == Venue
        self.account_id = account_id
        self.is_connected = False

        self._log.info(f"Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}-{self.id.value}"

    cpdef void _set_connected(self, bint value=True) except *:
        """
        Setter for pure Python implementations to change the readonly property.

        Parameters
        ----------
        value : bool
            The value to set for is_connected.

        """
        self.is_connected = value

    cpdef void connect(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void disconnect(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void dispose(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_order(self, UpdateOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void cancel_order(self, CancelOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_event_py(self, Event event):
        self._engine.process(event)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
