#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_testkit.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime

from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()


class TestStubsTests(unittest.TestCase):

    def test_can_get_unix_epoch(self):
        # Arrange
        # Act
        result = TestStubs.unix_epoch()

        # Assert
        self.assertIsInstance(result, datetime)
        self.assertEqual(result, UNIX_EPOCH)
