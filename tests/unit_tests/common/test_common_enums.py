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

import unittest

from parameterized import parameterized

from nautilus_trader.common.c_enums.component_state import ComponentState
from nautilus_trader.common.c_enums.component_state import ComponentStateParser
from nautilus_trader.common.c_enums.component_trigger import ComponentTrigger
from nautilus_trader.common.c_enums.component_trigger import ComponentTriggerParser


class ComponentStateTests(unittest.TestCase):
    @parameterized.expand(
        [
            [ComponentState.UNDEFINED, "UNDEFINED"],
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
        ]
    )
    def test_component_state_to_str(self, enum, expected):
        # Arrange
        # Act
        result = ComponentStateParser.to_str_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand(
        [
            ["", ComponentState.UNDEFINED],
            ["UNDEFINED", ComponentState.UNDEFINED],
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
        ]
    )
    def test_component_state_from_str(self, string, expected):
        # Arrange
        # Act
        result = ComponentStateParser.from_str_py(string)

        # Assert
        self.assertEqual(expected, result)


class ComponentTriggerTests(unittest.TestCase):
    @parameterized.expand(
        [
            [ComponentTrigger.UNDEFINED, "UNDEFINED"],
            [ComponentTrigger.START, "START"],
            [ComponentTrigger.RUNNING, "RUNNING"],
            [ComponentTrigger.STOP, "STOP"],
            [ComponentTrigger.STOPPED, "STOPPED"],
            [ComponentTrigger.RESUME, "RESUME"],
            [ComponentTrigger.RESET, "RESET"],
            [ComponentTrigger.DISPOSE, "DISPOSE"],
            [ComponentTrigger.DISPOSED, "DISPOSED"],
        ]
    )
    def test_component_trigger_to_str(self, enum, expected):
        # Arrange
        # Act
        result = ComponentTriggerParser.to_str_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand(
        [
            ["", ComponentTrigger.UNDEFINED],
            ["UNDEFINED", ComponentTrigger.UNDEFINED],
            ["START", ComponentTrigger.START],
            ["RUNNING", ComponentTrigger.RUNNING],
            ["STOP", ComponentTrigger.STOP],
            ["STOPPED", ComponentTrigger.STOPPED],
            ["RESUME", ComponentTrigger.RESUME],
            ["RESET", ComponentTrigger.RESET],
            ["DISPOSE", ComponentTrigger.DISPOSE],
            ["DISPOSED", ComponentTrigger.DISPOSED],
        ]
    )
    def test_component_trigger_from_str(self, string, expected):
        # Arrange
        # Act
        result = ComponentTriggerParser.from_str_py(string)

        # Assert
        self.assertEqual(expected, result)
