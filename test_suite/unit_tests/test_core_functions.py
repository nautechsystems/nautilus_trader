# -------------------------------------------------------------------------------------------------
# <copyright file="test_core_functions.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime
import unittest
import pandas as pd
import pytz

from datetime import timezone, timedelta

from nautilus_trader.core.functions import (
    pad_string,
    basis_points_as_percentage,
    format_zulu_datetime,
    as_utc_timestamp,
    with_utc_index,
    max_in_dict,
)

from test_kit.data import TestDataProvider
from test_kit.stubs import UNIX_EPOCH


class TestFunctionsTests(unittest.TestCase):

    def test_pad_string(self):
        # Arrange
        test_string = "1234"

        # Act
        result = pad_string(test_string, 5)

        # Assert
        self.assertEqual(" 1234", result)

    def test_format_zulu_datetime(self):
        # Arrange
        dt1 = UNIX_EPOCH
        dt2 = UNIX_EPOCH + timedelta(microseconds=1)
        dt3 = UNIX_EPOCH + timedelta(milliseconds=1)
        dt4 = UNIX_EPOCH + timedelta(seconds=1)
        dt5 = UNIX_EPOCH + timedelta(hours=1, minutes=1, seconds=2, milliseconds=3)

        # Act
        result1 = format_zulu_datetime(dt1)
        result2 = format_zulu_datetime(dt2)
        result3 = format_zulu_datetime(dt3)
        result4 = format_zulu_datetime(dt4)
        result5 = format_zulu_datetime(dt5)

        # Assert
        self.assertEqual('1970-01-01T00:00:00.000Z', result1)
        self.assertEqual('1970-01-01T00:00:00.000Z', result2)
        self.assertEqual('1970-01-01T00:00:00.001Z', result3)
        self.assertEqual('1970-01-01T00:00:01.000Z', result4)
        self.assertEqual('1970-01-01T01:01:02.003Z', result5)

    def test_basis_points_as_percentage(self):
        # Arrange
        # Act
        result1 = basis_points_as_percentage(0)
        result2 = basis_points_as_percentage(0.020)

        # Assert
        self.assertEqual(0.0, result1)
        self.assertAlmostEqual(0.000002, result2)

    def test_datetime_and_pd_timestamp_equality(self):
        # Arrange
        timestamp1 = datetime.datetime(1970, 1, 1, 0, 0, 0, 0)
        timestamp2 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0)
        min1 = timedelta(minutes=1)

        # Act
        timestamp3 = timestamp1 + min1
        timestamp4 = timestamp2 + min1
        timestamp5 = datetime.datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        timestamp6 = timestamp2.tz_localize('UTC')

        # Assert
        self.assertEqual(timestamp1, timestamp2)
        self.assertEqual(timestamp3, timestamp4)
        self.assertEqual(timestamp1.tzinfo, timestamp2.tzinfo)
        self.assertEqual(None, timestamp2.tz)
        self.assertEqual(timestamp5, timestamp6)

    def test_as_utc_timestamp_given_tz_naive_datetime(self):
        # Arrange
        timestamp = datetime.datetime(2013, 2, 1, 0, 0, 0, 0)

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_given_tz_naive_pandas_timestamp(self):
        # Arrange
        timestamp = pd.Timestamp(2013, 2, 1, 0, 0, 0, 0)

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_given_tz_aware_datetime(self):
        # Arrange
        timestamp = datetime.datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_given_tz_aware_pandas(self):
        # Arrange
        timestamp = pd.Timestamp(2013, 2, 1, 0, 0, 0, 0).tz_localize('UTC')

        # Act
        result = as_utc_timestamp(timestamp)

        # Assert
        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_equality(self):
        # Arrange
        timestamp1 = datetime.datetime(1970, 1, 1, 0, 0, 0, 0)
        timestamp2 = datetime.datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        timestamp3 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0)
        timestamp4 = pd.Timestamp(1970, 1, 1, 0, 0, 0, 0).tz_localize('UTC')

        # Act
        timestamp1_converted = as_utc_timestamp(timestamp1)
        timestamp2_converted = as_utc_timestamp(timestamp2)
        timestamp3_converted = as_utc_timestamp(timestamp3)
        timestamp4_converted = as_utc_timestamp(timestamp4)

        # Assert
        self.assertEqual(timestamp1_converted, timestamp2_converted)
        self.assertEqual(timestamp2_converted, timestamp3_converted)
        self.assertEqual(timestamp3_converted, timestamp4_converted)

    def test_with_utc_index_given_tz_unaware_dataframe(self):
        # Arrange
        data = TestDataProvider.usdjpy_test_ticks()

        # Act
        result = with_utc_index(data)

        # Assert
        self.assertEqual(pytz.UTC, result.index.tz)

    def test_with_utc_index_given_tz_aware_dataframe(self):
        # Arrange
        data = TestDataProvider.usdjpy_test_ticks().tz_localize('UTC')

        # Act
        result = with_utc_index(data)

        # Assert
        self.assertEqual(pytz.UTC, result.index.tz)

    def test_with_utc_index_given_tz_aware_different_timezone_dataframe(self):
        # Arrange
        data1 = TestDataProvider.usdjpy_test_ticks()
        data2 = TestDataProvider.usdjpy_test_ticks().tz_localize('UTC')

        # Act
        result1 = with_utc_index(data1)
        result2 = with_utc_index(data2)

        # Assert
        self.assertEqual(result1.index[0], result2.index[0])
        self.assertEqual(result1.index.tz, result2.index.tz)

    def test_max_in_dict_with_various_dictionaries_returns_expected_key(self):
        # Arrange
        dict1 = {1: 10, 2: 20, 3: 30}
        dict2 = {'a': 10.1, 'c': 30.1, 'b': 20.1, }

        # Act
        result1 = max_in_dict(dict1)
        result2 = max_in_dict(dict2)

        # Assert
        self.assertEqual(3, result1)
        self.assertEqual('c', result2)
