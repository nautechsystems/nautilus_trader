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

from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.model.c_enums.component_state cimport ComponentState
from nautilus_trader.model.c_enums.component_state cimport component_state_to_string


cdef dict _COMPONENT_STATE_TABLE = {
    (ComponentState.INITIALIZED, 'RESET'): ComponentState.RESETTING,
    (ComponentState.INITIALIZED, 'START'): ComponentState.STARTING,
    (ComponentState.INITIALIZED, 'DISPOSE'): ComponentState.DISPOSING,
    (ComponentState.RESETTING, 'RESET'): ComponentState.INITIALIZED,
    (ComponentState.STARTING, 'RUNNING'): ComponentState.RUNNING,
    (ComponentState.STARTING, 'STOP'): ComponentState.STOPPING,
    (ComponentState.RUNNING, 'STOP'): ComponentState.STOPPING,
    (ComponentState.RESUMING, 'STOP'): ComponentState.STOPPING,
    (ComponentState.RESUMING, 'RUNNING'): ComponentState.RUNNING,
    (ComponentState.STOPPING, 'STOPPED'): ComponentState.STOPPED,
    (ComponentState.STOPPED, 'RESET'): ComponentState.RESETTING,
    (ComponentState.STOPPED, 'RESUME'): ComponentState.RESUMING,
    (ComponentState.STOPPED, 'DISPOSE'): ComponentState.DISPOSING,
    (ComponentState.DISPOSING, 'DISPOSED'): ComponentState.DISPOSED,
}

cpdef dict get_state_transition_table():
    return _COMPONENT_STATE_TABLE


cpdef FiniteStateMachine create_component_fsm():
    return FiniteStateMachine(
        state_transition_table=get_state_transition_table(),
        initial_state=ComponentState.INITIALIZED,
        state_parser=component_state_to_string,
    )
