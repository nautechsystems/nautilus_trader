#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_clock.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime, timezone

from inv_trader.common.clock import Clock, LiveClock, TestClock


class ClockTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = Clock(timezone.utc)

    def test_timezone(self):
        # Arrange
        # Act
        result = self.clock.get_timezone()

        # Assert
        self.assertEqual(timezone.utc, result)

    def test_unix_epoch(self):
        # Arrange
        # Act
        result = self.clock.unix_epoch()
        # Assert
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc), result)


class LiveClockTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = LiveClock(timezone.utc)

    def test_time_now(self):
        # Arrange
        # Act
        result = self.clock.time_now()

        # Assert
        self.assertEqual(timezone.utc, result.tzinfo)


class TestClockTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock(timezone.utc)

    def test_time_now(self):
        # Arrange
        # Act
        result = self.clock.time_now()

        # Assert
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc), result)

    def test_increment_time(self):
        # Arrange
        # Act
        self.clock.increment_time()
        result = self.clock.time_now()

        # Assert
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 1, 0, timezone.utc), result)

    def test_set_time(self):
        # Arrange
        new_time = datetime(1970, 2, 1, 0, 0, 1, 0, timezone.utc)

        # Act
        self.clock.set_time(new_time)
        result = self.clock.time_now()

        # Assert
        self.assertEqual(new_time, result)
