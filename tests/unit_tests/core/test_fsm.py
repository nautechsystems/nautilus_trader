import pytest

from nautilus_trader.common.component import ComponentFSMFactory
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.enums import ComponentTrigger
from nautilus_trader.common.enums import component_state_to_str
from nautilus_trader.core.fsm import FiniteStateMachine
from nautilus_trader.core.fsm import InvalidStateTrigger


class TestFiniteStateMachine:
    def setup(self):
        # Fixture Setup
        self.fsm = FiniteStateMachine(
            state_transition_table=ComponentFSMFactory.get_state_transition_table(),
            initial_state=ComponentState.READY,
            state_parser=component_state_to_str,
        )

    def test_fsm_initialization(self):
        # Arrange, Act, Assert
        assert self.fsm.state == ComponentState.READY
        assert self.fsm.state_string == "READY"

    def test_trigger_with_invalid_transition_raises_exception(self):
        # Arrange
        fsm = FiniteStateMachine(
            state_transition_table=ComponentFSMFactory.get_state_transition_table(),
            initial_state=ComponentState.READY,
            state_parser=None,
            trigger_parser=None,
        )  # Invalid trigger will call parsers for ex msg

        # Act, Assert
        with pytest.raises(InvalidStateTrigger):
            fsm.trigger(ComponentState.RUNNING)

    def test_trigger_with_valid_transition_results_in_expected_state(self):
        # Arrange, Act
        self.fsm.trigger(ComponentTrigger.START)

        # Assert
        assert self.fsm.state == ComponentState.STARTING
        assert self.fsm.state_string == "STARTING"
