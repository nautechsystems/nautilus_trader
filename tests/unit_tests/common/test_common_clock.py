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

import asyncio
from datetime import datetime
from datetime import timedelta
import time

import pytest
import pytz

from nautilus_trader.common.clock import Clock
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.timer import TimeEvent
from tests.test_kit.stubs import UNIX_EPOCH


class TestClockBase:
    def test_unix_time_when_not_implemented_raises_exception(self):
        # Arrange
        clock = Clock()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            clock.unix_time()

    def test_unix_time_ms_when_not_implemented_raises_exception(self):
        # Arrange
        clock = Clock()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            clock.unix_time_ms()

    def test_unix_time_us_when_not_implemented_raises_exception(self):
        # Arrange
        clock = Clock()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            clock.unix_time_us()

    def test_unix_time_ns_when_not_implemented_raises_exception(self):
        # Arrange
        clock = Clock()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            clock.unix_time_ns()

    def test_set_timer_when_not_implemented_raises_exception(self):
        # Arrange
        clock = Clock()
        name = "TEST_TIMER"
        interval = timedelta(minutes=1)
        handler = []

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            clock.set_timer(
                name=name,
                interval=interval,
                start_time=UNIX_EPOCH + interval,
                stop_time=None,
                handler=handler.append,
            )


class TestTestClock:
    def test_instantiate_has_expected_time_and_properties(self):
        # Arrange
        init_time = UNIX_EPOCH + timedelta(minutes=1)
        clock = TestClock(UNIX_EPOCH + timedelta(minutes=1))

        # Act
        # Assert
        assert clock.utc_now() == init_time
        assert clock.is_test_clock

    def test_utc_now_returns_expected_datetime(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        result = clock.utc_now()

        # Assert
        assert result == UNIX_EPOCH

    def test_unix_time_returns_expected_double(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH + timedelta(minutes=1))

        # Act
        result = clock.unix_time()

        # Assert
        assert result == 60

    def test_unix_time_ms_returns_expected_int64(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH + timedelta(minutes=1))

        # Act
        result = clock.unix_time_ms()

        # Assert
        assert result == 60_000

    def test_unix_time_us_returns_expected_int64(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH + timedelta(minutes=1))

        # Act
        result = clock.unix_time_us()

        # Assert
        assert result == 60_000_000

    def test_unix_time_ns_returns_expected_int64(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH + timedelta(minutes=1))

        # Act
        result = clock.unix_time_ns()

        # Assert
        assert result == 60_000_000_000

    def test_set_time_changes_time(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        clock.set_time(UNIX_EPOCH + timedelta(minutes=1))

        # Assert
        assert clock.utc_now() == UNIX_EPOCH + timedelta(minutes=1)

    def test_advance_time_changes_time_produces_empty_list(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        events = clock.advance_time(UNIX_EPOCH + timedelta(minutes=1))

        # Assert
        assert clock.utc_now() == UNIX_EPOCH + timedelta(minutes=1)
        assert events == []

    def test_advance_time_given_time_in_past_raises_value_error(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        # Assert
        with pytest.raises(ValueError):
            clock.advance_time(UNIX_EPOCH - timedelta(minutes=1))

    def test_local_now(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        result = clock.local_now(pytz.timezone("Australia/Sydney"))

        assert str(result) == "1970-01-01 10:00:00+10:00"

    def test_delta(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        events = clock.delta(UNIX_EPOCH - timedelta(minutes=9))

        assert events == timedelta(minutes=9)

    def test_cancel_timer_when_no_timers_does_nothing(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        clock.cancel_timer("BOGUS_ALERT")

        # Assert
        assert clock.timer_names() == []
        assert clock.timer_count == 0

    def test_cancel_timers_when_no_timers_does_nothing(self):
        # Arrange
        clock = TestClock(UNIX_EPOCH)

        # Act
        clock.cancel_timers()

        # Assert
        assert clock.timer_names() == []
        assert clock.timer_count == 0

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
        assert clock.timer_names() == ["TEST_ALERT"]
        assert clock.timer("TEST_ALERT").name == "TEST_ALERT"
        assert clock.timer_count == 1

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
        assert clock.timer_names() == []
        assert clock.timer_count == 0

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
        assert clock.timer_names() == []
        assert clock.timer_count == 0

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
            handler=handler.append,
        )

        # Assert
        assert clock.timer_names() == ["TEST_TIMER"]
        assert clock.timer("TEST_TIMER").name == "TEST_TIMER"
        assert clock.timer_count == 1

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
        assert len(event_handlers) == 1
        assert event_handlers[0].event.name == "TEST_ALERT"
        assert clock.timer_names() == []
        assert clock.timer_count == 0

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
        assert len(event_handlers) == 3
        assert event_handlers[0].event.name == "TEST_ALERT1"
        assert event_handlers[1].event.name == "TEST_ALERT2"
        assert event_handlers[2].event.name == "TEST_ALERT3"
        assert clock.timer_names() == []
        assert clock.timer_count == 0

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
            handler=handler.append,
        )

        event_handlers = clock.advance_time(UNIX_EPOCH + timedelta(minutes=5))

        # Assert
        assert len(event_handlers) == 4
        assert event_handlers[0].event.name == "TEST_TIMER"
        assert clock.timer_names() == ["TEST_TIMER"]
        assert clock.timer("TEST_TIMER").name == "TEST_TIMER"
        assert clock.timer_count == 1

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
            handler=handler1.append,
        )

        clock.set_timer(
            name=name2,
            interval=interval2,
            start_time=UNIX_EPOCH,
            stop_time=None,
            handler=handler2.append,
        )

        event_handlers = clock.advance_time(UNIX_EPOCH + timedelta(minutes=5))

        # Assert
        assert len(event_handlers) == 15
        assert event_handlers[0].event.name == "TEST_TIMER2"
        assert event_handlers[1].event.name == "TEST_TIMER1"
        assert clock.timer_names() == ["TEST_TIMER1", "TEST_TIMER2"]
        assert clock.timer("TEST_TIMER1").name == "TEST_TIMER1"
        assert clock.timer("TEST_TIMER2").name == "TEST_TIMER2"
        assert clock.timer_count == 2


class TestLiveClockWithThreadTimer:
    def setup(self):
        # Fixture Setup
        self.handler = []
        self.clock = LiveClock()
        self.clock.register_default_handler(self.handler.append)

    def teardown(self):
        self.clock.cancel_timers()

    def test_instantiated_clock(self):
        # Arrange
        # Act
        # Assert
        assert self.clock.is_default_handler_registered
        assert not self.clock.is_test_clock
        assert self.clock.timer_names() == []

    def test_utc_now(self):
        # Arrange
        # Act
        result = self.clock.utc_now()

        # Assert
        assert type(result) == datetime
        assert result.tzinfo == pytz.utc

    def test_local_now(self):
        # Arrange
        # Act
        result = self.clock.local_now(pytz.timezone("Australia/Sydney"))

        # Assert
        assert type(result) == datetime
        assert str(result).endswith("+11:00")

    def test_delta(self):
        # Arrange
        start = self.clock.utc_now()

        # Act
        time.sleep(0.1)
        result = self.clock.delta(start)

        # Assert
        assert result > timedelta(0)
        assert type(result) == timedelta

    def test_set_time_alert(self):
        # Arrange
        name = "TEST_ALERT"
        interval = timedelta(milliseconds=100)
        alert_time = self.clock.utc_now() + interval

        # Act
        self.clock.set_time_alert(name, alert_time)
        time.sleep(0.3)

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
        assert self.clock.timer_names() == []
        assert len(self.handler) == 0

    def test_set_multiple_time_alerts(self):
        # Arrange
        alert_time1 = self.clock.utc_now() + timedelta(milliseconds=200)
        alert_time2 = self.clock.utc_now() + timedelta(milliseconds=300)

        # Act
        self.clock.set_time_alert("TEST_ALERT1", alert_time1)
        self.clock.set_time_alert("TEST_ALERT2", alert_time2)
        time.sleep(0.6)

        # Assert
        assert self.clock.timer_names() == []
        assert len(self.handler) == 2
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

        time.sleep(0.5)

        # Assert
        assert self.clock.timer_names() == [name]
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

        time.sleep(0.5)

        # Assert
        assert self.clock.timer_names() == [name]
        assert len(self.handler) >= 2
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

        time.sleep(0.5)

        # Assert
        assert self.clock.timer_names() == []
        assert len(self.handler) >= 1
        assert isinstance(self.handler[0], TimeEvent)

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
        assert self.clock.timer_names() == []
        assert len(self.handler) <= 4

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
        assert len(self.handler) >= 3
        assert isinstance(self.handler[0], TimeEvent)
        assert isinstance(self.handler[1], TimeEvent)
        assert isinstance(self.handler[2], TimeEvent)

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
        assert len(self.handler) <= 5

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
        assert len(self.handler) >= 8


class TestLiveClockWithLoopTimer:
    def setup(self):
        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        # Fixture Setup
        self.handler = []
        self.clock = LiveClock(loop=self.loop)
        self.clock.register_default_handler(self.handler.append)

    def teardown(self):
        self.clock.cancel_timers()

    def test_unix_time(self):
        # Arrange
        # Act
        result = self.clock.unix_time()

        # Assert
        assert type(result) == float
        assert result > 0

    def test_unix_time_ms(self):
        # Arrange
        # Act
        result = self.clock.unix_time_ms()

        # Assert
        assert type(result) == int
        assert result > 0

    def test_unix_time_us(self):
        # Arrange
        # Act
        result = self.clock.unix_time_us()

        # Assert
        assert type(result) == int
        assert result > 0

    def test_unix_time_ns(self):
        # Arrange
        # Act
        result = self.clock.unix_time_ns()

        # Assert
        assert type(result) == int
        assert result > 0

    def test_set_time_alert(self):
        async def run_test():
            # Arrange
            name = "TEST_ALERT"
            interval = timedelta(milliseconds=100)
            alert_time = self.clock.utc_now() + interval

            # Act
            self.clock.set_time_alert(name, alert_time)
            await asyncio.sleep(0.3)

            # Assert
            assert len(self.handler) >= 1
            assert isinstance(self.handler[0], TimeEvent)

        self.loop.run_until_complete(run_test())

    def test_cancel_time_alert(self):
        async def run_test():
            # Arrange
            name = "TEST_ALERT"
            interval = timedelta(milliseconds=300)
            alert_time = self.clock.utc_now() + interval

            self.clock.set_time_alert(name, alert_time)

            # Act
            self.clock.cancel_timer(name)

            # Assert
            assert self.clock.timer_names() == []
            assert len(self.handler) == 0

        self.loop.run_until_complete(run_test())

    def test_set_multiple_time_alerts(self):
        async def run_test():
            # Arrange
            alert_time1 = self.clock.utc_now() + timedelta(milliseconds=200)
            alert_time2 = self.clock.utc_now() + timedelta(milliseconds=300)

            # Act
            self.clock.set_time_alert("TEST_ALERT1", alert_time1)
            self.clock.set_time_alert("TEST_ALERT2", alert_time2)
            await asyncio.sleep(0.7)

            # Assert
            assert self.clock.timer_names() == []
            assert len(self.handler) >= 2
            assert isinstance(self.handler[0], TimeEvent)
            assert isinstance(self.handler[1], TimeEvent)

        self.loop.run_until_complete(run_test())

    def test_set_timer_with_immediate_start_time(self):
        async def run_test():
            # Arrange
            name = "TEST_TIMER"

            # Act
            self.clock.set_timer(
                name=name,
                interval=timedelta(milliseconds=100),
                start_time=None,
                stop_time=None,
            )

            await asyncio.sleep(0.5)

            # Assert
            assert self.clock.timer_names() == [name]
            assert isinstance(self.handler[0], TimeEvent)

        self.loop.run_until_complete(run_test())

    def test_set_timer(self):
        async def run_test():
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

            await asyncio.sleep(0.5)

            # Assert
            assert self.clock.timer_names() == [name]
            assert len(self.handler) >= 2
            assert isinstance(self.handler[0], TimeEvent)

        self.loop.run_until_complete(run_test())

    def test_set_timer_with_stop_time(self):
        async def run_test():
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

            await asyncio.sleep(0.5)

            # Assert
            assert self.clock.timer_names() == []
            assert len(self.handler) >= 1
            assert isinstance(self.handler[0], TimeEvent)

        self.loop.run_until_complete(run_test())

    def test_cancel_timer(self):
        async def run_test():
            # Arrange
            name = "TEST_TIMER"
            interval = timedelta(milliseconds=100)

            self.clock.set_timer(name=name, interval=interval)

            # Act
            await asyncio.sleep(0.3)
            self.clock.cancel_timer(name)
            await asyncio.sleep(0.3)

            # Assert
            assert self.clock.timer_names() == []
            assert len(self.handler) <= 4

        self.loop.run_until_complete(run_test())

    def test_set_repeating_timer(self):
        async def run_test():
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

            await asyncio.sleep(0.5)

            # Assert
            assert len(self.handler) >= 3
            assert isinstance(self.handler[0], TimeEvent)
            assert isinstance(self.handler[1], TimeEvent)
            assert isinstance(self.handler[2], TimeEvent)

        self.loop.run_until_complete(run_test())

    def test_cancel_repeating_timer(self):
        async def run_test():
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
            await asyncio.sleep(0.3)
            self.clock.cancel_timer(name)
            await asyncio.sleep(0.3)

            # Assert
            assert len(self.handler) <= 5

        self.loop.run_until_complete(run_test())

    def test_set_two_repeating_timers(self):
        async def run_test():
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

            await asyncio.sleep(0.9)

            # Assert
            assert len(self.handler) >= 8

        self.loop.run_until_complete(run_test())
