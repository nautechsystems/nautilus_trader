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

from datetime import timedelta

import pandas as pd
import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.events import TimeEvent


_LIVE_CLOCK = LiveClock()
_TEST_CLOCK = TestClock()


def test_live_clock_utc_now(benchmark) -> None:
    benchmark(_LIVE_CLOCK.utc_now)


def test_live_clock_unix_timestamp(benchmark) -> None:
    benchmark(_LIVE_CLOCK.timestamp)


def test_live_clock_timestamp_ns(benchmark) -> None:
    benchmark(_LIVE_CLOCK.timestamp_ns)


def test_live_clock_timestamp_us(benchmark) -> None:
    benchmark(_LIVE_CLOCK.timestamp_us)


def test_live_clock_timestamp_ms(benchmark) -> None:
    benchmark(_LIVE_CLOCK.timestamp_ms)


@pytest.mark.skip()
def test_live_clock_cancel(benchmark) -> None:
    def _start_and_cancel():
        _LIVE_CLOCK.set_timer("timer1", pd.Timedelta(microseconds=10), callback=print)
        _LIVE_CLOCK.cancel_timer("timer1")

    benchmark(_start_and_cancel)


def test_advance_time(benchmark) -> None:
    benchmark(_TEST_CLOCK.advance_time, 0)


def test_iteratively_advance_time(benchmark) -> None:
    store: list[TimeEvent] = []
    _TEST_CLOCK.set_timer("test", timedelta(seconds=1), callback=store.append)

    def _iteratively_advance_time():
        test_time = 0
        for _ in range(100_000):
            test_time += 1
        _TEST_CLOCK.advance_time(to_time_ns=test_time)

    benchmark(_iteratively_advance_time)
