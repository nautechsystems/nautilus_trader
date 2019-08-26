# -------------------------------------------------------------------------------------------------
# <copyright file="test_core_correctness.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal

from nautilus_trader.core.correctness import ConditionFailed, PyCondition
from nautilus_trader.model.identifiers import OrderId, PositionId


class ConditionTests(unittest.TestCase):

    def test_condition_true_when_predicate_false_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.true, False, "predicate")

    def test_condition_true_when_predicate_true_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.true(True, "this should be true")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_is_none_when_arg_not_none_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.none, "something", "param_name")

    def test_condition_is_none_when_arg_is_none_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.none(None, "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_not_none_when_arg_none_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.not_none, None, "param_name")

    def test_condition_not_none_when_arg_not_none_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.not_none("something", "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_valid_with_various_invalid_strings_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.valid_string, None, "param_name")
        self.assertRaises(ConditionFailed, PyCondition.valid_string, "", "param_name")
        self.assertRaises(ConditionFailed, PyCondition.valid_string, " ", "param_name")
        self.assertRaises(ConditionFailed, PyCondition.valid_string, "   ", "param_name")

    def test_condition_valid_string_with_valid_string_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.valid_string("123", "param_name")
        PyCondition.valid_string(" 123", "param_name")
        PyCondition.valid_string("abc  ", "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_equal_when_args_not_equal_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.equal, OrderId('123456'), OrderId('123'))
        self.assertRaises(ConditionFailed, PyCondition.equal, OrderId('123456'), PositionId('123456'))

    def test_condition_equal_when_args_are_equal_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.equal(OrderId('123456'), OrderId('123456'))
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_type_when_type_is_incorrect_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.type, "a string", int, "param_name")

    def test_condition_type_when_type_is_correct_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.type("a string", str, "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_type_or_none_when_type_is_incorrect_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.type, "a string", int, "param_name")

    def test_condition_type_or_none_when_type_is_correct_or_none_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.type_or_none("a string", str, "param_name")
        PyCondition.type_or_none(None, str, "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_list_type_when_contains_incorrect_types_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.list_type, ['a', 'b', 3], str, "param_name")

    def test_condition_list_type_when_contains_correct_types_or_none_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.list_type(['a', 'b', 'c'], str, "param_name")
        PyCondition.list_type([], None, "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_dict_types_when_contains_incorrect_types_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.dict_types, {'key': 1}, str, str, "param_name")
        self.assertRaises(ConditionFailed, PyCondition.dict_types, {1: 1}, str, str, "param_name")
        self.assertRaises(ConditionFailed, PyCondition.dict_types, {1: "value"}, str, str, "param_name")

    def test_condition_dict_types_when_contains_correct_types_or_none_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.dict_types({'key': 1}, str, int, "param_name")
        PyCondition.dict_types({}, str, str, "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_is_in_when_item_not_in_list_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.is_in, 'a', ['b', 1], 'item', 'list')

    def test_condition_is_in_when_item_is_in_list_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.is_in('a', ['a', 1], 'item', 'list')
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_not_in_when_item_is_in_list_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.not_in, 'a', ['a', 1], 'item', 'list')

    def test_condition_not_in_when_item_not_in_list_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.not_in('b', ['a', 1], 'item', 'list')
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_key_is_in_when_key_not_in_dictionary_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.is_in, 'a', {'b': 1}, 'key', 'dict')

    def test_condition_key_is_in_when_key_is_in_dictionary_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.is_in('a', {'a': 1}, 'key', 'dict')
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_key_not_in_when_key_is_in_dictionary_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.not_in, 'a', {'a': 1}, 'key', 'dict')

    def test_condition_key_not_in_when_key_not_in_dictionary_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.not_in('b', {'a': 1}, 'key', 'dict')
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_not_empty_when_collection_empty_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.not_empty, [], "some_collection")

    def test_condition_not_empty_when_collection_not_empty_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.not_empty([1], "some_collection")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_empty_when_collection_not_empty_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.empty, [1, 2], "some_collection")

    def test_condition_empty_when_collection_empty_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.empty([], "some_collection")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_lists_equal_length_when_args_not_equal_lengths_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.equal_length, [1], [1, 2], "1", "2")
        self.assertRaises(ConditionFailed, PyCondition.equal_length, [1], [1, 2], "1", "2")

    def test_condition_lists_equal_length_when_args_are_equal_lengths_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.equal_length([1], [1], "collection1", "collection2")
        PyCondition.equal_length([1, 2, 3], [1, 2, 3], "collection1", "collection2")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_dicts_equal_length_when_args_not_equal_lengths_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.equal_length, {1: 1}, {1: 1, 2: 2}, "1", "2")
        self.assertRaises(ConditionFailed, PyCondition.equal_length, {1: 1}, {1: 1, 2: 2}, "1", "2")

    def test_condition_dicts_equal_length_when_args_are_equal_lengths_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.equal_length({1: 1}, {1: 1}, "dict1", "dict2")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_not_negative_when_arg_negative_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.not_negative, -float("inf"), "param_name")
        self.assertRaises(ConditionFailed, PyCondition.not_negative, -1, "param_name")
        self.assertRaises(ConditionFailed, PyCondition.not_negative, -0.00000000000000001, "param_name")
        self.assertRaises(ConditionFailed, PyCondition.not_negative, Decimal('-0.1'), "param_name")

    def test_condition_not_negative_when_args_zero_or_positive_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.not_negative(0, "param_name")
        PyCondition.not_negative(1, "param_name")
        PyCondition.not_negative(-0.0, "param_name")
        PyCondition.not_negative(Decimal('0'), "param_name")
        PyCondition.not_negative(float("inf"), "param_name")
        self.assertTrue(True)  # ConditionFailed not raised

    def test_condition_positive_when_args_negative_or_zero_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, PyCondition.positive, -float("inf"), "param_name")
        self.assertRaises(ConditionFailed, PyCondition.positive, -0.0000000001, "param_name")
        self.assertRaises(ConditionFailed, PyCondition.positive, 0, "param_name")
        self.assertRaises(ConditionFailed, PyCondition.positive, 0., "param_name")
        self.assertRaises(ConditionFailed, PyCondition.positive, Decimal('0'), "param_name")

    def test_condition_positive_when_args_positive_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyCondition.positive(float("inf"), "param_name")
        PyCondition.positive(0.000000000000000000000000000000000001, "param_name")
        PyCondition.positive(1, "param_name")
        PyCondition.positive(1.0, "param_name")
        PyCondition.positive(Decimal('1.0'), "param_name")
        self.assertTrue(True)  # AssertionError not raised

    def test_condition_in_range_when_arg_out_of_range_raises_condition_failed(self):
        # Arrange
        # Act
        self.assertRaises(ConditionFailed, PyCondition.in_range, -1, "param_name", 0, 1)
        self.assertRaises(ConditionFailed, PyCondition.in_range, 2, "param_name", 0, 1)
        self.assertRaises(ConditionFailed, PyCondition.in_range, -0.000001, "param_name", 0., 1.)
        self.assertRaises(ConditionFailed, PyCondition.in_range, 1.0000001, "param_name", 0., 1.)
        self.assertRaises(ConditionFailed, PyCondition.in_range, Decimal('-1.0'), "param_name", 0, 1)

    def test_condition_in_range_when_args_in_range_does_nothing(self):
        # Arrange
        # Act
        PyCondition.in_range(0, "param_name", 0, 1)
        PyCondition.in_range(1, "param_name", 0, 1)
        self.assertTrue(True)  # ConditionFailed not raised
