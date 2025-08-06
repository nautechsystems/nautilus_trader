from collections.abc import Callable

class InvalidStateTrigger(Exception):
    """
    Represents an invalid trigger for the current state.
    """


class FiniteStateMachine:
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
        state_transition_table: dict,
        initial_state: int,
        trigger_parser: Callable[[int], str] = ...,
        state_parser: Callable[[int], str] = ...,
    ) -> None: ...
    @property
    def state_string(self) -> str:
        """
        Return the current state as a string.

        Returns
        -------
        str

        """
        ...
    def trigger(self, trigger: int) -> None:
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
        ...
