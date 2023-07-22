# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Callable

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.enums_c cimport ComponentTrigger
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.model.identifiers cimport Identifier
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class ComponentFSMFactory:

    @staticmethod
    cdef create()


cdef class Component:
    cdef readonly Clock _clock
    cdef readonly LoggerAdapter _log
    cdef readonly MessageBus _msgbus
    cdef FiniteStateMachine _fsm
    cdef dict _config

    cdef readonly TraderId trader_id
    """The trader ID associated with the component.\n\n:returns: `TraderId`"""
    cdef readonly Identifier id
    """The components ID.\n\n:returns: `ComponentId`"""
    cdef readonly type type
    """The components type.\n\n:returns: `type`"""

    cdef void _change_clock(self, Clock clock)
    cdef void _change_logger(self, Logger logger)
    cdef void _change_msgbus(self, MessageBus msgbus)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _start(self)
    cpdef void _stop(self)
    cpdef void _resume(self)
    cpdef void _reset(self)
    cpdef void _dispose(self)
    cpdef void _degrade(self)
    cpdef void _fault(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void _initialize(self)
    cpdef void start(self)
    cpdef void stop(self)
    cpdef void resume(self)
    cpdef void reset(self)
    cpdef void dispose(self)
    cpdef void degrade(self)
    cpdef void fault(self)

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger,
        bint is_transitory,
        action: Callable[[None], None]=*,
    )
