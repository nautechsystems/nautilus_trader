#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_identifiers.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.model.identifiers import Label, OrderId, PositionId, ExecutionId, ExecutionTicket


class IdentifierTests(unittest.TestCase):

    def test_label_equality(self):
        # Arrange
        label1 = Label('some-label-1')
        label2 = Label('some-label-2')

        # Act
        result1 = label1 == label1
        result2 = label1 != label1
        result3 = label1 == label2
        result4 = label1 != label2

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertFalse(result3)
        self.assertTrue(result4)

    def test_label_to_string(self):
        # Arrange
        label = Label('some-label')

        # Act
        result = str(label)

        # Assert
        self.assertEqual('some-label', result)

    def test_label_repr(self):
        # Arrange
        label = Label('some-label')

        # Act
        result = repr(label)

        # Assert
        self.assertTrue(result.startswith('<Label(some-label) object at'))

    def test_order_id_equality(self):
        # Arrange
        order_id1 = Label('some-order_id-1')
        order_id2 = Label('some-order_id-2')

        # Act
        result1 = order_id1 == order_id1
        result2 = order_id1 != order_id1
        result3 = order_id1 == order_id2
        result4 = order_id1 != order_id2

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertFalse(result3)
        self.assertTrue(result4)

    def test_mixed_identifier_equality(self):
        # Arrange
        identifier_string = 'some-id'
        id1 = OrderId(identifier_string)
        id2 = PositionId(identifier_string)

        # Act
        result1 = id1 == id1
        result2 = id1 == id2

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
