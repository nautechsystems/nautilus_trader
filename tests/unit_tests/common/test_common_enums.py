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

import pytest

from nautilus_trader.common.c_enums.component_state import ComponentState
from nautilus_trader.common.c_enums.component_state import ComponentStateParser
from nautilus_trader.common.c_enums.component_trigger import ComponentTrigger
from nautilus_trader.common.c_enums.component_trigger import ComponentTriggerParser


class TestComponentState:
    def test_component_state_parser_given_invalid_value_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            ComponentStateParser.to_str_py(0)

        with pytest.raises(ValueError):
            ComponentStateParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum,expected",
        [
            [ComponentState.INITIALIZED, "INITIALIZED"],
            [ComponentState.STARTING, "STARTING"],
            [ComponentState.RUNNING, "RUNNING"],
            [ComponentState.STOPPING, "STOPPING"],
            [ComponentState.STOPPED, "STOPPED"],
            [ComponentState.RESUMING, "RESUMING"],
            [ComponentState.RESETTING, "RESETTING"],
            [ComponentState.DISPOSING, "DISPOSING"],
            [ComponentState.DISPOSED, "DISPOSED"],
            [ComponentState.FAULTED, "FAULTED"],
        ],
    )
    def test_component_state_to_str(self, enum, expected):
        # Arrange
        # Act
        result = ComponentStateParser.to_str_py(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "string,expected",
        [
            ["INITIALIZED", ComponentState.INITIALIZED],
            ["STARTING", ComponentState.STARTING],
            ["RUNNING", ComponentState.RUNNING],
            ["STOPPING", ComponentState.STOPPING],
            ["STOPPED", ComponentState.STOPPED],
            ["RESUMING", ComponentState.RESUMING],
            ["RESETTING", ComponentState.RESETTING],
            ["DISPOSING", ComponentState.DISPOSING],
            ["DISPOSED", ComponentState.DISPOSED],
            ["FAULTED", ComponentState.FAULTED],
        ],
    )
    def test_component_state_from_str(self, string, expected):
        # Arrange
        # Act
        result = ComponentStateParser.from_str_py(string)

        # Assert
        assert result == expected


class TestComponentTrigger:
    def test_component_trigger_parser_given_invalid_value_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            ComponentTriggerParser.to_str_py(0)

        with pytest.raises(ValueError):
            ComponentTriggerParser.from_str_py("")

    @pytest.mark.parametrize(
        "enum,expected",
        [
            [ComponentTrigger.START, "START"],
            [ComponentTrigger.RUNNING, "RUNNING"],
            [ComponentTrigger.STOP, "STOP"],
            [ComponentTrigger.STOPPED, "STOPPED"],
            [ComponentTrigger.RESUME, "RESUME"],
            [ComponentTrigger.RESET, "RESET"],
            [ComponentTrigger.DISPOSE, "DISPOSE"],
            [ComponentTrigger.DISPOSED, "DISPOSED"],
        ],
    )
    def test_component_trigger_to_str(self, enum, expected):
        # Arrange
        # Act
        result = ComponentTriggerParser.to_str_py(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "string,expected",
        [
            ["START", ComponentTrigger.START],
            ["RUNNING", ComponentTrigger.RUNNING],
            ["STOP", ComponentTrigger.STOP],
            ["STOPPED", ComponentTrigger.STOPPED],
            ["RESUME", ComponentTrigger.RESUME],
            ["RESET", ComponentTrigger.RESET],
            ["DISPOSE", ComponentTrigger.DISPOSE],
            ["DISPOSED", ComponentTrigger.DISPOSED],
        ],
    )
    def test_component_trigger_from_str(self, string, expected):
        # Arrange
        # Act
        result = ComponentTriggerParser.from_str_py(string)

        # Assert
        assert result == expected
