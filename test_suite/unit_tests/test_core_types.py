# -------------------------------------------------------------------------------------------------
# <copyright file="test_core_types.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from nautilus_trader.core.types import GUID


class IdentifierTests(unittest.TestCase):

    def test_GUIDS_passed_different_UUID_are_not_equal(self):
        # Arrange
        # Act
        guid1 = GUID(uuid.uuid4()),
        guid2 = GUID(uuid.uuid4()),

        # Assert
        self.assertNotEqual(guid1, guid2)

    def test_GUID_passed_UUID_are_equal(self):
        # Arrange
        value = uuid.uuid4()

        # Act
        guid1 = GUID(value)
        guid2 = GUID(value)

        # Assert
        self.assertEqual(guid1, guid2)
