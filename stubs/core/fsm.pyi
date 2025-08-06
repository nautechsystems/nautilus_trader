from collections.abc import Callable

class InvalidStateTrigger(Exception):
    ...

class FiniteStateMachine:

    def __init__(
        self,
        state_transition_table: dict,
        initial_state: int,
        trigger_parser: Callable[[int], str] = ...,
        state_parser: Callable[[int], str] = ...,
    ) -> None: ...
    @property
    def state_string(self) -> str: ...
    def trigger(self, trigger: int) -> None: ...
