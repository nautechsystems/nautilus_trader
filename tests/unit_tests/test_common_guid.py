# -------------------------------------------------------------------------------------------------
# <copyright file="test_guid.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.identifiers import GUID
from nautilus_trader.common.guid import TestGuidFactory, LiveGuidFactory


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
