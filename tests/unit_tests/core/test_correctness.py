# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

import pytest

from nautilus_trader.core.correctness import PyCondition


class TestCondition:
    def test_raises_custom_exception(self):
        # Arrange, Act, Assert
        with pytest.raises(RuntimeError):
            PyCondition.is_true(False, "predicate", RuntimeError)

    def test_true_when_predicate_false_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.is_true(False, "predicate")

    def test_true_when_predicate_true_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.is_true(True, "this should be True")

    def test_false_when_predicate_true_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.is_false(True, "predicate")

    def test_false_when_predicate_false_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.is_false(False, "this should be False")

    def test_is_none_when_arg_not_none_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.none("something", "param")

    def test_is_none_when_arg_is_none_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.none(None, "param")

    def test_not_none_when_arg_none_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.not_none(None, "param")

    def test_not_none_when_arg_not_none_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.not_none("something", "param")

    def test_type_when_type_is_incorrect_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.type("a string", int, "param")

    def test_type_when_type_is_correct_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.type("a string", str, "param")

    def test_type_or_none_when_type_is_incorrect_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.type("a string", int, "param")

    def test_type_or_none_when_type_is_correct_or_none_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.type_or_none("a string", str, "param")
        PyCondition.type_or_none(None, str, "param")

    def test_callable_when_arg_not_callable_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.callable(None, "param")

    def test_callable_when_arg_is_callable_does_nothing(self):
        # Arrange
        collection = []

        # Act, Assert: ValueError not raised
        PyCondition.callable(collection.append, "param")

    def test_callable_or_none_when_arg_not_callable_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.callable_or_none("not_callable", "param")

    @pytest.mark.parametrize(
        "value",
        [[].append, None],
    )
    def test_callable_or_none_when_arg_is_callable_or_none_does_nothing(self, value):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.callable_or_none(value, "param")

    @pytest.mark.parametrize(
        ("id1", "id2"),
        [["O-123456", "O-123"], ["O-123456", "P-123456"]],
    )
    def test_equal_when_args_not_equal_raises_value_error(self, id1, id2):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.equal(id1, id2, "id1", "id2")

    def test_equal_when_args_are_equal_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.equal("O-123456", "O-123456", "order_id1", "order_id2")

    def test_not_equal_when_args_not_equal_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.not_equal("O-123456", "O-123456", "order_id1", "order_id2")

    def test_not_equal_when_args_are_not_equal_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.not_equal("O-123456", "O-", "order_id1", "order_id2")

    def test_list_type_when_contains_incorrect_types_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.list_type(["a", "b", 3], str, "param")

    @pytest.mark.parametrize(
        ("value", "list_type"),
        [[["a", "b", "c"], str], [[], None]],
    )
    def test_list_type_when_contains_correct_types_or_none_does_nothing(self, value, list_type):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.list_type(value, list_type, "param")

    @pytest.mark.parametrize(
        "value",
        [{"key": 1}, {1: 1}, {1: "value"}],
    )
    def test_dict_types_when_contains_incorrect_types_raises_type_error(self, value):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.dict_types(value, str, str, "param")

    @pytest.mark.parametrize(
        "value",
        [{"key": 1}, {}],
    )
    def test_dict_types_when_contains_correct_types_or_none_does_nothing(self, value):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.dict_types(value, str, int, "param_name")

    def test_is_in_when_item_not_in_list_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(KeyError):
            PyCondition.is_in("a", ["b", 1], "item", "list")

    def test_is_in_when_item_is_in_list_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.is_in("a", ["a", 1], "item", "list")

    def test_not_in_when_item_is_in_list_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(KeyError):
            PyCondition.not_in("a", ["a", 1], "item", "list")

    def test_not_in_when_item_not_in_list_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.not_in("b", ["a", 1], "item", "list")

    def test_key_is_in_when_key_not_in_dictionary_raises_key_error(self):
        # Arrange, Act, Assert
        with pytest.raises(KeyError):
            PyCondition.is_in("a", {"b": 1}, "key", "dict")

    def test_key_is_in_when_key_is_in_dictionary_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.is_in("a", {"a": 1}, "key", "dict")

    def test_key_not_in_when_key_is_in_dictionary_raises_key_error(self):
        # Arrange, Act, Assert
        with pytest.raises(KeyError):
            PyCondition.not_in("a", {"a": 1}, "key", "dict")

    def test_key_not_in_when_key_not_in_dictionary_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.not_in("b", {"a": 1}, "key", "dict")

    def test_not_empty_when_collection_empty_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.not_empty([], "some_collection")

    def test_not_empty_when_collection_not_empty_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.not_empty([1], "some_collection")

    def test_empty_when_collection_not_empty_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.empty([1, 2], "some_collection")

    def test_empty_when_collection_empty_does_nothing(self):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.empty([], "some_collection")

    @pytest.mark.parametrize(
        "value",
        [-float("inf"), -1e23],
    )
    def test_not_negative_when_arg_negative_raises_value_error(self, value):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.not_negative(value, "param")

    @pytest.mark.parametrize(
        "value",
        [-0.0, float("inf")],
    )
    def test_not_negative_when_args_zero_or_positive_does_nothing(self, value):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.not_negative(value, "param")

    def test_not_negative_int_when_arg_negative_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.not_negative_int(-1, "param")

    @pytest.mark.parametrize(
        "value",
        [0, 1],
    )
    def test_not_negative_int_when_args_zero_or_positive_does_nothing(self, value):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.not_negative_int(value, "param")

    @pytest.mark.parametrize(
        "value",
        [Decimal("-1"), -float("inf"), -0.0000000001, 0, 0.0],
    )
    def test_positive_when_args_negative_or_zero_raises_value_error(self, value):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.positive(value, "param")

    @pytest.mark.parametrize(
        "value",
        [Decimal("1"), float("inf"), 1e-22, 1, 1.0],
    )
    def test_positive_when_args_positive_does_nothing(self, value):
        # Arrange, Act, Assert: AssertionError not raised
        PyCondition.positive(value, "param")

    @pytest.mark.parametrize(
        "value",
        [0, -1],
    )
    def test_positive_int_when_args_negative_or_zero_raises_value_error(self, value):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.positive_int(value, "param")

    def test_positive_int_when_args_positive_does_nothing(self):
        # Arrange, Act, Assert: AssertionError not raised
        PyCondition.positive_int(1, "param")

    @pytest.mark.parametrize(
        ("value", "start", "end"),
        [
            [-1e16, 0.0, 1.0],
            [1 + 1e16, 0.0, 1.0],
        ],
    )
    def test_in_range_when_arg_out_of_range_raises_value_error(self, value, start, end):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.in_range(value, start, end, "param")

    @pytest.mark.parametrize(
        ("value", "start", "end"),
        [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]],
    )
    def test_in_range_when_args_in_range_does_nothing(self, value, start, end):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.in_range(value, start, end, "param")

    @pytest.mark.parametrize(
        ("value", "start", "end"),
        [[-1, 0, 1], [2, 0, 1]],
    )
    def test_in_range_int_when_arg_out_of_range_raises_value_error(self, value, start, end):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.in_range_int(value, start, end, "param")

    @pytest.mark.parametrize(
        ("value", "start", "end"),
        [[0, 0, 1], [1, 0, 1]],
    )
    def test_in_range_int_when_args_in_range_does_nothing(self, value, start, end):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.in_range_int(value, start, end, "param")

    def test_valid_string_given_none_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            PyCondition.valid_string(None, "param")

    @pytest.mark.parametrize(
        "value",
        ["", " ", "  ", "   "],
    )
    def test_valid_string_with_various_invalid_strings_raises_correct_exception(self, value):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            PyCondition.valid_string(value, "param")

    @pytest.mark.parametrize(
        "value",
        ["123", " 123", "abc  ", " xyz "],
    )
    def test_valid_string_with_valid_string_does_nothing(self, value):
        # Arrange, Act, Assert: ValueError not raised
        PyCondition.valid_string(value, "param")
