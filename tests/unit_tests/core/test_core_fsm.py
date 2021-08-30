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
from nautilus_trader.common.component import ComponentFSMFactory
from nautilus_trader.core.fsm import FiniteStateMachine
from nautilus_trader.core.fsm import InvalidStateTrigger


class TestFiniteStateMachine:
    def setup(self):
        # Fixture Setup
        self.fsm = FiniteStateMachine(
            state_transition_table=ComponentFSMFactory.get_state_transition_table(),
            initial_state=ComponentState.INITIALIZED,
            state_parser=ComponentStateParser.to_str_py,  # Calls python function wrapper
        )

    def test_fsm_initialization(self):
        # Arrange, Act, Assert
        assert self.fsm.state == ComponentState.INITIALIZED

    def test_trigger_with_invalid_transition_raises_exception(self):
        # Arrange
        fsm = FiniteStateMachine(
            state_transition_table=ComponentFSMFactory.get_state_transition_table(),
            initial_state=ComponentState.INITIALIZED,
            state_parser=None,
            trigger_parser=None,
        )  # Invalid trigger will call parsers for ex msg

        # Act, Assert
        with pytest.raises(InvalidStateTrigger):
            fsm.trigger(ComponentTrigger.RUNNING)

    def test_trigger_with_valid_transition_results_in_expected_state(self):
        # Arrange, Act
        self.fsm.trigger(ComponentTrigger.START)

        # Assert
        assert self.fsm.state == ComponentState.STARTING
