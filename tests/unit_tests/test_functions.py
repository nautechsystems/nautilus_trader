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

from datetime import timezone

from inv_trader.core.functions import as_utc_timestamp


class TestFunctionsTests(unittest.TestCase):

    def test_as_utc_timestamp_given_tz_naive_datetime(self):
        # Arrange
        timestamp = datetime.datetime(2013, 2, 1, 0, 0, 0, 0)
        # Act
        result = as_utc_timestamp(timestamp)

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

        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)

    def test_as_utc_timestamp_given_tz_aware_pandas(self):
        # Arrange
        timestamp = pd.Timestamp(2013, 2, 1, 0, 0, 0, 0).tz_localize('UTC')

        # Act
        result = as_utc_timestamp(timestamp)

        self.assertEqual(pd.Timestamp('2013-02-01 00:00:00+00:00'), result)
        self.assertEqual(pytz.UTC, result.tz)
