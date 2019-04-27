#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_functions.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime
import unittest
import pandas as pd
import pytz

from datetime import timezone, timedelta

from inv_trader.core.functions import as_utc_timestamp, with_utc_index
from test_kit.data import TestDataProvider


class TestFunctionsTests(unittest.TestCase):

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
