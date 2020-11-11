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

from datetime import datetime
from datetime import timedelta
import time
import unittest

import pytz

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.timer import TimeEvent
from nautilus_trader.core.uuid import uuid4
from tests.test_kit.stubs import UNIX_EPOCH


class TimeEventTests(unittest.TestCase):

    def test_sort_time_events(self):
        # Arrange
        event1 = TimeEvent("123", uuid4(), UNIX_EPOCH)
        event2 = TimeEvent("123", uuid4(), UNIX_EPOCH)
        event3 = TimeEvent("123", uuid4(), UNIX_EPOCH + timedelta(1))

        # Act
        # Stable sort as event1 and event2 remain in order
        result = sorted([event3, event1, event2])

        # Assert
        self.assertEqual([event1, event2, event3], result)


class TestClockTests(unittest.TestCase):

    def test_instantiate_has_expected_time_and_properties(self):
        # Arrange
        init_time = UNIX_EPOCH + timedelta(minutes=1)
        clock = TestClock(UNIX_EPOCH + timedelta(minutes=1))

        # Act
        # Assert
        self.assertEqual(init_time, clock.utc_now())
        self.assertTrue(clock.is_test_clock)

    def test_set_time_changes_time(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        clock.set_time(UNIX_EPOCH + timedelta(minutes=1))

        # Assert
        self.assertEqual(UNIX_EPOCH + timedelta(minutes=1), clock.utc_now())

    def test_advance_time_changes_time_produces_empty_list(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        events = clock.advance_time(UNIX_EPOCH + timedelta(minutes=1))

        # Assert
        self.assertEqual(UNIX_EPOCH + timedelta(minutes=1), clock.utc_now())
        self.assertEqual([], events)

    def test_advance_time_given_time_in_past_raises_value_error(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        # Assert
        self.assertRaises(ValueError, clock.advance_time, UNIX_EPOCH - timedelta(minutes=1))

    def test_local_now(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        result = clock.local_now(pytz.timezone("Australia/Sydney"))

        self.assertEqual("1970-01-01 10:00:00+10:00", str(result))

    def test_delta(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        events = clock.delta(UNIX_EPOCH - timedelta(minutes=9))

        self.assertEqual(timedelta(minutes=9), events)

    def test_cancel_timer_when_no_timers_does_nothing(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        clock.cancel_timer("BOGUS_ALERT")

        # Assert
        self.assertEqual([], clock.timer_names())
        self.assertEqual(0, clock.timer_count)

    def test_cancel_timers_when_no_timers_does_nothing(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        clock.cancel_timers()

        # Assert
        self.assertEqual([], clock.timer_names())
        self.assertEqual(0, clock.timer_count)

    def test_set_time_alert(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        name = "TEST_ALERT"
        interval = timedelta(minutes=10)
        alert_time = clock.utc_now() + interval
        handler = []

        # Act
        clock.set_time_alert(name, alert_time, handler.append)

        # Assert
        self.assertEqual(["TEST_ALERT"], clock.timer_names())
        self.assertEqual("TEST_ALERT", clock.timer("TEST_ALERT").name)
        self.assertEqual(1, clock.timer_count)

    def test_cancel_time_alert_when_timer_removes_timer(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=300)
        alert_time = clock.utc_now() + interval
        handler = []

        clock.set_time_alert(name, alert_time, handler.append)

        # Act
        clock.cancel_timer(name)

        # Assert
        self.assertEqual([], clock.timer_names())
        self.assertEqual(0, clock.timer_count)

    def test_cancel_timers_when_multiple_times_removes_all_timers(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        interval = timedelta(milliseconds=300)
        alert_time = clock.utc_now() + interval
        handler = []

        clock.set_time_alert("TEST_ALERT1", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT2", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT3", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT4", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT5", alert_time, handler.append)

        # Act
        clock.cancel_timers()

        # Assert
        self.assertEqual([], clock.timer_names())
        self.assertEqual(0, clock.timer_count)

    def test_set_timer(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        name = "TEST_TIMER"
        interval = timedelta(minutes=1)
        handler = []

        # Act
        clock.set_timer(
            name=name,
            interval=interval,
            start_time=UNIX_EPOCH + interval,
            stop_time=None,
            handler=handler.append
        )

        # Assert
        self.assertEqual(["TEST_TIMER"], clock.timer_names())
        self.assertEqual("TEST_TIMER", clock.timer("TEST_TIMER").name)
        self.assertEqual(1, clock.timer_count)

    def test_advance_time_with_set_time_alert_triggers_event(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        name = "TEST_ALERT"
        interval = timedelta(minutes=1)
        alert_time = clock.utc_now() + interval
        handler = []

        clock.set_time_alert(name, alert_time, handler.append)

        # Act
        event_handlers = clock.advance_time(UNIX_EPOCH + timedelta(minutes=2))

        # Assert
        self.assertEqual(1, len(event_handlers))
        self.assertEqual("TEST_ALERT", event_handlers[0].event.name)
        self.assertEqual([], clock.timer_names())
        self.assertEqual(0, clock.timer_count)

    def test_advance_time_with_multiple_set_time_alerts_triggers_event(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        interval = timedelta(minutes=1)
        alert_time = clock.utc_now() + interval
        handler = []

        clock.set_time_alert("TEST_ALERT1", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT2", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT3", alert_time, handler.append)

        # Act
        event_handlers = clock.advance_time(UNIX_EPOCH + timedelta(minutes=2))

        # Assert
        self.assertEqual(3, len(event_handlers))
        self.assertEqual("TEST_ALERT1", event_handlers[0].event.name)
        self.assertEqual("TEST_ALERT2", event_handlers[1].event.name)
        self.assertEqual("TEST_ALERT3", event_handlers[2].event.name)
        self.assertEqual([], clock.timer_names())
        self.assertEqual(0, clock.timer_count)

    def test_advance_time_with_set_timer_triggers_events(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        name = "TEST_TIMER"
        interval = timedelta(minutes=1)
        handler = []

        # Act
        clock.set_timer(
            name=name,
            interval=interval,
            start_time=UNIX_EPOCH + interval,
            stop_time=None,
            handler=handler.append
        )

        event_handlers = clock.advance_time(UNIX_EPOCH + timedelta(minutes=5))

        # Assert
        self.assertEqual(4, len(event_handlers))
        self.assertEqual("TEST_TIMER", event_handlers[0].event.name)
        self.assertEqual(["TEST_TIMER"], clock.timer_names())
        self.assertEqual("TEST_TIMER", clock.timer("TEST_TIMER").name)
        self.assertEqual(1, clock.timer_count)

    def test_advance_time_with_multiple_set_timers_triggers_events(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)
        name1 = "TEST_TIMER1"
        name2 = "TEST_TIMER2"
        interval1 = timedelta(minutes=1)
        interval2 = timedelta(seconds=30)
        handler1 = []
        handler2 = []

        # Act
        clock.set_timer(
            name=name1,
            interval=interval1,
            start_time=UNIX_EPOCH,
            stop_time=None,
            handler=handler1.append
        )

        clock.set_timer(
            name=name2,
            interval=interval2,
            start_time=UNIX_EPOCH,
            stop_time=None,
            handler=handler2.append
        )

        event_handlers = clock.advance_time(UNIX_EPOCH + timedelta(minutes=5))

        # Assert
        self.assertEqual(15, len(event_handlers))
        self.assertEqual("TEST_TIMER2", event_handlers[0].event.name)
        self.assertEqual("TEST_TIMER1", event_handlers[1].event.name)
        self.assertEqual(["TEST_TIMER1", "TEST_TIMER2"], clock.timer_names())
        self.assertEqual("TEST_TIMER1", clock.timer("TEST_TIMER1").name)
        self.assertEqual("TEST_TIMER2", clock.timer("TEST_TIMER2").name)
        self.assertEqual(2, clock.timer_count)


class LiveClockTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.handler = []
        self.clock = LiveClock()
        self.clock.register_default_handler(self.handler.append)

    def tearDown(self):
        self.clock.cancel_timers()

    def test_instantiated_clock(self):
        # Arrange
        # Act
        # Assert
        self.assertTrue(self.clock.is_default_handler_registered)
        self.assertFalse(self.clock.is_test_clock)
        self.assertEqual([], self.clock.timer_names())

    def test_utc_now(self):
        # Arrange
        # Act
        result = self.clock.utc_now()

        # Assert
        self.assertEqual(datetime, type(result))
        self.assertEqual(pytz.utc, result.tzinfo)

    def test_local_now(self):
        # Arrange
        # Act
        result = self.clock.local_now(pytz.timezone("Australia/Sydney"))

        # Assert
        self.assertEqual(datetime, type(result))
        self.assertTrue(str(result).endswith("+11:00"))

    def test_delta(self):
        # Arrange
        start = self.clock.utc_now()

        # Act
        time.sleep(0.1)
        result = self.clock.delta(start)

        # Assert
        self.assertTrue(result > timedelta(0))
        self.assertEqual(timedelta, type(result))

    def test_set_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.utc_now() + interval

        # Act
        self.clock.set_time_alert(name, alert_time)
        time.sleep(0.3)

        # Assert
        self.assertEqual(1, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_cancel_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=300)
        alert_time = self.clock.utc_now() + interval

        self.clock.set_time_alert(name, alert_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.timer_names())
        self.assertEqual(0, len(self.handler))

    def test_set_multiple_time_alerts(self):
        # Arrange
        alert_time1 = self.clock.utc_now() + timedelta(milliseconds=200)
        alert_time2 = self.clock.utc_now() + timedelta(milliseconds=300)

        # Act
        self.clock.set_time_alert("TEST_ALERT1", alert_time1)
        self.clock.set_time_alert("TEST_ALERT2", alert_time2)
        time.sleep(0.6)

        # Assert
        self.assertEqual([], self.clock.timer_names())
        self.assertEqual(2, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))
        self.assertTrue(isinstance(self.handler[1], TimeEvent))

    def test_set_timer_with_immediate_start_time(self):
        # Arrange
        name = "TEST_TIMER"

        # Act
        self.clock.set_timer(
            name=name,
            interval=timedelta(milliseconds=100),
            start_time=None,
            stop_time=None,
        )

        time.sleep(0.5)

        # Assert
        self.assertEqual([name], self.clock.timer_names())
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_set_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.utc_now() + interval

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=None,
        )

        time.sleep(0.5)

        # Assert
        self.assertEqual([name], self.clock.timer_names())
        self.assertTrue(len(self.handler) >= 2)
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_set_timer_with_stop_time(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.utc_now()
        stop_time = start_time + interval

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time,
        )

        time.sleep(0.5)

        # Assert
        self.assertEqual([], self.clock.timer_names())
        self.assertEqual(1, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_cancel_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        self.clock.set_timer(name=name, interval=interval)

        # Act
        time.sleep(0.3)
        self.clock.cancel_timer(name)
        time.sleep(0.3)

        # Assert
        self.assertEqual([], self.clock.timer_names())
        self.assertTrue(len(self.handler) <= 4)

    def test_set_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.utc_now()

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=None,
        )

        time.sleep(0.5)

        # Assert
        self.assertTrue(len(self.handler) >= 3)
        self.assertTrue(isinstance(self.handler[0], TimeEvent))
        self.assertTrue(isinstance(self.handler[1], TimeEvent))
        self.assertTrue(isinstance(self.handler[2], TimeEvent))

    def test_cancel_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.utc_now()
        stop_time = start_time + timedelta(seconds=5)

        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time,
        )

        # Act
        time.sleep(0.3)
        self.clock.cancel_timer(name)
        time.sleep(0.3)

        # Assert
        self.assertTrue(len(self.handler) <= 5)

    def test_set_two_repeating_timers(self):
        # Arrange
        interval = timedelta(milliseconds=100)
        start_time = self.clock.utc_now() + timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name="TEST_TIMER1",
            interval=interval,
            start_time=start_time,
            stop_time=None,
        )

        self.clock.set_timer(
            name="TEST_TIMER2",
            interval=interval,
            start_time=start_time,
            stop_time=None,
        )

        time.sleep(0.9)

        # Assert
        self.assertTrue(len(self.handler) >= 8)
