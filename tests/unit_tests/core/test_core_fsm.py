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

import unittest

from nautilus_trader.common.component import get_state_transition_table
from nautilus_trader.core.fsm import FiniteStateMachine
from nautilus_trader.core.fsm import InvalidStateTransition
from nautilus_trader.model.enums import ComponentState


class FiniteStateMachineTests(unittest.TestCase):

    @staticmethod
    def component_state_to_string(value: int):
        if value == 1:
            return 'INITIALIZED'
        elif value == 2:
            return 'STARTING'
        elif value == 2:
            return 'RUNNING'
        elif value == 2:
            return 'STOPPING'
        elif value == 2:
            return 'STOPPED'
        elif value == 2:
            return 'RESETTING'
        elif value == 2:
            return 'FAULTED'
        else:
            return 'UNDEFINED'

    def setUp(self):
        # Fixture setup
        self.fsm = FiniteStateMachine(
            state_transition_table=get_state_transition_table(),
            initial_state=ComponentState.INITIALIZED,
            state_parser=FiniteStateMachineTests.component_state_to_string)

    def test_fsm_initialization(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(ComponentState.INITIALIZED, self.fsm.state)

    def test_state_as_string_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual('INITIALIZED', self.fsm.state_as_string())

    def test_trigger_with_invalid_transition_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(InvalidStateTransition, self.fsm.trigger, 'RUNNING')

    def test_trigger_with_valid_transition_results_in_expected_state(self):
        # Arrange
        # Act
        self.fsm.trigger('START')

        # Assert
        self.assertEqual(ComponentState.STARTING, self.fsm.state)
