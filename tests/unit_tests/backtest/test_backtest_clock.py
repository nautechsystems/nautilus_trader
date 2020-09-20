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
import unittest

import pytz

from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.common.timer import TimeEventHandler
from tests.test_kit.stubs import UNIX_EPOCH


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
        self.assertEqual(UNIX_EPOCH.astimezone(tz=pytz.timezone("Australia/Sydney")), result)

    def test_get_delta(self):
        # Arrange
        start = self.clock.utc_now()

        # Act
        self.clock.set_time(start + timedelta(1))
        result = self.clock.get_delta(start)

        # Assert
        self.assertTrue(result > timedelta(0))
        self.assertEqual(timedelta, type(result))

    def test_can_set_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        alert_time = self.clock.utc_now() + timedelta(milliseconds=100)

        # Act
        self.clock.set_time_alert(name, alert_time)
        events = self.clock.advance_time(self.clock.utc_now() + timedelta(milliseconds=200))

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(1, len(events))
        self.assertEqual(TimeEventHandler, type(events[0]))

    def test_can_cancel_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.utc_now() + interval

        self.clock.set_time_alert(name, alert_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())
        self.assertEqual(0, len(self.handler))

    def test_can_set_multiple_time_alerts(self):
        # Arrange
        alert_time1 = self.clock.utc_now() + timedelta(milliseconds=200)
        alert_time2 = self.clock.utc_now() + timedelta(milliseconds=300)

        # Act
        self.clock.set_time_alert("TEST_ALERT1", alert_time1)
        self.clock.set_time_alert("TEST_ALERT2", alert_time2)
        events = self.clock.advance_time(self.clock.utc_now() + timedelta(milliseconds=300))

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
        events = self.clock.advance_time(self.clock.utc_now() + timedelta(milliseconds=400))

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
        events = self.clock.advance_time(self.clock.utc_now() + timedelta(milliseconds=400))

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
            stop_time=self.clock.utc_now() + timedelta(milliseconds=300))
        events = self.clock.advance_time(self.clock.utc_now() + timedelta(milliseconds=300))

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
            start_time=self.clock.utc_now() + timedelta(milliseconds=10),
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
            start_time=self.clock.utc_now(),
            stop_time=None)

        events = self.clock.advance_time(self.clock.utc_now() + timedelta(milliseconds=400))

        # Assert
        self.assertEqual([name], self.clock.get_timer_names())
        self.assertEqual(4, len(events))

    def test_can_cancel_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)
        start_time = self.clock.utc_now()
        stop_time = start_time + timedelta(seconds=5)

        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=self.clock.utc_now(),
            stop_time=stop_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        self.assertEqual([], self.clock.get_timer_names())

    def test_can_set_two_repeating_timers(self):
        # Arrange
        start_time = self.clock.utc_now()
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name="TEST_TIMER1",
            interval=interval,
            start_time=self.clock.utc_now(),
            stop_time=None)

        self.clock.set_timer(
            name="TEST_TIMER2",
            interval=interval,
            start_time=self.clock.utc_now(),
            stop_time=None)

        events = self.clock.advance_time(self.clock.utc_now() + timedelta(milliseconds=500))

        # Assert
        self.assertEqual(10, len(events))
        self.assertEqual(start_time + timedelta(milliseconds=500), self.clock.utc_now())
