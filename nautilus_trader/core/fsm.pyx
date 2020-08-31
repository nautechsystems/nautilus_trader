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

from nautilus_trader.core.correctness cimport Condition


cdef class InvalidStateTrigger(Exception):
    """
    Represents an invalid trigger for the current state.
    """
    pass


cdef class FiniteStateMachine:
    """
    Provides a generic finite state machine.
    """

    def __init__(self,
                 dict state_transition_table not None,
                 object initial_state not None,
                 state_parser not None=str):
        """
        Initialize a new instance of the FiniteStateMachine class.

        Parameters
        ----------
        state_transition_table : dict of tuples and states
            The state transition table for the FSM consisting of a tuple of
            starting state and trigger as keys, and resulting states as values.
        initial_state : object
            The initial state for the FSM.
        state_parser : callable, optional
            The optional state parser is required to convert C enum ints into strings.

        Raises
        ------
        ValueError
            If state_transition_table is empty.
            If state_transition_table contains a key of type other than tuple.
            If state_parser is not of type Callable or None.

        """
        Condition.not_empty(state_transition_table, "state_transition_table")
        Condition.dict_types(state_transition_table, tuple, object, "state_transition_table")
        Condition.callable_or_none(state_parser, "state_parser")

        self._state_transition_table = state_transition_table
        self.state = initial_state
        self._state_parser = state_parser

    cpdef void trigger(self, str trigger) except *:
        """
        Process the FSM with the given trigger. The trigger must be valid for
        the FSMs current state.

        Parameters
        ----------
        trigger : str
            The trigger to combine with the current state providing the key for
            the transition table lookup.

        Raises
        ------
        InvalidStateTrigger
            If the state and trigger combination is not found in the transition table.

        """
        Condition.valid_string(trigger, "trigger")

        next_state = self._state_transition_table.get((self.state, trigger))
        if next_state is None:
            raise InvalidStateTrigger(f"{self.state_as_string()} -> {trigger}")

        self.state = next_state

    cpdef void force_set(self, object state) except *:
        """
        Force the FSM state to the given state.

        Parameters
        ----------
        state : object

        """
        Condition.not_none(state, "state")

        self.state = state

    cpdef str state_as_string(self):
        """
        Return the state as a string.

        Returns
        -------
        str

        """
        if self._state_parser is None:
            return self.state
        return self._state_parser(self.state)
