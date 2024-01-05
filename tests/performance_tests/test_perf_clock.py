# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Any

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.clock import TimeEvent


_LIVE_CLOCK = LiveClock()
_TEST_CLOCK = TestClock()


def test_live_clock_utc_now(benchmark: Any) -> None:
    benchmark.pedantic(
        target=_LIVE_CLOCK.timestamp_ns,
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~1.3μs / 1330ns minimum of 100,000 runs @ 1 iteration each run.


def test_live_clock_unix_timestamp(benchmark: Any) -> None:
    benchmark.pedantic(
        target=_LIVE_CLOCK.timestamp,
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~0.1μs / 101ns minimum of 100,000 runs @ 1 iteration each run.


def test_live_clock_timestamp_ns(benchmark: Any) -> None:
    benchmark.pedantic(
        target=_LIVE_CLOCK.timestamp_ns,
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~0.1μs / 101ns minimum of 100,000 runs @ 1 iteration each run.


def test_advance_time(benchmark: Any) -> None:
    benchmark.pedantic(
        target=_TEST_CLOCK.advance_time,
        args=(0,),
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~0.2μs / 175ns minimum of 100,000 runs @ 1 iteration each run.


def test_iteratively_advance_time(benchmark: Any) -> None:
    store: list[TimeEvent] = []
    _TEST_CLOCK.set_timer("test", timedelta(seconds=1), callback=store.append)

    def _iteratively_advance_time():
        test_time = 0
        for _ in range(100000):
            test_time += 1
        _TEST_CLOCK.advance_time(to_time_ns=test_time)

    benchmark.pedantic(
        target=_iteratively_advance_time,
        iterations=1,
        rounds=1,
    )
    # ~320.1ms                       minimum of 1 runs @ 1 iteration each run. (100000 advances)
    # ~3.7ms / ~3655.1μs / 3655108ns minimum of 1 runs @ 1 iteration each run.
