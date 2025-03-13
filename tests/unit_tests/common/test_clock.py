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

import time
from datetime import datetime
from datetime import timedelta

import pandas as pd
import pytest
import pytz

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.component import TimeEventHandler
from nautilus_trader.common.events import TimeEvent
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH


class TestTestClock:
    def setup(self):
        # Fixture Setup
        self.handler = []
        self.clock = TestClock()
        self.clock.register_default_handler(self.handler.append)

    def teardown(self):
        self.clock.cancel_timers()

    def test_instantiated_clock(self):
        # Arrange, Act, Assert
        assert self.clock.timer_count == 0

    def test_utc_now_when_no_time_set(self):
        # Arrange, Act, Assert
        assert isinstance(self.clock.utc_now(), datetime)
        assert self.clock.utc_now().tzinfo == pytz.utc
        assert isinstance(self.clock.timestamp_ns(), int)

    def test_utc_now_when_time_set(self):
        # Arrange
        moment = pd.Timestamp("2000-01-01 10:00:00+00:00")
        self.clock.set_time(moment.value)

        # Act
        result = self.clock.utc_now()

        # Assert
        assert result == moment

    def test_local_now(self):
        # Arrange, Act
        result = self.clock.local_now(pytz.timezone("Australia/Sydney"))

        # Assert
        assert isinstance(result, datetime)
        assert result == UNIX_EPOCH.astimezone(tz=pytz.timezone("Australia/Sydney"))
        assert str(result) == "1970-01-01 10:00:00+10:00"

    def test_set_time_alert_advance_clock_within_next_alert(self):
        # Arrange
        name = "TEST_ALERT"
        alert_time = self.clock.utc_now() + timedelta(milliseconds=100)

        # Act
        self.clock.set_time_alert(name, alert_time)
        events = self.clock.advance_time(to_time_ns=millis_to_nanos(99))

        # Assert
        assert self.clock.timer_count == 1
        assert len(events) == 0

    def test_set_time_alert_advance_clock_beyond_next_alert_yields_event(self):
        # Arrange
        name = "TEST_ALERT"
        alert_time = self.clock.utc_now() + timedelta(milliseconds=100)

        # Act
        self.clock.set_time_alert(name, alert_time)
        events = self.clock.advance_time(to_time_ns=millis_to_nanos(150))

        # Assert
        assert self.clock.timer_count == 0
        assert len(events) == 1
        assert isinstance(events[0], TimeEventHandler)

    def test_cancel_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.utc_now() + interval

        self.clock.set_time_alert(name, alert_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        assert self.clock.timer_count == 0
        assert len(self.handler) == 0

    def test_set_multiple_time_alerts(self):
        # Arrange
        alert_time1 = self.clock.utc_now() + timedelta(milliseconds=200)
        alert_time2 = self.clock.utc_now() + timedelta(milliseconds=300)

        # Act
        self.clock.set_time_alert("TEST_ALERT1", alert_time1)
        self.clock.set_time_alert("TEST_ALERT2", alert_time2)
        events = self.clock.advance_time(to_time_ns=millis_to_nanos(300))

        # Assert
        assert self.clock.timer_count == 0
        assert len(events) == 2

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

        events = self.clock.advance_time(to_time_ns=millis_to_nanos(400))

        # Assert
        assert self.clock.timer_names == [name]
        assert len(events) == 4
        assert events[0].event.ts_event == 100_000_000
        assert events[1].event.ts_event == 200_000_000
        assert events[2].event.ts_event == 300_000_000
        assert events[3].event.ts_event == 400_000_000

    def test_set_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=None,
            stop_time=None,
        )

        events = self.clock.advance_time(to_time_ns=millis_to_nanos(400))

        # Assert
        assert self.clock.timer_names == [name]
        assert len(events) == 4

    def test_set_timer_with_stop_time(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            stop_time=self.clock.utc_now() + timedelta(milliseconds=300),
        )

        events = self.clock.advance_time(to_time_ns=millis_to_nanos(300))

        # Assert
        assert self.clock.timer_count == 0
        assert len(events) == 3

    def test_cancel_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=self.clock.utc_now() + timedelta(milliseconds=10),
            stop_time=None,
        )

        # Act
        self.clock.cancel_timer(name)

        # Assert
        assert self.clock.timer_count == 0

    def test_set_repeating_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name=name,
            interval=interval,
            start_time=self.clock.utc_now(),
            stop_time=None,
        )

        events = self.clock.advance_time(to_time_ns=millis_to_nanos(400))

        # Assert
        assert self.clock.timer_names == [name]
        assert len(events) == 4

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
        self.clock.cancel_timer(name)

        # Assert
        assert self.clock.timer_count == 0

    def test_set_two_repeating_timers(self):
        # Arrange
        start_time = self.clock.utc_now()
        interval = timedelta(milliseconds=100)

        # Act
        self.clock.set_timer(
            name="TEST_TIMER1",
            interval=interval,
            start_time=self.clock.utc_now(),
            stop_time=None,
        )

        self.clock.set_timer(
            name="TEST_TIMER2",
            interval=interval,
            start_time=self.clock.utc_now(),
            stop_time=None,
        )

        events = self.clock.advance_time(to_time_ns=millis_to_nanos(500))

        # Assert
        assert len(events) == 10
        assert self.clock.utc_now() == start_time + timedelta(milliseconds=500)
        assert self.clock.timestamp_ns() == 500_000_000

    def test_instantiate_has_expected_time_and_properties(self):
        # Arrange
        initial_ns = 42_000_000
        clock = TestClock()
        clock.set_time(initial_ns)

        # Act, Assert
        assert clock.timestamp_ns() == initial_ns

    def test_timestamp_returns_expected_datetime(self):
        # Arrange
        clock = TestClock()
        clock.advance_time(1_000_000_000)

        # Act
        result = clock.timestamp()

        # Assert
        assert isinstance(result, float)
        assert result == 1.0

    def test_timestamp_ms_returns_expected_datetime(self):
        # Arrange
        clock = TestClock()
        clock.advance_time(1_000_000_000)

        # Act
        result = clock.timestamp_ms()

        # Assert
        assert isinstance(result, int)
        assert result == 1000

    def test_timestamp_us_returns_expected_datetime(self):
        # Arrange
        clock = TestClock()
        clock.advance_time(1_000_000_000)

        # Act
        result = clock.timestamp_us()

        # Assert
        assert isinstance(result, int)
        assert result == 1_000_000

    def test_timestamp_ns_returns_expected_datetime(self):
        # Arrange
        clock = TestClock()
        clock.advance_time(1_000_000_000)

        # Act
        result = clock.timestamp_ns()

        # Assert
        assert isinstance(result, int)
        assert result == 1_000_000_000

    def test_timestamp_returns_expected_double(self):
        # Arrange
        clock = TestClock()
        clock.set_time(60_000_000_000)

        # Act
        result = clock.timestamp()

        # Assert
        assert result == 60

    def test_timestamp_ns_returns_expected_int64(self):
        # Arrange
        clock = TestClock()
        clock.set_time(60_000_000_000)

        # Act
        result = clock.timestamp_ns()

        # Assert
        assert result == 60_000_000_000

    def test_set_time_changes_time(self):
        # Arrange
        clock = TestClock()

        # Act
        clock.set_time(60_000_000_000)

        # Assert
        assert clock.timestamp_ns() == 60_000_000_000

    def test_advance_time_changes_time_produces_empty_list(self):
        # Arrange
        clock = TestClock()

        # Act
        events = clock.advance_time(1_000_000_000)

        # Assert
        assert clock.timestamp_ns() == 1_000_000_000
        assert events == []

    def test_advance_time_given_time_in_past_raises_value_error(self):
        # Arrange
        clock = TestClock()
        clock.advance_time(1_000_000_000)

        # Act, Assert
        with pytest.raises(ValueError):
            clock.advance_time(0)

    def test_cancel_timer_when_no_timers_raises_key_error(self):
        # Arrange
        clock = TestClock()

        # Act, Assert
        with pytest.raises(KeyError):
            clock.cancel_timer("BOGUS_ALERT")

    def test_cancel_timers_when_no_timers_does_nothing(self):
        # Arrange
        clock = TestClock()

        # Act
        clock.cancel_timers()

        # Assert
        assert clock.timer_count == 0

    def test_set_time_alert2(self):
        # Arrange
        clock = TestClock()
        name = "TEST_ALERT"
        interval = timedelta(minutes=10)
        alert_time = clock.utc_now() + interval
        handler = []

        # Act
        clock.set_time_alert(name, alert_time, handler.append)

        # Assert
        assert clock.timer_names == ["TEST_ALERT"]
        assert clock.timer_count == 1

    def test_cancel_time_alert_when_timer_removes_timer(self):
        # Arrange
        clock = TestClock()
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=300)
        alert_time = clock.utc_now() + interval
        handler = []

        clock.set_time_alert(name, alert_time, handler.append)

        # Act
        clock.cancel_timer(name)

        # Assert
        assert clock.timer_count == 0

    def test_cancel_timers_when_multiple_times_removes_all_timers(self):
        # Arrange
        clock = TestClock()
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
        assert clock.timer_count == 0

    def test_set_timer2(self):
        # Arrange
        clock = TestClock()
        name = "TEST_TIMER"
        interval = timedelta(minutes=1)
        handler = []

        # Act
        clock.set_timer(
            name=name,
            interval=interval,
            start_time=UNIX_EPOCH + interval,
            stop_time=None,
            callback=handler.append,
        )

        # Assert
        assert clock.timer_names == ["TEST_TIMER"]
        assert clock.timer_count == 1

    def test_advance_time_with_set_time_alert_triggers_event(self):
        # Arrange
        clock = TestClock()
        name = "TEST_ALERT"
        interval = timedelta(minutes=1)
        alert_time = clock.utc_now() + interval
        handler = []

        clock.set_time_alert(name, alert_time, handler.append)

        # Act
        event_handlers = clock.advance_time(2 * 60 * 1_000_000_000)
        event_handlers[0].handle()

        # Assert
        assert len(handler) == 1
        assert len(event_handlers) == 1
        assert event_handlers[0].event.name == "TEST_ALERT"
        assert clock.timer_count == 0

    def test_advance_time_with_multiple_set_time_alerts_triggers_event(self):
        # Arrange
        clock = TestClock()
        interval = timedelta(minutes=1)
        alert_time = clock.utc_now() + interval
        handler = []

        clock.set_time_alert("TEST_ALERT1", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT2", alert_time, handler.append)
        clock.set_time_alert("TEST_ALERT3", alert_time, handler.append)

        # Act
        event_handlers = clock.advance_time(2 * 60 * 1_000_000_000)

        # Assert
        event_names = [eh.event.name for eh in event_handlers]
        assert len(event_handlers) == 3
        assert "TEST_ALERT1" in event_names
        assert "TEST_ALERT2" in event_names
        assert "TEST_ALERT3" in event_names
        assert clock.timer_count == 0

    def test_advance_time_with_set_timer_triggers_events(self):
        # Arrange
        clock = TestClock()
        name = "TEST_TIMER"
        interval = timedelta(minutes=1)
        handler = []

        # Act
        clock.set_timer(
            name=name,
            interval=interval,
            start_time=UNIX_EPOCH + interval,
            stop_time=None,
            callback=handler.append,
        )

        event_handlers = clock.advance_time(5 * 60 * 1_000_000_000)

        # Assert
        assert len(event_handlers) == 4
        assert event_handlers[0].event.name == "TEST_TIMER"
        assert clock.timer_names == ["TEST_TIMER"]
        assert clock.timer_count == 1

    def test_advance_time_with_multiple_set_timers_triggers_events(self):
        # Arrange
        clock = TestClock()
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
            callback=handler1.append,
        )

        clock.set_timer(
            name=name2,
            interval=interval2,
            start_time=UNIX_EPOCH,
            stop_time=None,
            callback=handler2.append,
        )

        event_handlers = clock.advance_time(5 * 60 * 1_000_000_000)

        # Assert
        assert len(event_handlers) == 15
        assert clock.timer_names == ["TEST_TIMER1", "TEST_TIMER2"]
        assert clock.timer_count == 2


class TestLiveClock:
    def setup(self):
        # Fixture Setup
        self.handler = []
        self.clock = LiveClock()
        self.clock.register_default_handler(self.handler.append)

    def teardown(self):
        self.clock.cancel_timers()

    def test_instantiated_clock(self):
        # Arrange, Act, Assert
        assert self.clock.timer_count == 0

    def test_timestamp_is_monotonic(self):
        # Arrange, Act
        result1 = self.clock.timestamp()
        result2 = self.clock.timestamp()
        result3 = self.clock.timestamp()
        result4 = self.clock.timestamp()
        result5 = self.clock.timestamp()

        # Assert
        assert isinstance(result1, float)
        assert result1 > 0.0
        assert result5 >= result4
        assert result4 >= result3
        assert result3 >= result2
        assert result2 >= result1

    def test_timestamp_ms_is_monotonic(self):
        # Arrange, Act
        result1 = self.clock.timestamp_ms()
        result2 = self.clock.timestamp_ms()
        result3 = self.clock.timestamp_ms()
        result4 = self.clock.timestamp_ms()
        result5 = self.clock.timestamp_ms()

        # Assert
        assert isinstance(result1, int)
        assert result1 > 0
        assert result5 >= result4
        assert result4 >= result3
        assert result3 >= result2
        assert result2 >= result1

    def test_timestamp_us_is_monotonic(self):
        # Arrange, Act
        result1 = self.clock.timestamp_us()
        result2 = self.clock.timestamp_us()
        result3 = self.clock.timestamp_us()
        result4 = self.clock.timestamp_us()
        result5 = self.clock.timestamp_us()

        # Assert
        assert isinstance(result1, int)
        assert result1 > 0
        assert result5 >= result4
        assert result4 >= result3
        assert result3 >= result2
        assert result2 >= result1

    def test_timestamp_ns_is_monotonic(self):
        # Arrange, Act
        result1 = self.clock.timestamp_ns()
        result2 = self.clock.timestamp_ns()
        result3 = self.clock.timestamp_ns()
        result4 = self.clock.timestamp_ns()
        result5 = self.clock.timestamp_ns()

        # Assert
        assert isinstance(result1, int)
        assert result1 > 0
        assert result5 >= result4
        assert result4 >= result3
        assert result3 >= result2
        assert result2 >= result1

    def test_utc_now(self):
        # Arrange, Act
        result = self.clock.utc_now()

        # Assert
        assert isinstance(result, datetime)
        assert result.tzinfo == pytz.utc

    def test_local_now(self):
        # Arrange, Act
        result = self.clock.local_now(pytz.timezone("Australia/Sydney"))

        # Assert
        assert isinstance(result, datetime)
        assert str(result).endswith("+11:00") or str(result).endswith("+10:00")

    def test_set_time_alert_in_the_past(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(hours=1)
        alert_time = self.clock.utc_now() - interval

        # Act - will fire immediately
        self.clock.set_time_alert(name, alert_time)
        time.sleep(1.0)

        # Assert
        assert len(self.handler) == 1
        assert isinstance(self.handler[0], TimeEvent)

    def test_set_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.utc_now() + interval

        # Act
        self.clock.set_time_alert(name, alert_time)
        time.sleep(1.0)

        # Assert
        assert len(self.handler) == 1
        assert isinstance(self.handler[0], TimeEvent)

    def test_cancel_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=300)
        alert_time = self.clock.utc_now() + interval

        self.clock.set_time_alert(name, alert_time)

        # Act
        self.clock.cancel_timer(name)

        # Assert
        assert self.clock.timer_count == 0
        assert len(self.handler) == 0

    def test_set_multiple_time_alerts(self):
        # Arrange
        alert_time1 = self.clock.utc_now() + timedelta(milliseconds=200)
        alert_time2 = self.clock.utc_now() + timedelta(milliseconds=300)

        # Act
        self.clock.set_time_alert("TEST_ALERT1", alert_time1)
        self.clock.set_time_alert("TEST_ALERT2", alert_time2)
        time.sleep(2.0)

        # Assert
        assert self.clock.timer_count == 0
        assert len(self.handler) >= 2
        assert isinstance(self.handler[0], TimeEvent)
        assert isinstance(self.handler[1], TimeEvent)

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

        time.sleep(2.0)

        # Assert
        assert self.clock.timer_names == [name]
        assert isinstance(self.handler[0], TimeEvent)

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

        time.sleep(2.0)

        # Assert
        assert self.clock.timer_names == [name]
        assert len(self.handler) > 0
        assert isinstance(self.handler[0], TimeEvent)

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

        time.sleep(2.0)

        # Assert
        assert self.clock.timer_count == 0
        assert len(self.handler) > 0
        assert isinstance(self.handler[0], TimeEvent)

    def test_cancel_timer(self):
        # Arrange
        name = "TEST_TIMER"
        interval = timedelta(milliseconds=100)

        self.clock.set_timer(name=name, interval=interval)

        # Act
        time.sleep(0.3)
        self.clock.cancel_timer(name)
        time.sleep(1.0)

        # Assert
        assert self.clock.timer_count == 0
        assert len(self.handler) <= 6

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

        time.sleep(2.0)

        # Assert
        assert len(self.handler) > 0
        assert isinstance(self.handler[0], TimeEvent)

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
        time.sleep(1.0)

        # Assert
        assert len(self.handler) <= 6

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

        time.sleep(1.0)

        # Assert
        assert len(self.handler) >= 2
