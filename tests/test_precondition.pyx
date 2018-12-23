#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_precondition.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False
# pylint: disable=C0111, C0103,

import unittest

from collections import deque
from decimal import Decimal

from inv_trader.core.precondition cimport Precondition


class PreconditionTests(unittest.TestCase):

    def test_precondition_true_when_predicate_false_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.true, False, "predicate")

    def test_precondition_true_when_predicate_true_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.true(True, "predicate")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_is_none_when_arg_not_none_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.is_none, "something", "param_name")

    def test_precondition_is_none_when_arg_is_none_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.is_none(None, "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_not_none_when_arg_none_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.not_none, None, "param_name")

    def test_precondition_not_none_when_arg_not_none_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.not_none("something", "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_valid_with_various_invalid_strings_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.valid_string, None, "param_name")
        self.assertRaises(ValueError, Precondition.valid_string, "", "param_name")
        self.assertRaises(ValueError, Precondition.valid_string, " ", "param_name")
        self.assertRaises(ValueError, Precondition.valid_string, "   ", "param_name")

        long_string = "x"
        for i in range(1024):
            long_string += "x"

        self.assertRaises(ValueError, Precondition.valid_string, long_string, "param_name")

    def test_precondition_not_empty_or_whitespace_with_valid_string_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.valid_string("123", "param_name")
        Precondition.valid_string(" 123", "param_name")
        Precondition.valid_string("abc  ", "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_equal_when_args_not_equal_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.equal, "", " ")
        self.assertRaises(ValueError, Precondition.equal, 1, 2)

    def test_precondition_equal_when_args_are_equal_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.equal(1, 1)
        Precondition.equal("", "")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_equal_lengths_when_args_not_equal_lengths_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.equal_lengths, [1], [1, 2], "1", "2")
        self.assertRaises(ValueError, Precondition.equal_lengths, deque([1]), deque([1, 2]), "1", "2")

    def test_precondition_equal_lengths_when_args_are_equal_lengths_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.equal_lengths([1], [1], "collection1", "collection2")
        Precondition.equal_lengths(deque([1, 2, 3]), deque([1, 2, 3]), "collection1", "collection2")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_not_negative_when_arg_negative_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.not_negative, -float("inf"), "param_name")
        self.assertRaises(ValueError, Precondition.not_negative, -1, "param_name")
        self.assertRaises(ValueError, Precondition.not_negative, -0.00000000000000001, "param_name")
        self.assertRaises(ValueError, Precondition.not_negative, Decimal('-0.1'), "param_name")

    def test_precondition_not_negative_when_args_zero_or_positive_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.not_negative(0, "param_name")
        Precondition.not_negative(1, "param_name")
        Precondition.not_negative(0., "param_name")
        Precondition.not_negative(Decimal('0'), "param_name")
        Precondition.not_negative(float("inf"), "param_name")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_positive_when_args_negative_or_zero_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.positive, -float("inf"), "param_name")
        self.assertRaises(ValueError, Precondition.positive, -0.0000000001, "param_name")
        self.assertRaises(ValueError, Precondition.positive, 0, "param_name")
        self.assertRaises(ValueError, Precondition.positive, 0., "param_name")
        self.assertRaises(ValueError, Precondition.positive, Decimal('0'), "param_name")

    def test_precondition_positive_when_args_positive_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.positive(float("inf"), "param_name")
        Precondition.positive(0.000000000000000000000000000000000001, "param_name")
        Precondition.positive(1, "param_name")
        Precondition.positive(1., "param_name")
        Precondition.positive(Decimal('1.0'), "param_name")
        self.assertTrue(True)  # AssertionError not raised.

    def test_precondition_in_range_when_arg_out_of_range_raises_value_error(self):
        # arrange
        # act
        self.assertRaises(ValueError, Precondition.in_range, -1, "param_name", 0, 1)
        self.assertRaises(ValueError, Precondition.in_range, 2, "param_name", 0, 1)
        self.assertRaises(ValueError, Precondition.in_range, -0.000001, "param_name", 0., 1.)
        self.assertRaises(ValueError, Precondition.in_range, 1.0000001, "param_name", 0., 1.)
        self.assertRaises(ValueError, Precondition.in_range, Decimal('-1.0'), "param_name", 0, 1)

    def test_precondition_in_range_when_args_in_range_does_nothing(self):
        # arrange
        # act
        Precondition.in_range(0, "param_name", 0, 1)
        Precondition.in_range(1, "param_name", 0, 1)
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_not_empty_when_collection_empty_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.not_empty, [], "some_collection")

    def test_precondition_not_empty_when_collection_not_empty_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.not_empty([1], "some_collection")
        self.assertTrue(True)  # ValueError not raised.

    def test_precondition_empty_when_collection_not_empty_raises_value_error(self):
        # arrange
        # act
        # assert
        self.assertRaises(ValueError, Precondition.empty, [1, 2], "some_collection")

    def test_precondition_empty_when_collection_empty_does_nothing(self):
        # arrange
        # act
        # assert
        Precondition.empty([], "some_collection")
        self.assertTrue(True)  # ValueError not raised.

if __name__ == "__main__":
    PreconditionTests.test_precondition_empty_when_collection_not_empty_raises_value_error()
