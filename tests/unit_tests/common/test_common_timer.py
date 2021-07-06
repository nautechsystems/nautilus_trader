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

from datetime import timedelta

import pytest

from nautilus_trader.common.timer import TimeEvent
from nautilus_trader.common.timer import TimeEventHandler
from nautilus_trader.common.timer import Timer
from nautilus_trader.core.uuid import uuid4
from tests.test_kit.stubs import UNIX_EPOCH


class TestTimeEvent:
    def test_equality(self):
        # Arrange
        event1 = TimeEvent("EVENT_1", uuid4(), UNIX_EPOCH, 0, 0)
        event2 = TimeEvent("EVENT_1", uuid4(), UNIX_EPOCH, 0, 0)
        event3 = TimeEvent("EVENT_2", uuid4(), UNIX_EPOCH, 0, 0)

        # Act
        # Assert
        assert event1 == event1
        assert event1 == event2
        assert event1 != event3

    def test_str_repr(self):
        # Arrange
        uuid = uuid4()
        event = TimeEvent("EVENT", uuid, UNIX_EPOCH, 0, 0)

        # Act
        # Assert
        assert str(event) == (
            f"TimeEvent(name=EVENT, id={uuid}, " f"event_timestamp=1970-01-01T00:00:00.000Z)"
        )
        assert repr(event) == (
            f"TimeEvent(name=EVENT, id={uuid}, " f"event_timestamp=1970-01-01T00:00:00.000Z)"
        )


class TestTimeEventHandler:
    def test_comparisons(self):
        # Arrange
        receiver = []
        event1 = TimeEventHandler(TimeEvent("123", uuid4(), UNIX_EPOCH, 0, 0), receiver.append)
        event2 = TimeEventHandler(
            TimeEvent(
                "123",
                uuid4(),
                UNIX_EPOCH + timedelta(seconds=1),
                1_000_000_000,
                0,
            ),
            receiver.append,
        )

        # Act
        # Assert
        assert event1 == event1
        assert event1 != event2
        assert event1 < event2
        assert event1 <= event2
        assert event2 > event1
        assert event2 >= event1

    def test_str_repr(self):
        # Arrange
        receiver = []
        uuid = uuid4()
        handler = TimeEventHandler(TimeEvent("123", uuid, UNIX_EPOCH, 0, 0), receiver.append)

        print(str(handler))
        # Act
        # Assert
        assert str(handler) == (
            f"TimeEventHandler(event=TimeEvent(name=123, id={uuid}, "
            f"event_timestamp=1970-01-01T00:00:00.000Z))"
        )
        assert repr(handler) == (
            f"TimeEventHandler(event=TimeEvent(name=123, id={uuid}, "
            f"event_timestamp=1970-01-01T00:00:00.000Z))"
        )

    def test_sort(self):
        # Arrange
        receiver = []
        event1 = TimeEventHandler(TimeEvent("123", uuid4(), UNIX_EPOCH, 0, 0), receiver.append)
        event2 = TimeEventHandler(TimeEvent("123", uuid4(), UNIX_EPOCH, 0, 0), receiver.append)
        event3 = TimeEventHandler(
            TimeEvent("123", uuid4(), UNIX_EPOCH + timedelta(1), 0, 0), receiver.append
        )

        # Act
        # Stable sort as event1 and event2 remain in order
        result = sorted([event3, event1, event2])

        # Assert
        assert result == [event1, event2, event3]


class TestTimer:
    def test_equality(self):
        # Arrange
        receiver = []
        timer1 = Timer(
            "TIMER_1",
            receiver.append,
            1_000_000_000,
            0,
        )

        timer2 = Timer(
            "TIMER_2",
            receiver.append,
            1_000_000_000,
            0,
        )

        # Act
        # Assert
        assert timer1 == timer1
        assert timer1 != timer2

    def test_str_repr(self):
        # Arrange
        receiver = []
        timer = Timer(
            "TIMER_1",
            receiver.append,
            1_000_000_000,
            1_000_000_000,
        )

        # Act
        # Assert
        assert str(timer) == (
            "Timer(name=TIMER_1, "
            "interval_ns=1000000000, "
            "start_time_ns=1000000000, "
            "next_time_ns=2000000000, "
            "stop_time_ns=0, "
            "is_expired=False)"
        )
        assert repr(timer) == (
            "Timer(name=TIMER_1, "
            "interval_ns=1000000000, "
            "start_time_ns=1000000000, "
            "next_time_ns=2000000000, "
            "stop_time_ns=0, "
            "is_expired=False)"
        )

    def test_hash(self):
        # Arrange
        receiver = []
        timer = Timer(
            "TIMER_1",
            receiver.append,
            1_000_000_000,
            0,
        )

        # Act
        # Assert
        assert isinstance(hash(timer), int)
        assert hash(timer) == hash(timer)

    def test_cancel_when_not_overridden_raises_not_implemented_error(self):
        # Arrange
        receiver = []
        timer = Timer(
            "TIMER_1",
            receiver.append,
            1_000_000_000,
            0,
        )

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            timer.cancel()
