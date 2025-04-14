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

from datetime import datetime
from datetime import timedelta

import pandas as pd
import pytest
import pytz

from nautilus_trader.core.datetime import as_utc_index
from nautilus_trader.core.datetime import as_utc_timestamp
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import format_iso8601
from nautilus_trader.core.datetime import format_optional_iso8601
from nautilus_trader.core.datetime import is_datetime_utc
from nautilus_trader.core.datetime import is_tz_aware
from nautilus_trader.core.datetime import is_tz_naive
from nautilus_trader.core.datetime import maybe_dt_to_unix_nanos
from nautilus_trader.core.datetime import maybe_unix_nanos_to_dt
from nautilus_trader.core.datetime import micros_to_nanos
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import nanos_to_micros
from nautilus_trader.core.datetime import nanos_to_millis
from nautilus_trader.core.datetime import nanos_to_secs
from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH


class TestDatetimeFunctions:
    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, 0],
            [1, 1_000_000_000],
            [1.1, 1_100_000_000],
            [42, 42_000_000_000],
            [0.0001235, 123500],
            [0.00000001, 10],
            [0.000000001, 1],
            [9.999999999, 9_999_999_999],
        ],
    )
    def test_secs_to_nanos(self, value, expected):
        # Arrange, Act
        result = secs_to_nanos(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, 0],
            [1, 1_000],
            [1.1, 1_100],
            [42, 42_000],
            [0.01234, 12],
            [0.001, 1],
        ],
    )
    def test_secs_to_millis(self, value, expected):
        # Arrange, Act
        result = secs_to_millis(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, 0],
            [1, 1_000_000],
            [1.1, 1_100_000],
            [42, 42_000_000],
            [0.0001234, 123],
            [0.00001, 10],
            [0.000001, 1],
            [9.999999, 9_999_999],
        ],
    )
    def test_millis_to_nanos(self, value, expected):
        # Arrange, Act
        result = millis_to_nanos(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, 0],
            [1, 1_000],
            [1.1, 1_100],
            [42, 42_000],
            [0.1234, 123],
            [0.01, 10],
            [0.001, 1],
            [9.999, 9_999],
        ],
    )
    def test_micros_to_nanos(self, value, expected):
        # Arrange, Act
        result = micros_to_nanos(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, 0],
            [1, 1e-09],
            [1_000_000_000, 1],
            [42_897_123_111, 42.897123111],
        ],
    )
    def test_nanos_to_secs(self, value, expected):
        # Arrange, Act
        result = nanos_to_secs(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, 0],
            [1_000_000, 1],
            [1_000_000_000, 1000],
            [42_897_123_111, 42897],
        ],
    )
    def test_nanos_to_millis(self, value, expected):
        # Arrange, Act
        result = nanos_to_millis(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, 0],
            [1_000, 1],
            [1_000_000_000, 1_000_000],
            [42_897_123, 42897],
        ],
    )
    def test_nanos_to_micros(self, value, expected):
        # Arrange, Act
        result = nanos_to_micros(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, UNIX_EPOCH],
            [1_000, pd.Timestamp("1970-01-01 00:00:00.000001+0000", tz="UTC")],
            [1_000_000_000, pd.Timestamp("1970-01-01 00:00:01+0000", tz="UTC")],
        ],
    )
    def test_unix_nanos_to_dt(self, value, expected):
        # Arrange, Act
        result = unix_nanos_to_dt(value)

        # Assert
        assert result == expected
        assert result.tzinfo == pytz.utc

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [None, None],
            [0, UNIX_EPOCH],
            [1_000, pd.Timestamp("1970-01-01 00:00:00.000001+0000", tz="UTC")],
            [1_000_000_000, pd.Timestamp("1970-01-01 00:00:01+0000", tz="UTC")],
        ],
    )
    def test_maybe_unix_nanos_to_dt(self, value, expected):
        # Arrange, Act
        result = maybe_unix_nanos_to_dt(value)

        # Assert
        assert result == expected
        assert result is None or result.tzinfo == pytz.utc

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [UNIX_EPOCH, 0],
            [UNIX_EPOCH + timedelta(milliseconds=100), 100_000_000],
            [UNIX_EPOCH + timedelta(milliseconds=1), 1_000_000],
            [UNIX_EPOCH + timedelta(microseconds=3), 3_000],
            [UNIX_EPOCH + timedelta(hours=12), 43_200_000_000_000],
            [datetime(2021, 5, 7, 13, 41, 7, 930000, tzinfo=pytz.utc), 1620394867930000000],
        ],
    )
    def test_dt_to_unix_nanos(self, value, expected):
        # Arrange, Act
        result = dt_to_unix_nanos(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [None, None],
            [UNIX_EPOCH, 0],
            [UNIX_EPOCH + timedelta(milliseconds=100), 100_000_000],
            [UNIX_EPOCH + timedelta(milliseconds=1), 1_000_000],
            [UNIX_EPOCH + timedelta(microseconds=3), 3_000],
            [UNIX_EPOCH + timedelta(hours=12), 43_200_000_000_000],
            [datetime(2021, 5, 7, 13, 41, 7, 930000, tzinfo=pytz.utc), 1620394867930000000],
        ],
    )
    def test_maybe_dt_to_unix_nanos(self, value, expected):
        # Arrange, Act
        result = maybe_dt_to_unix_nanos(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [datetime(1970, 1, 1, 0, 0, tzinfo=pytz.utc).isoformat(), 0],
            [
                datetime(2013, 1, 1, 1, 0, tzinfo=pytz.utc).isoformat(),
                1357002000000000000,
            ],
            [
                datetime(2020, 1, 2, 3, 2, microsecond=333, tzinfo=pytz.utc).isoformat(),
                1577934120003330000,
            ],
        ],
    )
    def test_iso8601_to_unix_nanos_given_iso8601_datetime_string(self, value, expected):
        # Arrange, Act
        result = dt_to_unix_nanos(value)

        # Assert
        assert result == pytest.approx(expected, 100)  # 100 nanoseconds

    def test_is_datetime_utc_given_tz_naive_datetime_returns_false(self):
        # Arrange
        dt = datetime(2013, 1, 1, 1, 0)

        # Act, Assert
        assert is_datetime_utc(dt) is False

    def test_is_datetime_utc_given_utc_datetime_returns_true(self):
        # Arrange
        dt = datetime(2013, 1, 1, 1, 0, tzinfo=pytz.utc)

        # Act, Assert
        assert is_datetime_utc(dt) is True

    def test_is_tz_awareness_given_unrecognized_type_raises_exception(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            is_tz_aware("hello")

    def test_is_tz_awareness_with_various_aware_objects_returns_true(self):
        # Arrange
        time_object1 = UNIX_EPOCH
        time_object2 = pd.Timestamp(UNIX_EPOCH)

        time_object3 = pd.DataFrame(
            {"timestamp": ["2019-05-21T12:00:00+00:00", "2019-05-21T12:15:00+00:00"]},
        )
        time_object3.set_index("timestamp")
        time_object3.index = pd.to_datetime(time_object3.index)

        # Act, Assert
        assert is_tz_aware(time_object1) is True
        assert is_tz_aware(time_object2) is True
        assert is_tz_aware(time_object3) is True
        assert is_tz_naive(time_object1) is False
        assert is_tz_naive(time_object2) is False
        assert is_tz_naive(time_object3) is False

    def test_is_tz_awareness_with_various_objects_returns_false(self):
        # Arrange
        time_object1 = datetime(1970, 1, 1, 0, 0, 0, 0)
        time_object2 = pd.Timestamp(datetime(1970, 1, 1, 0, 0, 0, 0))

        # Act, Assert
        assert is_tz_aware(time_object1) is False
        assert is_tz_aware(time_object2) is False
        assert is_tz_naive(time_object1) is True
        assert is_tz_naive(time_object2) is True

    def test_format_iso8601(self):
        # Arrange
        dt1 = UNIX_EPOCH
        dt2 = UNIX_EPOCH + timedelta(microseconds=1)
        dt3 = UNIX_EPOCH + timedelta(milliseconds=1)
        dt4 = UNIX_EPOCH + timedelta(seconds=1)
        dt5 = UNIX_EPOCH + timedelta(hours=1, minutes=1, seconds=2, milliseconds=3)

        # Act
        result1 = format_iso8601(dt1)
        result2 = format_iso8601(dt2)
        result3 = format_iso8601(dt3)
        result4 = format_iso8601(dt4)
        result5 = format_iso8601(dt5)

        # Assert
        assert str(pd.to_datetime(dt1, utc=True)) == "1970-01-01 00:00:00+00:00"
        assert result1 == "1970-01-01T00:00:00.000000000Z"
        assert result2 == "1970-01-01T00:00:00.000001000Z"
        assert result3 == "1970-01-01T00:00:00.001000000Z"
        assert result4 == "1970-01-01T00:00:01.000000000Z"
        assert result5 == "1970-01-01T01:01:02.003000000Z"

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [None, "None"],
            [pd.to_datetime(0), "1970-01-01T00:00:00.000000000Z"],
        ],
    )
    def test_format_optional_iso8601(self, value: pd.Timestamp | None, expected: str):
        # Arrange, Act
        result = format_optional_iso8601(value)

        # Assert
        assert result == expected

    def test_datetime_and_pd_timestamp_equality(self):
        # Arrange
        timestamp1 = datetime(1970, 1, 1, 0, 0, 0, 0)
        timestamp2 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0)
        min1 = timedelta(minutes=1)

        # Act
        timestamp3 = timestamp1 + min1
        timestamp4 = timestamp2 + min1
        timestamp5 = UNIX_EPOCH
        timestamp6 = timestamp2.tz_localize("UTC")

        # Assert
        assert timestamp1 == timestamp2
        assert timestamp3 == timestamp4
        assert timestamp1.tzinfo == timestamp2.tzinfo
        assert timestamp2.tz is None
        assert timestamp5 == timestamp6

    def test_as_utc_timestamp_given_tz_naive_datetime(self):
        # Arrange
        timestamp = datetime(2013, 2, 1, 0, 0, 0, 0)

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        assert result == pd.Timestamp("2013-02-01 00:00:00+00:00")
        assert result.tzinfo == pytz.utc

    def test_as_utc_timestamp_given_tz_naive_pandas_timestamp(self):
        # Arrange
        timestamp = pd.Timestamp(2013, 2, 1, 0, 0, 0, 0)

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        assert result == pd.Timestamp("2013-02-01 00:00:00+00:00")
        assert result.tzinfo == pytz.utc

    def test_as_utc_timestamp_given_tz_aware_datetime(self):
        # Arrange
        timestamp = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        assert result == pd.Timestamp("2013-02-01 00:00:00+00:00")
        assert result.tzinfo == pytz.utc

    def test_as_utc_timestamp_given_tz_aware_pandas(self):
        # Arrange
        timestamp = pd.Timestamp(2013, 2, 1, 0, 0, 0, 0).tz_localize("UTC")

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        assert result == pd.Timestamp("2013-02-01 00:00:00+00:00")
        assert result.tzinfo == pytz.utc

    def test_as_utc_timestamp_equality(self):
        # Arrange
        timestamp1 = datetime(1970, 1, 1, 0, 0, 0, 0)
        timestamp2 = UNIX_EPOCH
        timestamp3 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0)
        timestamp4 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0).tz_localize("UTC")

        # Act
        timestamp1_converted = as_utc_timestamp(timestamp1)
        timestamp2_converted = as_utc_timestamp(timestamp2)
        timestamp3_converted = as_utc_timestamp(timestamp3)
        timestamp4_converted = as_utc_timestamp(timestamp4)

        # Assert
        assert timestamp1_converted == timestamp2_converted
        assert timestamp2_converted == timestamp3_converted
        assert timestamp3_converted == timestamp4_converted

    def test_as_utc_index_given_empty_dataframe_returns_empty_dataframe(self):
        # Arrange
        data = pd.DataFrame()

        # Act
        result = as_utc_index(data)

        # Assert
        assert result.empty

    def test_with_utc_index_given_tz_unaware_dataframe(self):
        # Arrange
        data = pd.DataFrame(
            {"timestamp": ["2019-05-21T12:00:00+00:00", "2019-05-21T12:15:00+00:00"]},
        )
        data.set_index("timestamp")
        data.index = pd.to_datetime(data.index)

        # Act
        result = as_utc_index(data)

        # Assert
        assert result.index.tz == pytz.utc

    def test_with_utc_index_given_tz_aware_dataframe(self):
        # Arrange
        data = pd.DataFrame(
            {"timestamp": ["2019-05-21T12:00:00+00:00", "2019-05-21T12:15:00+00:00"]},
        )
        data.set_index("timestamp")
        data.index = pd.to_datetime(data.index, utc=True)

        # Act
        result = as_utc_index(data)

        # Assert
        assert result.index.tz == pytz.utc

    def test_with_utc_index_given_tz_aware_different_timezone_dataframe(self):
        # Arrange
        data1 = pd.DataFrame({"timestamp": ["2019-05-21 12:00:00", "2019-05-21 12:15:00"]})
        data1.set_index("timestamp")
        data1.index = pd.to_datetime(data1.index)

        data2 = pd.DataFrame(
            {
                "timestamp": [
                    datetime(1970, 1, 1, 0, 0, 0, 0),
                    datetime(1970, 1, 1, 0, 0, 0, 0),
                ],
            },
        )
        data2.set_index("timestamp")
        data2.index = pd.to_datetime(data2.index, utc=True)

        # Act
        result1 = as_utc_index(data1)
        result2 = as_utc_index(data2)

        # Assert
        assert result1.index[0] == result2.index[0]
        assert result1.index.tz == result2.index.tz
