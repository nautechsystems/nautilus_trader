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

import warnings

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.c_enums.component_state cimport ComponentStateParser
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTrigger
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTriggerParser
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.fsm cimport InvalidStateTrigger


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
            C Enums.
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
            trigger_parser=ComponentTriggerParser.to_str,
            state_parser=ComponentStateParser.to_str,
        )


cdef class Component:
    """
    The abstract base class for all system components.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        Clock clock not None,
        Logger logger not None,
        ComponentId component_id=None,
        str component_name=None,
        bint log_initialized=True,
    ):
        """
        Initialize a new instance of the ``Component`` class.

        Parameters
        ----------
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        component_id : ComponentId, optional
            The component ID. If None is passed then the identifier will be
            taken from `type(self).__name__`.
        log_initialized : bool
            If the initial state should be logged.

        Raises
        ------
        ValueError
            If component_name is not a valid string.

        """
        if component_id is None:
            component_id = ComponentId(type(self).__name__)
        if component_name is None:
            component_name = component_id.value
        Condition.valid_string(component_name, "component_name")

        self.id = component_id

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(component_name=component_name, logger=logger)
        self._fsm = ComponentFSMFactory.create()

        if log_initialized:
            self._log.info(f"{self._fsm.state_string_c()}.")

    def __str__(self) -> str:
        return self.id.value

    def __repr__(self) -> str:
        return self.id.value

    cdef ComponentState state_c(self) except *:
        return <ComponentState>self._fsm.state

    cdef str state_string_c(self):
        return self._fsm.state_string_c()

    cdef bint is_running_c(self):
        return self._fsm.state == ComponentState.RUNNING

    @property
    def state(self):
        """
        The components current state.

        Returns
        -------
        ComponentState

        """
        return self.state_c()

    @property
    def is_running(self):
        """
        The components current state is RUNNING.

        Returns
        -------
        bool

        """
        return self.is_running_c()

    cdef void _change_clock(self, Clock clock) except *:
        Condition.not_none(clock, "clock")

        self._clock = clock

    cdef void _change_logger(self, Logger logger) except *:
        Condition.not_none(logger, "logger")

        self._log = LoggerAdapter(component_name=self.id.value, logger=logger)

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _start(self) except *:
        # Should override in subclass
        warnings.warn("_start was called when not overridden")

    cpdef void _stop(self) except *:
        # Should override in subclass
        warnings.warn("_stop was called when not overridden")

    cpdef void _resume(self) except *:
        # Should override in subclass
        warnings.warn("_resume was called when not overridden")

    cpdef void _reset(self) except *:
        # Should override in subclass
        warnings.warn("_reset was called when not overridden")

    cpdef void _dispose(self) except *:
        # Should override in subclass
        warnings.warn("_dispose was called when not overridden")

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void start(self) except *:
        """
        Start the component.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current strategy state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        self._trigger_fsm(
            trigger1=ComponentTrigger.START,  # -> STARTING
            trigger2=ComponentTrigger.RUNNING,
            action=self._start,
        )

    cpdef void stop(self) except *:
        """
        Stop the component.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current component state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        self._trigger_fsm(
            trigger1=ComponentTrigger.STOP,  # -> STOPPING
            trigger2=ComponentTrigger.STOPPED,
            action=self._stop,
        )

    cpdef void resume(self) except *:
        """
        Resume the component.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current component state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        self._trigger_fsm(
            trigger1=ComponentTrigger.RESUME,  # -> RESUMING
            trigger2=ComponentTrigger.RUNNING,
            action=self._resume,
        )

    cpdef void reset(self) except *:
        """
        Reset the component.

        All stateful fields are reset to their initial value.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current component state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        self._trigger_fsm(
            trigger1=ComponentTrigger.RESET,  # -> RESETTING
            trigger2=ComponentTrigger.RESET,
            action=self._reset,
        )

    cpdef void dispose(self) except *:
        """
        Dispose of the component.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current component state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        self._trigger_fsm(
            trigger1=ComponentTrigger.DISPOSE,  # -> DISPOSING
            trigger2=ComponentTrigger.DISPOSED,
            action=self._dispose,
        )

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger1,
        ComponentTrigger trigger2,
        action,
    ) except *:
        try:
            self._fsm.trigger(trigger1)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            raise  # Guards against component being put in an invalid state

        self._log.info(f"{self._fsm.state_string_c()}...")

        try:
            action()
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._fsm.trigger(trigger2)
            self._log.info(f"{self._fsm.state_string_c()}.")
