#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_guid.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.model.identifiers import GUID
from inv_trader.common.guid import TestGuidFactory, LiveGuidFactory


class TestGuidFactoryTests(unittest.TestCase):

    def test_factory_returns_identical_guids(self):
        # Arrange
        factory = TestGuidFactory()

        # Act
        result1 = factory.generate()
        result2 = factory.generate()
        result3 = factory.generate()

        self.assertEqual(GUID, type(result1))
        self.assertEqual(result1, result2)
        self.assertEqual(result2, result3)


class LiveGuidFactoryTests(unittest.TestCase):

    def test_factory_returns_unique_guids(self):
        # Arrange
        factory = LiveGuidFactory()

        # Act
        result1 = factory.generate()
        result2 = factory.generate()
        result3 = factory.generate()

        self.assertEqual(GUID, type(result1))
        self.assertNotEqual(result1, result2)
        self.assertNotEqual(result2, result3)
