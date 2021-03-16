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
import unittest

from nautilus_trader.common.timer import TimeEvent
from nautilus_trader.common.timer import TimeEventHandler
from nautilus_trader.common.timer import Timer
from nautilus_trader.core.uuid import uuid4
from tests.test_kit.stubs import UNIX_EPOCH


class TimeEventTests(unittest.TestCase):
    def test_equality(self):
        # Arrange
        event1 = TimeEvent("EVENT_1", uuid4(), UNIX_EPOCH)
        event2 = TimeEvent("EVENT_1", uuid4(), UNIX_EPOCH)
        event3 = TimeEvent("EVENT_2", uuid4(), UNIX_EPOCH)

        # Act
        # Assert
        self.assertTrue(event1 == event1)
        self.assertTrue(event1 == event2)
        self.assertTrue(event1 != event3)

    def test_str_repr(self):
        # Arrange
        uuid = uuid4()
        event = TimeEvent("EVENT", uuid, UNIX_EPOCH)

        # Act
        # Assert
        self.assertEqual(
            f"TimeEvent(name=EVENT, id={uuid}, timestamp=1970-01-01T00:00:00.000Z)",
            str(event),
        )  # noqa
        self.assertEqual(
            f"TimeEvent(name=EVENT, id={uuid}, timestamp=1970-01-01T00:00:00.000Z)",
            repr(event),
        )  # noqa


class TimeEventHandlerTests(unittest.TestCase):
    def test_comparisons(self):
        # Arrange
        receiver = []
        event1 = TimeEventHandler(
            TimeEvent("123", uuid4(), UNIX_EPOCH), receiver.append
        )
        event2 = TimeEventHandler(
            TimeEvent("123", uuid4(), UNIX_EPOCH + timedelta(1)), receiver.append
        )

        # Act
        # Assert
        self.assertTrue(event1 == event1)
        self.assertTrue(event1 != event2)
        self.assertTrue(event1 < event2)
        self.assertTrue(event1 <= event2)
        self.assertTrue(event2 > event1)
        self.assertTrue(event2 >= event1)

    def test_str_repr(self):
        # Arrange
        receiver = []
        uuid = uuid4()
        handler = TimeEventHandler(TimeEvent("123", uuid, UNIX_EPOCH), receiver.append)

        # Act
        # Assert
        self.assertEqual(
            f"TimeEventHandler(event=TimeEvent(name=123, id={uuid}, timestamp=1970-01-01T00:00:00.000Z))",
            str(handler),
        )  # noqa
        self.assertEqual(
            f"TimeEventHandler(event=TimeEvent(name=123, id={uuid}, timestamp=1970-01-01T00:00:00.000Z))",
            repr(handler),
        )  # noqa

    def test_sort(self):
        # Arrange
        receiver = []
        event1 = TimeEventHandler(
            TimeEvent("123", uuid4(), UNIX_EPOCH), receiver.append
        )
        event2 = TimeEventHandler(
            TimeEvent("123", uuid4(), UNIX_EPOCH), receiver.append
        )
        event3 = TimeEventHandler(
            TimeEvent("123", uuid4(), UNIX_EPOCH + timedelta(1)), receiver.append
        )

        # Act
        # Stable sort as event1 and event2 remain in order
        result = sorted([event3, event1, event2])

        # Assert
        self.assertEqual([event1, event2, event3], result)


class TimerTests(unittest.TestCase):
    def test_equality(self):
        # Arrange
        receiver = []
        timer1 = Timer(
            "TIMER_1",
            receiver.append,
            timedelta(seconds=1),
            UNIX_EPOCH,
        )

        timer2 = Timer(
            "TIMER_2",
            receiver.append,
            timedelta(seconds=1),
            UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertTrue(timer1 == timer1)
        self.assertTrue(timer1 != timer2)

    def test_str_repr(self):
        # Arrange
        receiver = []
        timer = Timer(
            "TIMER_1",
            receiver.append,
            timedelta(seconds=1),
            UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertEqual(
            "Timer(name=TIMER_1, interval=0:00:01, start_time=1970-01-01 00:00:00+00:00, next_time=1970-01-01 00:00:01+00:00, stop_time=None)",
            str(timer),
        )  # noqa
        self.assertEqual(
            "Timer(name=TIMER_1, interval=0:00:01, start_time=1970-01-01 00:00:00+00:00, next_time=1970-01-01 00:00:01+00:00, stop_time=None)",
            repr(timer),
        )  # noqa

    def test_hash(self):
        # Arrange
        receiver = []
        timer = Timer(
            "TIMER_1",
            receiver.append,
            timedelta(seconds=1),
            UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertEqual(int, type(hash(timer)))
        self.assertEqual(hash(timer), hash(timer))

    def test_cancel_when_not_overridden_raises_not_implemented_error(self):
        # Arrange
        receiver = []
        timer = Timer(
            "TIMER_1",
            receiver.append,
            timedelta(seconds=1),
            UNIX_EPOCH,
        )

        # Act
        # Assert
        self.assertRaises(NotImplementedError, timer.cancel)
