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

from nautilus_trader.execution.reports import ExecutionStateReport
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.stubs import TestStubs


class ExecutionCacheFacadeTests(unittest.TestCase):
    def test_empty_execution_state_report(self):
        # Arrange
        venue = Venue("SIM")
        account_id = TestStubs.account_id()

        # Act
        report = ExecutionStateReport(
            name=venue.value,
            account_id=account_id,
            order_states={},
            order_filled={},
            position_states={},
        )

        # Assert
        self.assertEqual(venue.value, report.name)
        self.assertEqual(account_id, report.account_id)
        self.assertEqual({}, report.order_states)
        self.assertEqual({}, report.order_filled)
        self.assertEqual({}, report.position_states)
