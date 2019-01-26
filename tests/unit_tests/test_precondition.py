#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_precondition.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal

from inv_trader.core.precondition import PyPrecondition
from inv_trader.model.identifiers import OrderId


class PreconditionTests(unittest.TestCase):

    def test_precondition_true_when_predicate_false_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.true, False, "predicate")

    def test_precondition_true_when_predicate_true_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.true(True, "predicate")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_type_when_type_is_incorrect_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.type, "a string", int, "param_name")

    def test_precondition_type_when_type_is_correct_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.type("a string", str, "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_type_or_none_when_type_is_incorrect_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.type, "a string", int, "param_name")

    def test_precondition_type_or_none_when_type_is_correct_or_none_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.type_or_none("a string", str, "param_name")
        PyPrecondition.type_or_none(None, str, "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_is_in_when_key_not_in_dictionary_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyPrecondition.is_in, 'a', {'b': 1}, 'key', 'dict')

    def test_precondition_is_in_when_key_is_in_dictionary_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyPrecondition.is_in('a', {'a': 1}, 'key', 'dict')
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_not_in_when_key_is_in_dictionary_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyPrecondition.not_in, 'a', {'a': 1}, 'key', 'dict')

    def test_precondition_not_in_when_key_not_in_dictionary_does_nothing(self):
        # Arrange
        # Act
        # Assert
        PyPrecondition.not_in('b', {'a': 1}, 'key', 'dict')
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_list_type_when_contains_incorrect_types_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.list_type, ['a', 'b', 3], str, "param_name")

    def test_precondition_list_type_when_contains_correct_types_or_none_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.list_type(['a', 'b', 'c'], str, "param_name")
        PyPrecondition.list_type([], None, "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_dict_types_when_contains_incorrect_types_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.dict_types, {'key': 1}, str, str, "param_name")
        self.assertRaises(ValueError, PyPrecondition.dict_types, {1: 1}, str, str, "param_name")
        self.assertRaises(ValueError, PyPrecondition.dict_types, {1: "value"}, str, str, "param_name")

    def test_precondition_dict_types_when_contains_correct_types_or_none_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.dict_types({'key': 1}, str, int, "param_name")
        PyPrecondition.dict_types({}, str, str, "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_is_none_when_arg_not_none_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.none, "something", "param_name")

    def test_precondition_is_none_when_arg_is_none_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.none(None, "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_not_none_when_arg_none_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.not_none, None, "param_name")

    def test_precondition_not_none_when_arg_not_none_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.not_none("something", "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_valid_with_various_invalid_strings_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.valid_string, None, "param_name")
        self.assertRaises(ValueError, PyPrecondition.valid_string, "", "param_name")
        self.assertRaises(ValueError, PyPrecondition.valid_string, " ", "param_name")
        self.assertRaises(ValueError, PyPrecondition.valid_string, "   ", "param_name")

        long_string = 'x'
        for i in range(1024):
            long_string += 'x'

        self.assertRaises(ValueError, PyPrecondition.valid_string, long_string, "param_name")

    def test_precondition_not_empty_or_whitespace_with_valid_string_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.valid_string("123", "param_name")
        PyPrecondition.valid_string(" 123", "param_name")
        PyPrecondition.valid_string("abc  ", "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_equal_when_args_not_equal_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.equal, OrderId('123456'), OrderId('123'))

    def test_precondition_equal_when_args_are_equal_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.equal(OrderId('123456'), OrderId('123456'))
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_equal_lengths_when_args_not_equal_lengths_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.equal_lengths, [1], [1, 2], "1", "2")
        self.assertRaises(ValueError, PyPrecondition.equal_lengths, [1], [1, 2], "1", "2")

    def test_precondition_equal_lengths_when_args_are_equal_lengths_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.equal_lengths([1], [1], "collection1", "collection2")
        PyPrecondition.equal_lengths([1, 2, 3], [1, 2, 3], "collection1", "collection2")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_not_negative_when_arg_negative_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.not_negative, -float("inf"), "param_name")
        self.assertRaises(ValueError, PyPrecondition.not_negative, -1, "param_name")
        self.assertRaises(ValueError, PyPrecondition.not_negative, -0.00000000000000001, "param_name")
        self.assertRaises(ValueError, PyPrecondition.not_negative, Decimal('-0.1'), "param_name")

    def test_precondition_not_negative_when_args_zero_or_positive_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.not_negative(0, "param_name")
        PyPrecondition.not_negative(1, "param_name")
        PyPrecondition.not_negative(0., "param_name")
        PyPrecondition.not_negative(Decimal('0'), "param_name")
        PyPrecondition.not_negative(float("inf"), "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_positive_when_args_negative_or_zero_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.positive, -float("inf"), "param_name")
        self.assertRaises(ValueError, PyPrecondition.positive, -0.0000000001, "param_name")
        self.assertRaises(ValueError, PyPrecondition.positive, 0, "param_name")
        self.assertRaises(ValueError, PyPrecondition.positive, 0., "param_name")
        self.assertRaises(ValueError, PyPrecondition.positive, Decimal('0'), "param_name")

    def test_precondition_positive_when_args_positive_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.positive(float("inf"), "param_name")
        PyPrecondition.positive(0.000000000000000000000000000000000001, "param_name")
        PyPrecondition.positive(1, "param_name")
        PyPrecondition.positive(1., "param_name")
        PyPrecondition.positive(Decimal('1.0'), "param_name")
        self.assertTrue(True)  # AssertionError not raised.

    def test_precondition_in_range_when_arg_out_of_range_raises_value_error(self):
        # arrange
        # act
        self.assertRaises(ValueError, PyPrecondition.in_range, -1, "param_name", 0, 1)
        self.assertRaises(ValueError, PyPrecondition.in_range, 2, "param_name", 0, 1)
        self.assertRaises(ValueError, PyPrecondition.in_range, -0.000001, "param_name", 0., 1.)
        self.assertRaises(ValueError, PyPrecondition.in_range, 1.0000001, "param_name", 0., 1.)
        self.assertRaises(ValueError, PyPrecondition.in_range, Decimal('-1.0'), "param_name", 0, 1)

    def test_precondition_in_range_when_args_in_range_does_nothing(self):
        # arrange
        # act
        PyPrecondition.in_range(0, "param_name", 0, 1)
        PyPrecondition.in_range(1, "param_name", 0, 1)
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_not_empty_when_collection_empty_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.not_empty, [], "some_collection")

    def test_precondition_not_empty_when_collection_not_empty_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.not_empty([1], "some_collection")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_empty_when_collection_not_empty_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, PyPrecondition.empty, [1, 2], "some_collection")

    def test_precondition_empty_when_collection_empty_does_nothing(self):
        # arrange
        # act
        # assert
        PyPrecondition.empty([], "some_collection")
        self.assertTrue(True)  # ValueError not raised.
