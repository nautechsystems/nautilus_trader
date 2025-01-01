# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
Defines a generic `Finite-State Machine` (FSM).

The FSM operates with a state-transition table of tuples and C-level enums. The
intended use case is to ensure correct state transitions, as well as holding a
deterministic state value.

References
----------
https://en.wikipedia.org/wiki/Finite-state_machine

"""

from typing import Callable

from nautilus_trader.core.correctness cimport Condition


cdef class InvalidStateTrigger(Exception):
    """
    Represents an invalid trigger for the current state.
    """
    pass


cdef class FiniteStateMachine:
    """
    Provides a generic finite state machine.

    Parameters
    ----------
    state_transition_table : dict of tuples and states
        The state-transition table for the FSM consisting of a tuple of
        starting state and trigger as keys, and resulting states as values.
    initial_state : int / C Enum
        The initial state for the FSM.
    trigger_parser : Callable[[int], str], optional
        The trigger parser needed to convert C Enum ints into strings.
        If ``None`` then will just print the integer.
    state_parser : Callable[[int], str], optional
        The state parser needed to convert C Enum ints into strings.
        If ``None`` then will just print the integer.

    Raises
    ------
    ValueError
        If `state_transition_table` is empty.
    ValueError
        If `state_transition_table` key not tuple.
    ValueError
        If `trigger_parser` not of type `Callable` or ``None``.
    ValueError
        If `state_parser` not of type `Callable` or ``None``.
    """

    def __init__(
        self,
        dict state_transition_table not None,
        int initial_state,
        trigger_parser: Callable[[int], str] = str,
        state_parser: Callable[[int], str] = str,
    ):
        if trigger_parser is None:
            trigger_parser = str
        if state_parser is None:
            state_parser = str
        Condition.not_empty(state_transition_table, "state_transition_table")
        Condition.dict_types(state_transition_table, tuple, object, "state_transition_table")
        Condition.callable_or_none(trigger_parser, "trigger_parser")
        Condition.callable_or_none(state_parser, "state_parser")

        self._state_transition_table = state_transition_table
        self._trigger_parser = trigger_parser
        self._state_parser = state_parser

        self.state = initial_state

    cdef str state_string_c(self):
        return self._state_parser(self.state)

    @property
    def state_string(self) -> str:
        """
        Return the current state as a string.

        Returns
        -------
        str

        """
        return self.state_string_c()

    cpdef void trigger(self, int trigger):
        """
        Process the FSM with the given trigger. The trigger must be valid for
        the FSMs current state.

        Parameters
        ----------
        trigger : int / C Enum
            The trigger to combine with the current state providing the key for
            the transition table lookup.

        Raises
        ------
        InvalidStateTrigger
            If the state and `trigger` combination is not found in the transition table.

        """
        cdef int next_state = self._state_transition_table.get((self.state, trigger), -1)
        if next_state == -1:  # Invalid
            raise InvalidStateTrigger(f"{self.state_string_c()} -> {self._trigger_parser(trigger)}")

        self.state = next_state
