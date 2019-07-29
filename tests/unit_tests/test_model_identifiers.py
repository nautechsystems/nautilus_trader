# -------------------------------------------------------------------------------------------------
# <copyright file="test_model_identifiers.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.identifiers import GUID, Label, OrderId, PositionId, OrderIdGenerator, PositionIdGenerator


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
        self.assertEqual('Label(some-label)', result)

    def test_label_repr(self):
        # Arrange
        label = Label('some-label')

        # Act
        result = repr(label)

        # Assert
        self.assertTrue(result.startswith('<Label(some-label) object at'))

    def test_order_id_equality(self):
        # Arrange
        order_id1 = OrderId('some-order_id-1')
        order_id2 = OrderId('some-order_id-2')

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
        # Assert
        self.assertTrue(id1 == id1)
        self.assertFalse(id1 == id2)


class OrderIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_id_generator = OrderIdGenerator(
            id_tag_trader='001',
            id_tag_strategy='001',
            clock=TestClock())

    def test_generate_order_id(self):
        # Arrange
        # Act
        result1 = self.order_id_generator.generate()
        result2 = self.order_id_generator.generate()
        result3 = self.order_id_generator.generate()

        # Assert
        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), result1)
        self.assertEqual(OrderId('O-19700101-000000-001-001-2'), result2)
        self.assertEqual(OrderId('O-19700101-000000-001-001-3'), result3)

    def test_can_reset_id_generator(self):
        # Arrange
        self.order_id_generator.generate()
        self.order_id_generator.generate()
        self.order_id_generator.generate()

        # Act
        self.order_id_generator.reset()
        result1 = self.order_id_generator.generate()

        # Assert
        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), result1)


class PositionIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader='001',
            id_tag_strategy='001',
            clock=TestClock())

    def test_generate_position_id(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate()
        result2 = self.position_id_generator.generate()
        result3 = self.position_id_generator.generate()

        # Assert
        self.assertEqual(PositionId('P-19700101-000000-001-001-1'), result1)
        self.assertEqual(PositionId('P-19700101-000000-001-001-2'), result2)
        self.assertEqual(PositionId('P-19700101-000000-001-001-3'), result3)

    def test_can_reset_id_generator(self):
        # Arrange
        self.position_id_generator.generate()
        self.position_id_generator.generate()
        self.position_id_generator.generate()

        # Act
        self.position_id_generator.reset()
        result1 = self.position_id_generator.generate()

        # Assert
        self.assertEqual(PositionId('P-19700101-000000-001-001-1'), result1)
