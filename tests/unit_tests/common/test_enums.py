import pytest

from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.enums import ComponentTrigger
from nautilus_trader.common.enums import component_state_from_str
from nautilus_trader.common.enums import component_state_to_str
from nautilus_trader.common.enums import component_trigger_from_str
from nautilus_trader.common.enums import component_trigger_to_str


class TestComponentState:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [ComponentState.PRE_INITIALIZED, "PRE_INITIALIZED"],
            [ComponentState.READY, "READY"],
            [ComponentState.STARTING, "STARTING"],
            [ComponentState.RUNNING, "RUNNING"],
            [ComponentState.STOPPING, "STOPPING"],
            [ComponentState.STOPPED, "STOPPED"],
            [ComponentState.RESUMING, "RESUMING"],
            [ComponentState.RESETTING, "RESETTING"],
            [ComponentState.DISPOSING, "DISPOSING"],
            [ComponentState.DISPOSED, "DISPOSED"],
            [ComponentState.DEGRADING, "DEGRADING"],
            [ComponentState.DEGRADED, "DEGRADED"],
            [ComponentState.FAULTING, "FAULTING"],
            [ComponentState.FAULTED, "FAULTED"],
        ],
    )
    def test_component_state_to_str(self, enum, expected):
        # Arrange, Act
        result = component_state_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["PRE_INITIALIZED", ComponentState.PRE_INITIALIZED],
            ["READY", ComponentState.READY],
            ["STARTING", ComponentState.STARTING],
            ["RUNNING", ComponentState.RUNNING],
            ["STOPPING", ComponentState.STOPPING],
            ["STOPPED", ComponentState.STOPPED],
            ["RESUMING", ComponentState.RESUMING],
            ["RESETTING", ComponentState.RESETTING],
            ["DISPOSING", ComponentState.DISPOSING],
            ["DISPOSED", ComponentState.DISPOSED],
            ["DEGRADING", ComponentState.DEGRADING],
            ["DEGRADED", ComponentState.DEGRADED],
            ["FAULTING", ComponentState.FAULTING],
            ["FAULTED", ComponentState.FAULTED],
        ],
    )
    def test_component_state_from_str(self, string, expected):
        # Arrange, Act
        result = component_state_from_str(string)

        # Assert
        assert result == expected


class TestComponentTrigger:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [ComponentTrigger.INITIALIZE, "INITIALIZE"],
            [ComponentTrigger.START, "START"],
            [ComponentTrigger.START_COMPLETED, "START_COMPLETED"],
            [ComponentTrigger.STOP, "STOP"],
            [ComponentTrigger.STOP_COMPLETED, "STOP_COMPLETED"],
            [ComponentTrigger.RESUME, "RESUME"],
            [ComponentTrigger.RESUME, "RESUME"],
            [ComponentTrigger.RESET, "RESET"],
            [ComponentTrigger.DISPOSE, "DISPOSE"],
            [ComponentTrigger.DISPOSE_COMPLETED, "DISPOSE_COMPLETED"],
            [ComponentTrigger.DEGRADE, "DEGRADE"],
            [ComponentTrigger.DEGRADE_COMPLETED, "DEGRADE_COMPLETED"],
            [ComponentTrigger.FAULT, "FAULT"],
            [ComponentTrigger.FAULT_COMPLETED, "FAULT_COMPLETED"],
        ],
    )
    def test_component_trigger_to_str(self, enum, expected):
        # Arrange, Act
        result = component_trigger_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["INITIALIZE", ComponentTrigger.INITIALIZE],
            ["START", ComponentTrigger.START],
            ["START_COMPLETED", ComponentTrigger.START_COMPLETED],
            ["STOP", ComponentTrigger.STOP],
            ["STOP_COMPLETED", ComponentTrigger.STOP_COMPLETED],
            ["RESUME", ComponentTrigger.RESUME],
            ["RESET", ComponentTrigger.RESET],
            ["DISPOSE", ComponentTrigger.DISPOSE],
            ["DISPOSE_COMPLETED", ComponentTrigger.DISPOSE_COMPLETED],
            ["DEGRADE", ComponentTrigger.DEGRADE],
            ["DEGRADE_COMPLETED", ComponentTrigger.DEGRADE_COMPLETED],
            ["FAULT", ComponentTrigger.FAULT],
            ["FAULT_COMPLETED", ComponentTrigger.FAULT_COMPLETED],
        ],
    )
    def test_component_trigger_from_str(self, string, expected):
        # Arrange, Act
        result = component_trigger_from_str(string)

        # Assert
        assert result == expected
