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
from nautilus_trader.common.timer import TimeEvent
from nautilus_trader.core.uuid import uuid4
from tests.test_kit.stubs import UNIX_EPOCH


class TimeEventTests(unittest.TestCase):

    def test_sort_time_events(self):
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
