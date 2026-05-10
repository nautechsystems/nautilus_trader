# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime as dt

import pytest

from nautilus_trader.common import Clock


def test_clock_requires_callback_for_timers():
    clock = Clock.new_test()

    with pytest.raises(ValueError, match="No callbacks provided"):
        clock.set_timer_ns("heartbeat", 1_000)

    with pytest.raises(ValueError, match="No callbacks provided"):
        clock.set_time_alert_ns("alert", 10)


def test_clock_datetime_surface_with_default_handler():
    clock = Clock.new_test()
    received = []

    clock.register_default_handler(received.append)

    base = clock.utc_now()

    clock.set_time_alert(
        "alert_dt",
        base + dt.timedelta(seconds=1),
        allow_past=False,
    )
    clock.set_timer(
        "timer_dt",
        dt.timedelta(seconds=2),
        start_time=base,
        stop_time=base + dt.timedelta(seconds=6),
        allow_past=False,
        fire_immediately=False,
    )

    assert clock.timestamp_ns() == 0
    assert clock.timestamp_us() == 0
    assert clock.timestamp_ms() == 0
    assert clock.timestamp() == 0.0
    assert base == dt.datetime(1970, 1, 1, tzinfo=dt.UTC)
    assert clock.timer_count() == 2
    assert set(clock.timer_names()) == {"alert_dt", "timer_dt"}
    assert clock.next_time_ns("alert_dt") == 1_000_000_000
    assert clock.next_time_ns("timer_dt") == 2_000_000_000
    assert received == []

    clock.cancel_timer("alert_dt")

    assert clock.timer_count() == 1
    assert clock.next_time_ns("alert_dt") is None

    clock.cancel_timers()

    assert clock.timer_names() == []
    assert clock.timer_count() == 0


def test_clock_ns_surface_with_explicit_callbacks():
    clock = Clock.new_test()
    received = []

    clock.set_time_alert_ns(
        "alert_ns",
        10,
        callback=received.append,
        allow_past=False,
    )
    clock.set_timer_ns(
        "timer_ns",
        1_000,
        start_time_ns=0,
        stop_time_ns=5_000,
        callback=received.append,
        allow_past=False,
        fire_immediately=False,
    )

    assert clock.timer_count() == 2
    assert set(clock.timer_names()) == {"alert_ns", "timer_ns"}
    assert clock.next_time_ns("alert_ns") == 10
    assert clock.next_time_ns("timer_ns") == 1_000
    assert received == []

    clock.cancel_timers()

    assert clock.timer_names() == []
    assert clock.timer_count() == 0
