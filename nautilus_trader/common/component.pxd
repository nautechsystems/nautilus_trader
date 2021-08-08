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

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTrigger
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.model.identifiers cimport ComponentId


cdef class ComponentFSMFactory:

    @staticmethod
    cdef create()


cdef class Component:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef FiniteStateMachine _fsm

    cdef readonly ComponentId id
    """The components ID.\n\n:returns: `ComponentId`"""

    cdef ComponentState state_c(self) except *
    cdef str state_string_c(self)
    cdef bint is_running_c(self)

    cdef void _change_clock(self, Clock clock) except *
    cdef void _change_logger(self, Logger logger) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _start(self) except *
    cpdef void _stop(self) except *
    cpdef void _resume(self) except *
    cpdef void _reset(self) except *
    cpdef void _dispose(self) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void start(self) except *
    cpdef void stop(self) except *
    cpdef void resume(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger1,
        ComponentTrigger trigger2,
        action,
    ) except *
