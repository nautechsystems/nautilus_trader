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

from libc.stdint cimport int64_t

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.c_enums.component_state cimport ComponentStateParser
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTrigger
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTriggerParser
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.events.system cimport ComponentStateChanged
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.bus cimport MessageBus


cdef dict _COMPONENT_STATE_TABLE = {
    (ComponentState.PRE_INITIALIZED, ComponentTrigger.INITIALIZE): ComponentState.INITIALIZED,
    (ComponentState.INITIALIZED, ComponentTrigger.RESET): ComponentState.RESETTING,  # Transitional state
    (ComponentState.INITIALIZED, ComponentTrigger.START): ComponentState.STARTING,  # Transitional state
    (ComponentState.INITIALIZED, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,  # Transitional state
    (ComponentState.RESETTING, ComponentTrigger.RESET): ComponentState.INITIALIZED,
    (ComponentState.STARTING, ComponentTrigger.RUNNING): ComponentState.RUNNING,
    (ComponentState.STARTING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.STARTING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.DEGRADE): ComponentState.DEGRADING,  # Transitional state
    (ComponentState.RUNNING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.RESUMING, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.RESUMING, ComponentTrigger.RUNNING): ComponentState.RUNNING,
    (ComponentState.RESUMING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.STOPPING, ComponentTrigger.STOPPED): ComponentState.STOPPED,
    (ComponentState.STOPPING, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.RESET): ComponentState.RESETTING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.RESUME): ComponentState.RESUMING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.DISPOSE): ComponentState.DISPOSING,  # Transitional state
    (ComponentState.STOPPED, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transitional state
    (ComponentState.DEGRADING, ComponentTrigger.DEGRADED): ComponentState.DEGRADED,
    (ComponentState.DEGRADED, ComponentTrigger.RESUME): ComponentState.RESUMING,  # Transitional state
    (ComponentState.DEGRADED, ComponentTrigger.STOP): ComponentState.STOPPING,  # Transitional state
    (ComponentState.DEGRADED, ComponentTrigger.FAULT): ComponentState.FAULTING,  # Transition state
    (ComponentState.DISPOSING, ComponentTrigger.DISPOSED): ComponentState.DISPOSED,  # Terminal state
    (ComponentState.FAULTING, ComponentTrigger.FAULTED): ComponentState.FAULTED,  # Terminal state
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
            initial_state=ComponentState.PRE_INITIALIZED,
            trigger_parser=ComponentTriggerParser.to_str,
            state_parser=ComponentStateParser.to_str,
        )


cdef class Component:
    """
    The abstract base class for all system components.

    A component is not considered initialized until a message bus is wired up
    (this either happens when one is passed to the constructor, or when
    registered with a trader).

    Thus if the component does not receive a message bus through the constructor,
    then it will be in a ``PRE_INITIALIZED`` state, otherwise ``INITIALIZED``.

    Parameters
    ----------
    clock : Clock
        The clock for the component.
    logger : Logger
        The logger for the component.
    trader_id : TraderId, optional
        The trader ID associated with the component.
    component_id : ComponentId, optional
        The component ID. If ``None`` is passed then the identifier will be
        taken from `type(self).__name__`.
    component_name : str, optional
        The custom component name.
    msgbus : MessageBus, optional
        The message bus for the component (required before initialized).
    config : dict[str, Any], optional
        The configuration for the component.

    Raises
    ------
    ValueError
        If `component_name` is not a valid string.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        Clock clock not None,
        Logger logger not None,
        TraderId trader_id=None,
        ComponentId component_id=None,
        str component_name=None,
        MessageBus msgbus=None,
        dict config=None,
    ):
        if config is None:
            config = {}
        if component_id is None:
            component_id = ComponentId(type(self).__name__)
        if component_name is None:
            component_name = component_id.value
        Condition.valid_string(component_name, "component_name")

        self.trader_id = msgbus.trader_id if msgbus is not None else None
        self.id = component_id
        self.type = type(self)

        self._msgbus = msgbus
        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(component_name=component_name, logger=logger)
        self._fsm = ComponentFSMFactory.create()
        self._config = config

        if self._msgbus is not None:
            self._initialize()

    def __eq__(self, Component other) -> bool:
        return self.id.value == other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __str__(self) -> str:
        return self.id.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.id})"

    cdef ComponentState state_c(self) except *:
        return <ComponentState>self._fsm.state

    cdef str state_string_c(self):
        return self._fsm.state_string_c()

    cdef bint is_initialized_c(self):
        return self._fsm.state >= ComponentState.INITIALIZED

    cdef bint is_running_c(self):
        return self._fsm.state == ComponentState.RUNNING

    cdef bint is_stopped_c(self):
        return self._fsm.state == ComponentState.STOPPED

    cdef bint is_disposed_c(self):
        return self._fsm.state == ComponentState.DISPOSED

    cdef bint is_degraded_c(self):
        return self._fsm.state == ComponentState.DEGRADED

    cdef bint is_faulted_c(self):
        return self._fsm.state == ComponentState.FAULTED

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
    def is_initialized(self):
        """
        If the component has been initialized (component.state >= ``INITIALIZED``).

        Returns
        -------
        bool

        """
        return self.is_initialized_c()

    @property
    def is_running(self):
        """
        If the current component state is ``RUNNING``.

        Returns
        -------
        bool

        """
        return self.is_running_c()

    @property
    def is_stopped(self):
        """
        If the current component state is ``STOPPED``.

        Returns
        -------
        bool

        """
        return self.is_stopped_c()

    @property
    def is_disposed(self):
        """
        If the current component state is ``DISPOSED``.

        Returns
        -------
        bool

        """
        return self.is_disposed_c()

    @property
    def is_degraded(self):
        """
        If the current component state is ``DEGRADED``.

        Returns
        -------
        bool

        """
        return self.is_degraded_c()

    @property
    def is_faulted(self):
        """
        If the current component state is ``FAULTED``.

        Returns
        -------
        bool

        """
        return self.is_faulted_c()

    cdef void _change_clock(self, Clock clock) except *:
        Condition.not_none(clock, "clock")

        self._clock = clock

    cdef void _change_logger(self, Logger logger) except *:
        Condition.not_none(logger, "logger")

        self._log = LoggerAdapter(component_name=self.id.value, logger=logger)

    cdef void _change_msgbus(self, MessageBus msgbus) except *:
        # As an additional system wiring check: if a message bus is being added
        # here then there should not be an existing trader ID or message bus.
        Condition.not_none(msgbus, "msgbus")
        Condition.none(self.trader_id, "self.trader_id")
        Condition.none(self._msgbus, "self._msgbus")

        self.trader_id = msgbus.trader_id
        self._msgbus = msgbus
        self._initialize()

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _start(self) except *:
        # Optionally override in subclass
        pass

    cpdef void _stop(self) except *:
        # Optionally override in subclass
        pass

    cpdef void _resume(self) except *:
        # Optionally override in subclass
        pass

    cpdef void _reset(self) except *:
        # Optionally override in subclass
        pass

    cpdef void _dispose(self) except *:
        # Optionally override in subclass
        pass

    cpdef void _degrade(self) except *:
        # Optionally override in subclass
        pass

    cpdef void _fault(self) except *:
        # Optionally override in subclass
        pass

# -- COMMANDS --------------------------------------------------------------------------------------

    cdef void _initialize(self) except *:
        # This is a protected method dependent on registration of a message bus
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.INITIALIZE,  # -> INITIALIZED
                is_transitory=False,
                action=None,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise

    cpdef void start(self) except *:
        """
        Start the component.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current component state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.START,  # -> STARTING
                is_transitory=True,
                action=self._start,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._trigger_fsm(
                trigger=ComponentTrigger.RUNNING,
                is_transitory=False,
                action=None,
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
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.STOP,  # -> STOPPING
                is_transitory=True,
                action=self._stop,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._trigger_fsm(
                trigger=ComponentTrigger.STOPPED,
                is_transitory=False,
                action=None,
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
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.RESUME,  # -> RESUMING
                is_transitory=True,
                action=self._resume,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._trigger_fsm(
                trigger=ComponentTrigger.RUNNING,
                is_transitory=False,
                action=None,
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
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.RESET,  # -> RESETTING
                is_transitory=True,
                action=self._reset,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._trigger_fsm(
                trigger=ComponentTrigger.RESET,
                is_transitory=False,
                action=None,
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
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.DISPOSE,  # -> DISPOSING
                is_transitory=True,
                action=self._dispose,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._trigger_fsm(
                trigger=ComponentTrigger.DISPOSED,
                is_transitory=False,
                action=None,
            )

    cpdef void degrade(self) except *:
        """
        Degrade the component.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current component state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.DEGRADE,  # -> DEGRADING
                is_transitory=True,
                action=self._degrade,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._trigger_fsm(
                trigger=ComponentTrigger.DEGRADED,
                is_transitory=False,
                action=None,
            )

    cpdef void fault(self) except *:
        """
        Fault the component.

        This method is idempotent and irreversible. No other methods should be
        called after faulting.

        Raises
        ------
        InvalidStateTrigger
            If invalid trigger from current component state.

        Warnings
        --------
        Do not override.

        Exceptions raised will be caught, logged, and reraised.

        """
        try:
            self._trigger_fsm(
                trigger=ComponentTrigger.FAULT,  # -> FAULTING
                is_transitory=True,
                action=self._fault,
            )
        except Exception as ex:
            self._log.exception(ex)
            raise
        finally:
            self._trigger_fsm(
                trigger=ComponentTrigger.FAULTED,
                is_transitory=False,
                action=None,
            )

# --------------------------------------------------------------------------------------------------

    cdef void _trigger_fsm(
        self,
        ComponentTrigger trigger,
        bint is_transitory,
        action: Callable[[None], None]=None,
    ) except *:
        try:
            self._fsm.trigger(trigger)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            raise  # Guards against component being put in an invalid state

        self._log.info(f"{self._fsm.state_string_c()}.{'..' if is_transitory else ''}")

        if action is not None:
            action()

        if not self.is_initialized_c():
            return  # Cannot publish event

        cdef int64_t now = self._clock.timestamp_ns()
        cdef ComponentStateChanged event = ComponentStateChanged(
            trader_id=self.trader_id,
            component_id=self.id,
            component_type=self.type.__name__,
            state=self._fsm.state,
            config=self._config,
            event_id=self._uuid_factory.generate(),
            ts_event=now,
            ts_init=now,
        )

        self._msgbus.publish(
            topic=f"events.system.{self.id}",
            msg=event,
        )
