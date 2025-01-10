# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pickle

from nautilus_trader.common.events import TimeEvent
from nautilus_trader.core.uuid import UUID4


def test_time_event_equality():
    # Arrange
    event_id = UUID4()

    event1 = TimeEvent(
        "TEST_EVENT",
        event_id,
        1,
        2,
    )

    event2 = TimeEvent(
        "TEST_EVENT",
        event_id,
        1,
        2,
    )

    event3 = TimeEvent(
        "TEST_EVENT",
        UUID4(),
        1,
        2,
    )

    # Act, Assert
    assert event1.name == event2.name == event3.name
    assert event1 == event2
    assert event3 != event1
    assert event3 != event2


def test_time_event_picking():
    # Arrange
    event = TimeEvent(
        "TEST_EVENT",
        UUID4(),
        1,
        2,
    )

    # Act
    pickled = pickle.dumps(event)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Assert
    assert event == unpickled
