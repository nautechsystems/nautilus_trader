# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest
import pandas as pd
import pytz
from datetime import datetime, timezone, timedelta

from nautilus_trader.core.datetime import is_tz_aware, is_tz_naive, format_iso8601
from nautilus_trader.core.datetime import as_timestamp_utc, with_utc_index

from tests.test_kit.stubs import UNIX_EPOCH


class TestFunctionsTests(unittest.TestCase):

    def test_is_tz_awareness_with_various_aware_objects_returns_true(self):
        # Arrange
        time_object1 = datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC)
        time_object2 = pd.Timestamp(datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC))

        time_object3 = pd.DataFrame({'timestamp': ['2019-05-21T12:00:00+00:00',
                                                   '2019-05-21T12:15:00+00:00']})
        time_object3.set_index('timestamp')
        time_object3.index = pd.to_datetime(time_object3.index)

        # Act
        # Assert
        self.assertTrue(is_tz_aware(time_object1))
        self.assertTrue(is_tz_aware(time_object2))
        self.assertTrue(is_tz_aware(time_object3))
        self.assertFalse(is_tz_naive(time_object1))
        self.assertFalse(is_tz_naive(time_object2))
        self.assertFalse(is_tz_naive(time_object3))

    def test_is_tz_awareness_with_various_objects_returns_false(self):
        # Arrange
        time_object1 = datetime(1970, 1, 1, 0, 0, 0, 0)
        time_object2 = pd.Timestamp(datetime(1970, 1, 1, 0, 0, 0, 0))

        # Act
        # Assert
        self.assertFalse(is_tz_aware(time_object1))
        self.assertFalse(is_tz_aware(time_object2))
        self.assertTrue(is_tz_naive(time_object1))
        self.assertTrue(is_tz_naive(time_object2))

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
        self.assertEqual('1970-01-01 00:00:00+00:00', str(pd.to_datetime(dt1, utc=True)))
        self.assertEqual('1970-01-01T00:00:00.000Z', result1)
        self.assertEqual('1970-01-01T00:00:00.000Z', result2)
        self.assertEqual('1970-01-01T00:00:00.001Z', result3)
        self.assertEqual('1970-01-01T00:00:01.000Z', result4)
        self.assertEqual('1970-01-01T01:01:02.003Z', result5)

    def test_datetime_and_pd_timestamp_equality(self):
        # Arrange
        timestamp1 = datetime(1970, 1, 1, 0, 0, 0, 0)
        timestamp2 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0)
        min1 = timedelta(minutes=1)

        # Act
        timestamp3 = timestamp1 + min1
        timestamp4 = timestamp2 + min1
        timestamp5 = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        timestamp6 = timestamp2.tz_localize('UTC')

        # Assert
        self.assertEqual(timestamp1, timestamp2)
        self.assertEqual(timestamp3, timestamp4)
        self.assertEqual(timestamp1.tzinfo, timestamp2.tzinfo)
        self.assertEqual(None, timestamp2.tz)
        self.assertEqual(timestamp5, timestamp6)

    def test_as_utc_timestamp_given_tz_naive_datetime(self):
        # Arrange
        timestamp = datetime(2013, 2, 1, 0, 0, 0, 0)

        # Act
        result = as_timestamp_utc(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_given_tz_naive_pandas_timestamp(self):
        # Arrange
        timestamp = pd.Timestamp(2013, 2, 1, 0, 0, 0, 0)

        # Act
        result = as_timestamp_utc(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_given_tz_aware_datetime(self):
        # Arrange
        timestamp = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        result = as_timestamp_utc(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_given_tz_aware_pandas(self):
        # Arrange
        timestamp = pd.Timestamp(2013, 2, 1, 0, 0, 0, 0).tz_localize('UTC')

        # Act
        result = as_timestamp_utc(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_equality(self):
        # Arrange
        timestamp1 = datetime(1970, 1, 1, 0, 0, 0, 0)
        timestamp2 = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        timestamp3 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0)
        timestamp4 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0).tz_localize('UTC')

        # Act
        timestamp1_converted = as_timestamp_utc(timestamp1)
        timestamp2_converted = as_timestamp_utc(timestamp2)
        timestamp3_converted = as_timestamp_utc(timestamp3)
        timestamp4_converted = as_timestamp_utc(timestamp4)

        # Assert
        self.assertEqual(timestamp1_converted, timestamp2_converted)
        self.assertEqual(timestamp2_converted, timestamp3_converted)
        self.assertEqual(timestamp3_converted, timestamp4_converted)

    def test_with_utc_index_given_tz_unaware_dataframe(self):
        # Arrange
        data = pd.DataFrame({'timestamp': ['2019-05-21T12:00:00+00:00',
                                           '2019-05-21T12:15:00+00:00']})
        data.set_index('timestamp')
        data.index = pd.to_datetime(data.index)

        # Act
        result = with_utc_index(data)

        # Assert
        self.assertEqual(pytz.UTC, result.index.tz)

    def test_with_utc_index_given_tz_aware_dataframe(self):
        # Arrange
        data = pd.DataFrame({'timestamp': ['2019-05-21T12:00:00+00:00',
                                           '2019-05-21T12:15:00+00:00']})
        data.set_index('timestamp')
        data.index = pd.to_datetime(data.index, utc=True)

        # Act
        result = with_utc_index(data)

        # Assert
        self.assertEqual(pytz.UTC, result.index.tz)

    def test_with_utc_index_given_tz_aware_different_timezone_dataframe(self):
        # Arrange
        data1 = pd.DataFrame({'timestamp': ['2019-05-21 12:00:00',
                                            '2019-05-21 12:15:00']})
        data1.set_index('timestamp')
        data1.index = pd.to_datetime(data1.index)

        data2 = pd.DataFrame({'timestamp': [datetime(1970, 1, 1, 0, 0, 0, 0),
                                            datetime(1970, 1, 1, 0, 0, 0, 0)]})
        data2.set_index('timestamp')
        data2.index = pd.to_datetime(data2.index, utc=True)

        # Act
        result1 = with_utc_index(data1)
        result2 = with_utc_index(data2)

        # Assert
        self.assertEqual(result1.index[0], result2.index[0])
        self.assertEqual(result1.index.tz, result2.index.tz)
