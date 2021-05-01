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
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport Venue


cdef class ExecutionClient:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef ExecutionEngine _engine
    cdef dict _config

    cdef readonly ClientId id
    """The clients identifier.\n\n:returns: `ClientId`"""
    cdef readonly Venue venue
    """The clients venue.\n\n:returns: `Venue`"""
    cdef readonly AccountId account_id
    """The clients account identifier.\n\n:returns: `AccountId`"""
    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""

    cpdef void _set_connected(self, bint value=*) except *
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command) except *
    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *
    cpdef void update_order(self, UpdateOrder command) except *
    cpdef void cancel_order(self, CancelOrder command) except *

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *
