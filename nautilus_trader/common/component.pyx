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

"""
Provides the C enums for the `common` sub-package.

TODO: isort currently producing a bad format for cimport component_trigger_to_string

isort:skip_file
"""

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.c_enums.component_state cimport ComponentStateParser
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTrigger
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTriggerParser
from nautilus_trader.core.fsm cimport FiniteStateMachine


cdef dict _COMPONENT_STATE_TABLE = {
    (ComponentState.INITIALIZED, ComponentTrigger.RESET): ComponentState.RESETTING,
    (ComponentState.INITIALIZED, ComponentTrigger.START): ComponentState.STARTING,
    (ComponentState.INITIALIZED, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,
    (ComponentState.RESETTING, ComponentTrigger.RESET): ComponentState.INITIALIZED,
    (ComponentState.STARTING, ComponentTrigger.RUNNING): ComponentState.RUNNING,
    (ComponentState.STARTING, ComponentTrigger.STOP): ComponentState.STOPPING,
    (ComponentState.RUNNING, ComponentTrigger.STOP): ComponentState.STOPPING,
    (ComponentState.RESUMING, ComponentTrigger.STOP): ComponentState.STOPPING,
    (ComponentState.RESUMING, ComponentTrigger.RUNNING): ComponentState.RUNNING,
    (ComponentState.STOPPING, ComponentTrigger.STOPPED): ComponentState.STOPPED,
    (ComponentState.STOPPED, ComponentTrigger.RESET): ComponentState.RESETTING,
    (ComponentState.STOPPED, ComponentTrigger.RESUME): ComponentState.RESUMING,
    (ComponentState.STOPPED, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,
    (ComponentState.DISPOSING, ComponentTrigger.DISPOSED): ComponentState.DISPOSED,
}

cdef class ComponentFSMFactory:
    """
    Provides generic component Finite-State Machines.
    """

    @staticmethod
    def get_state_transition_table() -> dict:
        """
        The default state transition table.

        Returns
        -------
        dict[int, int]
            C enums.
        """
        return _COMPONENT_STATE_TABLE.copy()

    @staticmethod
    cdef create():
        """
        Create a new generic component FSM.

        Returns
        -------
        FiniteStateMachine

        """
        return FiniteStateMachine(
            state_transition_table=ComponentFSMFactory.get_state_transition_table(),
            initial_state=ComponentState.INITIALIZED,
            trigger_parser=ComponentTriggerParser.to_string,
            state_parser=ComponentStateParser.to_string,
        )
