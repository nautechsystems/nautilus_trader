
cdef class InvalidStateTrigger(Exception):
    pass


cdef class FiniteStateMachine:
    cdef dict _state_transition_table
    cdef object _trigger_parser
    cdef object _state_parser

    cdef readonly int state
    """The current state of the FSM.\n\n:returns: `int / C Enum`"""

    cdef str state_string_c(self)
    cpdef void trigger(self, int trigger)
