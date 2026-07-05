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

import sys
import time
from datetime import UTC
from datetime import datetime

import pytest

from nautilus_trader.core import MILLISECONDS_IN_SECOND
from nautilus_trader.core import NANOSECONDS_IN_MICROSECOND
from nautilus_trader.core import NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core import NANOSECONDS_IN_SECOND
from nautilus_trader.core import dt_to_unix_nanos
from nautilus_trader.core import is_within_last_24_hours
from nautilus_trader.core import last_weekday_nanos
from nautilus_trader.core import micros_to_nanos
from nautilus_trader.core import millis_to_nanos
from nautilus_trader.core import nanos_to_micros
from nautilus_trader.core import nanos_to_millis
from nautilus_trader.core import nanos_to_secs
from nautilus_trader.core import secs_to_millis
from nautilus_trader.core import secs_to_nanos
from nautilus_trader.core import unix_nanos_to_dt
from nautilus_trader.core import unix_nanos_to_iso8601
from nautilus_trader.core.datetime import dt_to_unix_nanos as dt_to_unix_nanos_from_datetime_module
from nautilus_trader.core.datetime import unix_nanos_to_dt as unix_nanos_to_dt_from_datetime_module


def test_time_constants_match_conversion_scale():
    assert MILLISECONDS_IN_SECOND == 1_000
    assert NANOSECONDS_IN_SECOND == 1_000_000_000
    assert NANOSECONDS_IN_MILLISECOND == 1_000_000
    assert NANOSECONDS_IN_MICROSECOND == 1_000
    assert secs_to_millis(1) == MILLISECONDS_IN_SECOND
    assert secs_to_nanos(1) == NANOSECONDS_IN_SECOND
    assert millis_to_nanos(1) == NANOSECONDS_IN_MILLISECOND
    assert micros_to_nanos(1) == NANOSECONDS_IN_MICROSECOND


def test_datetime_helpers_are_available_from_core_and_datetime_module():
    assert dt_to_unix_nanos_from_datetime_module is dt_to_unix_nanos
    assert unix_nanos_to_dt_from_datetime_module is unix_nanos_to_dt


def test_dt_to_unix_nanos_accepts_datetime():
    value = datetime(2023, 11, 14, 22, 13, 20, tzinfo=UTC)

    assert dt_to_unix_nanos(value) == 1_700_000_000_000_000_000


def test_unix_nanos_to_dt_roundtrip():
    nanos = 1_700_000_000_123_456_000

    assert dt_to_unix_nanos(unix_nanos_to_dt(nanos)) == nanos


def test_dt_to_unix_nanos_does_not_accept_arbitrary_value_attribute():
    pytest.importorskip("pandas")

    class ValueOnly:
        value = 1_700_000_000_000_000_000

    with pytest.raises(TypeError, match="Cannot convert input"):
        dt_to_unix_nanos(ValueOnly())


def test_dt_to_unix_nanos_without_pandas_accepts_datetime_str_and_int(monkeypatch):
    monkeypatch.setitem(sys.modules, "pandas", None)

    value = datetime(2023, 11, 14, 22, 13, 20, tzinfo=UTC)

    assert dt_to_unix_nanos(value) == 1_700_000_000_000_000_000
    assert dt_to_unix_nanos("2023-11-14T22:13:20+00:00") == 1_700_000_000_000_000_000
    assert dt_to_unix_nanos(1_700_000_000_000_000_000) == 1_700_000_000_000_000_000


def test_dt_to_unix_nanos_without_pandas_rejects_arbitrary_value_attribute(monkeypatch):
    monkeypatch.setitem(sys.modules, "pandas", None)

    class ValueOnly:
        value = 1_700_000_000_000_000_000

    with pytest.raises(TypeError, match="value must be datetime-like"):
        dt_to_unix_nanos(ValueOnly())


def test_dt_to_unix_nanos_without_pandas_rejects_nanosecond_precision_string(monkeypatch):
    monkeypatch.setitem(sys.modules, "pandas", None)

    with pytest.raises(ValueError, match="pandas is required"):
        dt_to_unix_nanos("2023-11-14T22:13:20.123456789+00:00")


def test_unix_nanos_to_dt_without_pandas_accepts_microsecond_aligned_timestamp(monkeypatch):
    monkeypatch.setitem(sys.modules, "pandas", None)

    value = unix_nanos_to_dt(1_700_000_000_123_456_000)

    assert value == datetime(2023, 11, 14, 22, 13, 20, 123456, tzinfo=UTC)


def test_unix_nanos_to_dt_without_pandas_rejects_nanosecond_precision_loss(monkeypatch):
    monkeypatch.setitem(sys.modules, "pandas", None)

    with pytest.raises(ValueError, match="pandas is required"):
        unix_nanos_to_dt(1_700_000_000_123_456_789)


@pytest.mark.parametrize(
    ("secs", "expected"),
    [
        (0, 0),
        (1, 1_000_000_000),
        (0.5, 500_000_000),
        (60, 60_000_000_000),
    ],
)
def test_secs_to_nanos(secs, expected):
    assert secs_to_nanos(secs) == expected


@pytest.mark.parametrize(
    ("secs", "expected"),
    [
        (0, 0),
        (1, 1_000),
        (0.5, 500),
        (60, 60_000),
    ],
)
def test_secs_to_millis(secs, expected):
    assert secs_to_millis(secs) == expected


@pytest.mark.parametrize(
    ("millis", "expected"),
    [
        (0, 0),
        (1, 1_000_000),
        (1_000, 1_000_000_000),
    ],
)
def test_millis_to_nanos(millis, expected):
    assert millis_to_nanos(millis) == expected


@pytest.mark.parametrize(
    ("micros", "expected"),
    [
        (0, 0),
        (1, 1_000),
        (1_000_000, 1_000_000_000),
    ],
)
def test_micros_to_nanos(micros, expected):
    assert micros_to_nanos(micros) == expected


@pytest.mark.parametrize(
    ("nanos", "expected"),
    [
        (0, 0.0),
        (1_000_000_000, 1.0),
        (500_000_000, 0.5),
    ],
)
def test_nanos_to_secs(nanos, expected):
    assert nanos_to_secs(nanos) == expected


@pytest.mark.parametrize(
    ("nanos", "expected"),
    [
        (0, 0),
        (1_000_000, 1),
        (1_000_000_000, 1_000),
    ],
)
def test_nanos_to_millis(nanos, expected):
    assert nanos_to_millis(nanos) == expected


@pytest.mark.parametrize(
    ("nanos", "expected"),
    [
        (0, 0),
        (1_000, 1),
        (1_000_000_000, 1_000_000),
    ],
)
def test_nanos_to_micros(nanos, expected):
    assert nanos_to_micros(nanos) == expected


def test_secs_to_nanos_roundtrip():
    assert nanos_to_secs(secs_to_nanos(3.5)) == 3.5


def test_millis_to_nanos_roundtrip():
    assert nanos_to_millis(millis_to_nanos(42)) == 42


def test_micros_to_nanos_roundtrip():
    assert nanos_to_micros(micros_to_nanos(999)) == 999


def test_unix_nanos_to_iso8601_epoch():
    assert unix_nanos_to_iso8601(0) == "1970-01-01T00:00:00.000000000Z"


def test_unix_nanos_to_iso8601_one_second():
    assert unix_nanos_to_iso8601(1_000_000_000) == "1970-01-01T00:00:01.000000000Z"


def test_unix_nanos_to_iso8601_known_timestamp():
    ts = 1_546_387_200_000_000_000
    assert unix_nanos_to_iso8601(ts) == "2019-01-02T00:00:00.000000000Z"


def test_unix_nanos_to_iso8601_nanos_precision_true():
    result = unix_nanos_to_iso8601(1_234_567_890, nanos_precision=True)
    assert "." in result
    fractional = result.split(".")[1].rstrip("Z")
    assert len(fractional) == 9


def test_unix_nanos_to_iso8601_nanos_precision_false():
    result = unix_nanos_to_iso8601(1_000_000_000, nanos_precision=False)
    assert result == "1970-01-01T00:00:01.000Z"


def test_last_weekday_nanos_returns_int():
    result = last_weekday_nanos(2024, 1, 15)
    assert isinstance(result, int)
    assert result > 0


def test_last_weekday_nanos_weekday():
    # 2024-01-15 is a Monday, returns that day's midnight
    result = last_weekday_nanos(2024, 1, 15)
    iso = unix_nanos_to_iso8601(result)
    assert iso.startswith("2024-01-15T")


@pytest.mark.parametrize(
    ("year", "month", "day"),
    [
        (2024, 1, 13),  # Saturday
        (2024, 1, 14),  # Sunday
    ],
)
def test_last_weekday_nanos_weekend_returns_friday(year, month, day):
    result = last_weekday_nanos(year, month, day)
    iso = unix_nanos_to_iso8601(result)
    assert iso.startswith("2024-01-12T")


def test_is_within_last_24_hours_recent():
    now_ns = int(time.time() * 1_000_000_000)
    assert is_within_last_24_hours(now_ns) is True


def test_is_within_last_24_hours_epoch():
    assert is_within_last_24_hours(0) is False


def test_is_within_last_24_hours_one_hour_ago():
    one_hour_ago_ns = int((time.time() - 3600) * 1_000_000_000)
    assert is_within_last_24_hours(one_hour_ago_ns) is True


def test_is_within_last_24_hours_two_days_ago():
    two_days_ago_ns = int((time.time() - 172_800) * 1_000_000_000)
    assert is_within_last_24_hours(two_days_ago_ns) is False
