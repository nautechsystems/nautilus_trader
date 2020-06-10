# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.correctness import PyCondition


class ConditionTests(unittest.TestCase):

    def test_can_raise_custom_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(RuntimeError, PyCondition.true, False, "predicate", RuntimeError)

    def test_condition_true_when_predicate_false_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.true, False, "predicate")

    def test_condition_true_when_predicate_true_does_nothing(self):
        # Arrange
        # Act
        PyCondition.true(True, "this should be true")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_false_when_predicate_true_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.false, True, "predicate")

    def test_condition_false_when_predicate_false_does_nothing(self):
        # Arrange
        # Act
        PyCondition.false(False, "this should be false")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_is_none_when_arg_not_none_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.none, "something", "param")

    def test_condition_is_none_when_arg_is_none_does_nothing(self):
        # Arrange
        # Act
        PyCondition.none(None, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_not_none_when_arg_none_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.not_none, None, "param")

    def test_condition_not_none_when_arg_not_none_does_nothing(self):
        # Arrange
        # Act
        PyCondition.not_none("something", "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_type_when_type_is_incorrect_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, PyCondition.type, "a string", int, "param")

    def test_condition_type_when_type_is_correct_does_nothing(self):
        # Arrange
        # Act
        PyCondition.type("a string", str, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_type_or_none_when_type_is_incorrect_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, PyCondition.type, "a string", int, "param")

    def test_condition_type_or_none_when_type_is_correct_or_none_does_nothing(self):
        # Arrange
        # Act
        PyCondition.type_or_none("a string", str, "param")
        PyCondition.type_or_none(None, str, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_callable_when_arg_not_callable_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, PyCondition.callable, None, "param")

    def test_condition_callable_when_arg_is_callable_does_nothing(self):
        # Arrange
        collection = []

        # Act
        PyCondition.callable(collection.append, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_callable_or_none_when_arg_not_callable_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, PyCondition.callable_or_none, "not_callable", "param")

    def test_condition_callable_or_none_when_arg_is_callable_or_none_does_nothing(self):
        # Arrange
        collection = []

        # Act
        PyCondition.callable_or_none(collection.append, "param")
        PyCondition.callable_or_none(None, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_equal_when_args_not_equal_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.equal, 'O-123456', 'O-123', 'order_id1', 'order_id2')
        self.assertRaises(ValueError, PyCondition.equal, 'O-123456', 'P-123456', 'order_id', 'position_id')

    def test_condition_equal_when_args_are_equal_does_nothing(self):
        # Arrange
        # Act
        PyCondition.equal('O-123456', 'O-123456', 'order_id1', 'order_id2')

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_not_equal_when_args_not_equal_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.not_equal, 'O-123456', 'O-123456', 'order_id1', 'order_id2')

    def test_condition_not_equal_when_args_are_not_equal_does_nothing(self):
        # Arrange
        # Act
        PyCondition.not_equal('O-123456', 'O-', 'order_id1', 'order_id2')

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_list_type_when_contains_incorrect_types_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, PyCondition.list_type, ['a', 'b', 3], str, "param")

    def test_condition_list_type_when_contains_correct_types_or_none_does_nothing(self):
        # Arrange
        # Act
        PyCondition.list_type(['a', 'b', 'c'], str, "param")
        PyCondition.list_type([], None, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_dict_types_when_contains_incorrect_types_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, PyCondition.dict_types, {'key': 1}, str, str, "param")
        self.assertRaises(TypeError, PyCondition.dict_types, {1: 1}, str, str, "param")
        self.assertRaises(TypeError, PyCondition.dict_types, {1: "value"}, str, str, "param")

    def test_condition_dict_types_when_contains_correct_types_or_none_does_nothing(self):
        # Arrange
        # Act
        PyCondition.dict_types({'key': 1}, str, int, "param_name")
        PyCondition.dict_types({}, str, str, "param_name")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_is_in_when_item_not_in_list_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.is_in, 'a', ['b', 1], 'item', 'list')

    def test_condition_is_in_when_item_is_in_list_does_nothing(self):
        # Arrange
        # Act
        PyCondition.is_in('a', ['a', 1], 'item', 'list')

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_not_in_when_item_is_in_list_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.not_in, 'a', ['a', 1], 'item', 'list')

    def test_condition_not_in_when_item_not_in_list_does_nothing(self):
        # Arrange
        # Act
        PyCondition.not_in('b', ['a', 1], 'item', 'list')

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_key_is_in_when_key_not_in_dictionary_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.is_in, 'a', {'b': 1}, 'key', 'dict')

    def test_condition_key_is_in_when_key_is_in_dictionary_does_nothing(self):
        # Arrange
        # Act
        PyCondition.is_in('a', {'a': 1}, 'key', 'dict')

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_key_not_in_when_key_is_in_dictionary_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.not_in, 'a', {'a': 1}, 'key', 'dict')

    def test_condition_key_not_in_when_key_not_in_dictionary_does_nothing(self):
        # Arrange
        # Act
        PyCondition.not_in('b', {'a': 1}, 'key', 'dict')

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_not_empty_when_collection_empty_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.not_empty, [], "some_collection")

    def test_condition_not_empty_when_collection_not_empty_does_nothing(self):
        # Arrange
        # Act
        PyCondition.not_empty([1], "some_collection")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_empty_when_collection_not_empty_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.empty, [1, 2], "some_collection")

    def test_condition_empty_when_collection_empty_does_nothing(self):
        # Arrange
        # Act
        PyCondition.empty([], "some_collection")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_not_negative_when_arg_negative_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.not_negative, -float("inf"), "param")
        self.assertRaises(ValueError, PyCondition.not_negative, -0.00000000000000001, "param")

    def test_condition_not_negative_when_args_zero_or_positive_does_nothing(self):
        # Arrange
        # Act
        PyCondition.not_negative(-0.0, "param")
        PyCondition.not_negative(float("inf"), "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_not_negative_int_when_arg_negative_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.not_negative_int, -1, "param")

    def test_condition_not_negative_int_when_args_zero_or_positive_does_nothing(self):
        # Arrange
        # Act
        PyCondition.not_negative_int(0, "param")
        PyCondition.not_negative_int(1, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_positive_when_args_negative_or_zero_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.positive, -float("inf"), "param")
        self.assertRaises(ValueError, PyCondition.positive, -0.0000000001, "param")
        self.assertRaises(ValueError, PyCondition.positive, 0, "param")
        self.assertRaises(ValueError, PyCondition.positive, 0., "param")

    def test_condition_positive_when_args_positive_does_nothing(self):
        # Arrange
        # Act
        PyCondition.positive(float("inf"), "param")
        PyCondition.positive(0.000000000000000000000000000000000001, "param")
        PyCondition.positive(1, "param")
        PyCondition.positive(1.0, "param")

        # Assert
        self.assertTrue(True)  # AssertionError not raised

    def test_condition_positive_int_when_args_negative_or_zero_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.positive_int, 0, "param")
        self.assertRaises(ValueError, PyCondition.positive_int, -1, "param")

    def test_condition_positive_int_when_args_positive_does_nothing(self):
        # Arrange
        # Act
        PyCondition.positive_int(1, "param")

        # Assert
        self.assertTrue(True)  # AssertionError not raised

    def test_condition_in_range_when_arg_out_of_range_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.in_range, -0.000001, 0., 1., "param")
        self.assertRaises(ValueError, PyCondition.in_range, 1.0000001, 0., 1., "param")

    def test_condition_in_range_when_args_in_range_does_nothing(self):
        # Arrange
        # Act
        PyCondition.in_range(0., 0., 1., "param")
        PyCondition.in_range(1., 0., 1., "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_in_range_int_when_arg_out_of_range_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.in_range_int, -1, 0, 1, "param")
        self.assertRaises(ValueError, PyCondition.in_range_int, 2, 0, 1, "param")

    def test_condition_in_range_int_when_args_in_range_does_nothing(self):
        # Arrange
        # Act
        PyCondition.in_range_int(0, 0, 1, "param")
        PyCondition.in_range_int(1, 0, 1, "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_valid_with_various_invalid_strings_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.valid_string, None, "param")
        self.assertRaises(ValueError, PyCondition.valid_string, "", "param")
        self.assertRaises(ValueError, PyCondition.valid_string, " ", "param")
        self.assertRaises(ValueError, PyCondition.valid_string, "   ", "param")

    def test_condition_valid_string_with_valid_string_does_nothing(self):
        # Arrange
        # Act
        PyCondition.valid_string("123", "param")
        PyCondition.valid_string(" 123", "param")
        PyCondition.valid_string("abc  ", "param")

        # Assert
        self.assertTrue(True)  # ValueError not raised

    def test_condition_valid_port_when_value_out_of_range_raises_condition_failed(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, PyCondition.valid_port, 49151, "port")
        self.assertRaises(ValueError, PyCondition.valid_port, 65536, "port")

    def test_condition_valid_port_when_in_range_does_nothing(self):
        # Arrange
        # Act
        PyCondition.valid_port(55555, "port")

        # Assert
        self.assertTrue(True)  # ValueError not raised
