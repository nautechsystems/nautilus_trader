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


def test_clock_cancel_default_handler():
    clock = Clock.new_test()
    clock.register_default_handler(lambda _: None)

    # Default handler satisfies the "any callback" precondition for new timers
    clock.set_timer_ns("timer_ok", 1_000, start_time_ns=0, stop_time_ns=5_000)
    clock.cancel_timers()

    clock.cancel_default_handler()

    # After cancel, no fallback handler exists; new timers must supply their own
    with pytest.raises(ValueError, match="No callbacks provided"):
        clock.set_timer_ns("timer_after_cancel", 1_000)

    with pytest.raises(ValueError, match="No callbacks provided"):
        clock.set_time_alert_ns("alert_after_cancel", 10)


def test_clock_cancel_callbacks_clears_named_handlers():
    clock = Clock.new_test()

    clock.set_time_alert_ns("alert_ns", 10, callback=lambda _: None)
    clock.set_timer_ns(
        "timer_ns",
        1_000,
        start_time_ns=0,
        stop_time_ns=5_000,
        callback=lambda _: None,
    )
    clock.cancel_timers()
    clock.cancel_callbacks()

    # Reschedule the SAME names that previously had callbacks. Without an explicit
    # callback, the precondition only passes if the registry still has the named
    # callback; a no-op `cancel_callbacks()` would let this succeed.
    with pytest.raises(ValueError, match="No callbacks provided"):
        clock.set_time_alert_ns("alert_ns", 20)
    with pytest.raises(ValueError, match="No callbacks provided"):
        clock.set_timer_ns("timer_ns", 1_000)


def test_clock_cancel_default_handler_releases_handler_object():
    # Regression for the BacktestEngine actor leak: the clock must release its
    # `Py<PyAny>` to break the actor <- bound method <- clock cycle. Keep clocks
    # alive through the assertion so a no-op cancel cannot pass via clock drop.
    import gc

    class Holder:
        def handler(self, _event):
            pass

    baseline = sum(1 for o in gc.get_objects() if isinstance(o, Holder))
    clocks = []

    for _ in range(10):
        holder = Holder()
        clock = Clock.new_test()
        clock.register_default_handler(holder.handler)
        clock.cancel_default_handler()
        clocks.append(clock)
        del holder
        gc.collect()

    residual = sum(1 for o in gc.get_objects() if isinstance(o, Holder)) - baseline
    assert residual <= 1, f"holders accumulated: residual={residual}"

    del clocks
    gc.collect()


def test_clock_cancel_default_handler_is_idempotent():
    clock = Clock.new_test()

    # No handler registered: cancel must be a no-op rather than panic
    clock.cancel_default_handler()
    clock.cancel_default_handler()

    clock.register_default_handler(lambda _: None)
    clock.cancel_default_handler()
    clock.cancel_default_handler()  # Second call on cleared state

    with pytest.raises(ValueError, match="No callbacks provided"):
        clock.set_timer_ns("timer", 1_000)


def test_clock_cancel_callbacks_is_idempotent():
    clock = Clock.new_test()

    clock.cancel_callbacks()  # Empty registry, no-op
    clock.cancel_callbacks()


def test_clock_cancel_callbacks_does_not_clear_default_handler():
    clock = Clock.new_test()
    clock.register_default_handler(lambda _: None)
    clock.set_time_alert_ns("alert", 10, callback=lambda _: None)

    clock.cancel_callbacks()

    # Default handler still satisfies the precondition for new timers
    clock.set_timer_ns("timer_after_clear", 1_000, start_time_ns=0, stop_time_ns=5_000)


def test_clock_cancel_default_handler_does_not_clear_named_callbacks():
    clock = Clock.new_test()
    clock.register_default_handler(lambda _: None)
    clock.set_time_alert_ns("alert", 10, callback=lambda _: None)

    clock.cancel_default_handler()

    # Named callback for `alert` is still registered, so re-scheduling it without
    # a callback succeeds (the clock falls back to the existing named callback)
    clock.cancel_timer("alert")
    clock.set_time_alert_ns("alert", 20, allow_past=False)
