#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_identifiers.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from inv_trader.common.clock import TestClock
from inv_trader.model.identifiers import GUID, Label, OrderId, PositionId, OrderIdGenerator, PositionIdGenerator
from inv_trader.model.enums import Venue
from inv_trader.model.objects import Symbol


class IdentifierTests(unittest.TestCase):

    def test_GUID_passed_str_raises_exceptions(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, GUID, 'a_fake_uuid')

    def test_GUIDS_passed_different_UUID_are_not_equal(self):
        # Arrange
        # Act
        guid1 = GUID(uuid.uuid4())
        guid2 = GUID(uuid.uuid4())

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


AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class OrderIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_id_generator = OrderIdGenerator(
            id_tag_trader='001',
            id_tag_strategy='001',
            clock=TestClock())

    def test_generate_order_id_with_one_symbol(self):
        # Arrange
        # Act
        result1 = self.order_id_generator.generate(AUDUSD_FXCM)
        result2 = self.order_id_generator.generate(AUDUSD_FXCM)
        result3 = self.order_id_generator.generate(AUDUSD_FXCM)

        # Assert
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-1'), result1)
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-2'), result2)
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-3'), result3)

    def test_generate_order_id_with_two_symbols(self):
        # Arrange
        # Act
        result1 = self.order_id_generator.generate(AUDUSD_FXCM)
        result2 = self.order_id_generator.generate(GBPUSD_FXCM)

        # Assert
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-1'), result1)
        self.assertEqual(OrderId('19700101-000000-001-001-GBPUSD-FXCM-1'), result2)

    def test_generate_order_id_with_two_symbols_multiple_times(self):
        # Arrange
        # Act
        result1 = self.order_id_generator.generate(AUDUSD_FXCM)
        result2 = self.order_id_generator.generate(GBPUSD_FXCM)
        result3 = self.order_id_generator.generate(AUDUSD_FXCM)

        # Assert
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-1'), result1)
        self.assertEqual(OrderId('19700101-000000-001-001-GBPUSD-FXCM-1'), result2)
        self.assertEqual(OrderId('19700101-000000-001-001-AUDUSD-FXCM-2'), result3)


class PositionIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader='001',
            id_tag_strategy='001',
            clock=TestClock())

    def test_generate_order_id_with_one_symbol(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate(AUDUSD_FXCM)
        result2 = self.position_id_generator.generate(AUDUSD_FXCM)
        result3 = self.position_id_generator.generate(AUDUSD_FXCM)

        # Assert
        self.assertEqual(PositionId('19700101-000000-001-001-AUDUSD-FXCM-1'), result1)
        self.assertEqual(PositionId('19700101-000000-001-001-AUDUSD-FXCM-2'), result2)
        self.assertEqual(PositionId('19700101-000000-001-001-AUDUSD-FXCM-3'), result3)

    def test_generate_order_id_with_two_symbols(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate(AUDUSD_FXCM)
        result2 = self.position_id_generator.generate(GBPUSD_FXCM)

        # Assert
        self.assertEqual(PositionId('19700101-000000-001-001-AUDUSD-FXCM-1'), result1)
        self.assertEqual(PositionId('19700101-000000-001-001-GBPUSD-FXCM-1'), result2)

    def test_generate_order_id_with_two_symbols_multiple_times(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate(AUDUSD_FXCM)
        result2 = self.position_id_generator.generate(GBPUSD_FXCM)
        result3 = self.position_id_generator.generate(AUDUSD_FXCM)

        # Assert
        self.assertEqual(PositionId('19700101-000000-001-001-AUDUSD-FXCM-1'), result1)
        self.assertEqual(PositionId('19700101-000000-001-001-GBPUSD-FXCM-1'), result2)
        self.assertEqual(PositionId('19700101-000000-001-001-AUDUSD-FXCM-2'), result3)
