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

from datetime import timedelta
import unittest

from nautilus_trader.common.timer import TimeEvent
from nautilus_trader.core.uuid import uuid4
from tests.test_kit.stubs import UNIX_EPOCH


class TimeEventTests(unittest.TestCase):

    def test_hash_time_event(self):
        # Arrange
        event = TimeEvent("123", uuid4(), UNIX_EPOCH)

        # Act
        result = hash(event)

        # Assert
        self.assertEqual(int, type(result))  # No assertions raised

    def test_sort_time_events(self):
        # Arrange
        event1 = TimeEvent("123", uuid4(), UNIX_EPOCH)
        event2 = TimeEvent("123", uuid4(), UNIX_EPOCH + timedelta(1))

        # Act
        result = sorted([event2, event1])

        # Assert
        self.assertEqual([event1, event2], result)
