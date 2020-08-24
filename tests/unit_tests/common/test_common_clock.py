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

from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.common.timer import TimeEvent
from nautilus_trader.common.timer import TimeEventHandler
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.live.clock import LiveClock
from tests.test_kit.stubs import UNIX_EPOCH


class TimeEventTests(unittest.TestCase):

    def test_can_hash_time_event(self):
        # Arrange
        event = TimeEvent("123", uuid4(), UNIX_EPOCH)

        # Act
        result = hash(event)

        # Assert
        self.assertEqual(int, type(result))  # No assertions raised

    def test_can_sort_time_events(self):
        # Arrange
        event1 = TimeEvent("123", uuid4(), UNIX_EPOCH)
        event2 = TimeEvent("123", uuid4(), UNIX_EPOCH + timedelta(1))

        # Act
        result = sorted([event2, event1])

        # Assert
        self.assertEqual([event1, event2], result)


class LiveClockTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.handler = []
        self.clock = LiveClock()
        self.clock.register_default_handler(self.handler.append)

    def tearDown(self):
        self.clock.cancel_all_timers()

    def test_instantiated_clock(self):
        # Arrange
        # Act
        # Assert
        self.assertTrue(self.clock.is_default_handler_registered)
        self.assertEqual([], self.clock.get_timer_names())

    def test_time_now(self):
        # Arrange
        # Act
        result = self.clock.time_now()

        # Assert
        self.assertEqual(pytz.utc, result.tzinfo)
        self.assertEqual(datetime, type(result))

    def test_get_delta(self):
        # Arrange
        start = self.clock.time_now()

        # Act
        time.sleep(0.1)
        result = self.clock.get_delta(start)

        # Assert
        self.assertTrue(result > timedelta(0))
        self.assertEqual(timedelta, type(result))

    def test_can_set_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.time_now() + interval

        # Act
        self.clock.set_time_alert(name, alert_time)
        time.sleep(0.2)

        # Assert
        self.assertEqual(1, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_can_cancel_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.time_now() + interval

        self.clock.set_time_alert(name, alert_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(0, len(self.handler))

    def test_can_set_multiple_time_alerts(self):
        # Arrange
        alert_time1 = self.clock.time_now() + timedelta(milliseconds=200)
        alert_time2 = self.clock.time_now() + timedelta(milliseconds=300)

        # Act
        self.clock.set_time_alert("TEST_ALERT1", alert_time1)
        self.clock.set_time_alert("TEST_ALERT2", alert_time2)
        time.sleep(0.5)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(2, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))
        self.assertTrue(isinstance(self.handler[1], TimeEvent))

    def test_can_set_timer_with_immediate_start_time(self):
        # Arrange
        name = "TEST_TIMER"

        # Act
        self.clock.set_timer(
            name=name,
            interval=timedelta(milliseconds=100),
            start_time=None,
            stop_time=None)

        time.sleep(0.5)

        # Assert
        self.assertEqual([name], self.clock.get_timer_names())
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_can_set_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.time_now() + interval

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=None)

        time.sleep(0.55)

        # Assert
        self.assertEqual([name], self.clock.get_timer_names())
        self.assertEqual(4, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_can_set_timer_with_stop_time(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.time_now()
        stop_time = start_time + interval

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time)

        time.sleep(0.5)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(1, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))

    def test_can_cancel_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        self.clock.set_timer(name=name, interval=interval)

        # Act
        time.sleep(0.25)
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(2, len(self.handler))

    def test_can_set_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.time_now()

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=None)

        time.sleep(0.55)

        # Assert
        self.assertEqual([name], self.clock.get_timer_names())
        self.assertEqual(5, len(self.handler))
        self.assertTrue(isinstance(self.handler[0], TimeEvent))
        self.assertTrue(isinstance(self.handler[1], TimeEvent))
        self.assertTrue(isinstance(self.handler[2], TimeEvent))

    def test_can_cancel_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.time_now()
        stop_time = start_time + timedelta(seconds=5)

        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=start_time,
            stop_time=stop_time)

        # Act
        time.sleep(0.35)
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual(3, len(self.handler))

    def test_can_set_two_repeating_timers(self):
        # Arrange
        interval = timedelta(milliseconds=100)
        start_time = self.clock.time_now() + timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name="TEST_TIMER1",
            interval=interval,
            start_time=start_time,
            stop_time=None)

        self.clock.set_timer(
            name="TEST_TIMER2",
            interval=interval,
            start_time=start_time,
            stop_time=None)

        time.sleep(0.55)

        # Assert
        self.assertEqual(8, len(self.handler))


class TestClockTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.handler = []
        self.clock = TestClock()
        self.clock.register_default_handler(self.handler.append)

    def tearDown(self):
        self.clock.cancel_all_timers()

    def test_instantiated_clock(self):
        # Arrange
        # Act
        # Assert
        self.assertTrue(self.clock.is_default_handler_registered)
        self.assertEqual([], self.clock.get_timer_names())

    def test_time_now(self):
        # Arrange
        # Act
        result = self.clock.time_now()

        # Assert
        self.assertEqual(pytz.utc, result.tzinfo)
        self.assertEqual(datetime, type(result))

    def test_get_delta(self):
        # Arrange
        start = self.clock.time_now()

        # Act
        self.clock.set_time(start + timedelta(1))
        result = self.clock.get_delta(start)

        # Assert
        self.assertTrue(result > timedelta(0))
        self.assertEqual(timedelta, type(result))

    def test_can_set_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        alert_time = self.clock.time_now() + timedelta(milliseconds=100)

        # Act
        self.clock.set_time_alert(name, alert_time)
        events = self.clock.advance_time(self.clock.time_now() + timedelta(milliseconds=200))

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(1, len(events))
        self.assertEqual(TimeEventHandler, type(events[0]))

    def test_can_cancel_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.time_now() + interval

        self.clock.set_time_alert(name, alert_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(0, len(self.handler))

    def test_can_set_multiple_time_alerts(self):
        # Arrange
        alert_time1 = self.clock.time_now() + timedelta(milliseconds=200)
        alert_time2 = self.clock.time_now() + timedelta(milliseconds=300)

        # Act
        self.clock.set_time_alert("TEST_ALERT1", alert_time1)
        self.clock.set_time_alert("TEST_ALERT2", alert_time2)
        events = self.clock.advance_time(self.clock.time_now() + timedelta(milliseconds=300))

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(2, len(events))

    def test_can_set_timer_with_immediate_start_time(self):
        # Arrange
        name = "TEST_TIMER"

        # Act
        self.clock.set_timer(
            name=name,
            interval=timedelta(milliseconds=100),
            start_time=None,
            stop_time=None)
        events = self.clock.advance_time(self.clock.time_now() + timedelta(milliseconds=400))

        # Assert
        self.assertEqual([name], self.clock.get_timer_names())
        self.assertEqual(4, len(events))
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 400000, tzinfo=pytz.utc), events[3].event.timestamp)

    def test_can_set_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=None,
            stop_time=None)
        events = self.clock.advance_time(self.clock.time_now() + timedelta(milliseconds=400))

        # Assert
        self.assertEqual([name], self.clock.get_timer_names())
        self.assertEqual(4, len(events))

    def test_can_set_timer_with_stop_time(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            stop_time=self.clock.time_now() + timedelta(milliseconds=300))
        events = self.clock.advance_time(self.clock.time_now() + timedelta(milliseconds=300))

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(3, len(events))

    def test_can_cancel_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=self.clock.time_now() + timedelta(milliseconds=10),
            stop_time=None)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())

    def test_can_set_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=self.clock.time_now(),
            stop_time=None)

        events = self.clock.advance_time(self.clock.time_now() + timedelta(milliseconds=400))

        # Assert
        self.assertEqual([name], self.clock.get_timer_names())
        self.assertEqual(4, len(events))

    def test_can_cancel_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.time_now()
        stop_time = start_time + timedelta(seconds=5)

        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=self.clock.time_now(),
            stop_time=stop_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())

    def test_can_set_two_repeating_timers(self):
        # Arrange
        start_time = self.clock.time_now()
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name="TEST_TIMER1",
            interval=interval,
            start_time=self.clock.time_now(),
            stop_time=None)

        self.clock.set_timer(
            name="TEST_TIMER2",
            interval=interval,
            start_time=self.clock.time_now(),
            stop_time=None)

        events = self.clock.advance_time(self.clock.time_now() + timedelta(milliseconds=500))

        # Assert
        self.assertEqual(10, len(events))
        self.assertEqual(start_time + timedelta(milliseconds=500), self.clock.time_now())
