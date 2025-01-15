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

"""
Defines static condition checks similar to the `design by contract` philosophy
to help ensure software correctness.
"""

from cpython.object cimport PyCallable_Check


cdef class Condition:
    """
    Provides checking of function or method conditions.

    A condition is a predicate which must be true just prior to the execution of
    some section of code - for correct behavior as per the design specification.

    If a check fails, then an Exception is thrown with a descriptive message.
    """

    @staticmethod
    cdef void is_true(bint predicate, str fail_msg, ex_type = None):
        """
        Check the condition predicate is True.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is False.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is False.

        """
        if predicate:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=fail_msg,
        )

    @staticmethod
    cdef void is_false(bint predicate, str fail_msg, ex_type = None):
        """
        Check the condition predicate is False.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is True.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is True.

        """
        if not predicate:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=fail_msg,
        )

    @staticmethod
    cdef void none(object argument, str param, ex_type = None):
        """
        Check the argument is ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None``.

        """
        if argument is None:
            return  # Check passed

        raise make_exception(
            ex_default=TypeError,
            ex_type=ex_type,
            msg=f"\'{param}\' argument was not `None`",
        )

    @staticmethod
    cdef void not_none(object argument, str param, ex_type = None):
        """
        Check the argument is not ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is ``None``.

        """
        if argument is not None:
            return  # Check passed

        raise make_exception(
            ex_default=TypeError,
            ex_type=ex_type,
            msg=f"\'{param}\' argument was `None`",
        )

    @staticmethod
    cdef void type(
        object argument,
        object expected,
        str param,
        ex_type = None,
    ):
        """
        Check the argument is of the specified type.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : type or tuple of types
            The expected type(s).
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `object` is not of the expected type.

        """
        if isinstance(argument, expected):
            return  # Check passed

        raise make_exception(
            ex_default=TypeError,
            ex_type=ex_type,
            msg=f"\'{param}\' argument not of type {expected}, was {type(argument)}",
        )

    @staticmethod
    cdef void type_or_none(
        object argument,
        object expected,
        str param,
        ex_type = None,
    ):
        """
        Check the argument is of the specified type, or is ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : type or tuple of types
            The expected type(s) (if not ``None``).
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `object` is not ``None`` and not of the expected type.

        """
        if argument is None:
            return  # Check passed

        Condition.type(argument, expected, param, ex_type)

    @staticmethod
    cdef void callable(object argument, str param, ex_type = None):
        """
        Check the object is of type `Callable`.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The object parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not of type `Callable`.

        """
        if PyCallable_Check(argument):
            return  # Check passed

        raise make_exception(
            ex_default=TypeError,
            ex_type=ex_type,
            msg=f"\'{param}\' object was not callable",
        )

    @staticmethod
    cdef void callable_or_none(object argument, str param, ex_type = None):
        """
        Check the object is of type `Callable` or ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The object parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None`` and not of type `Callable`.

        """
        if argument is None:
            return  # Check passed

        Condition.callable(argument, param, ex_type)

    @staticmethod
    cdef void equal(
        object argument1,
        object argument2,
        str param1,
        str param2,
        ex_type = None,
    ):
        """
        Check the objects are equal.

        Parameters
        ----------
        argument1 : object
            The first object to check.
        argument2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are not equal.

        """
        if argument1 == argument2:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=(f"\'{param1}\' {type(argument1)} of {argument1} "
                 f"was not equal to \'{param2}\' {type(argument2)} of {argument2}"),
        )

    @staticmethod
    cdef void not_equal(
        object object1,
        object object2,
        str param1,
        str param2,
        ex_type = None,
    ):
        """
        Check the objects are not equal.

        Parameters
        ----------
        object1 : object
            The first object to check.
        object2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are equal.

        """
        if object1 != object2:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=(f"\'{param1}\' {type(object1)} of {object1} "
                 f"was equal to \'{param2}\' {type(object2)} of {object1}"),
        )

    @staticmethod
    cdef void list_type(
        list argument,
        type expected_type,
        str param,
        ex_type = None,
    ):
        """
        Check the list only contains types of the given expected type.

        Parameters
        ----------
        argument : list
            The list to check.
        expected_type : type
            The expected element type (if not empty).
        param : str
            The list parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
             If `argument` is not empty and contains a type other than `expected_type`.

        """
        Condition.not_none(argument, param, ex_type)

        if all(isinstance(element, expected_type) for element in argument):
            return  # Check passed

        raise make_exception(
            ex_default=TypeError,
            ex_type=ex_type,
            msg=f"\'{param}\' collection contained an element with type other than {expected_type}",
        )

    @staticmethod
    cdef void dict_types(
        dict argument,
        type key_type,
        type value_type,
        str param,
        ex_type = None,
    ):
        """
        Check the dictionary only contains types of the given key and value types to contain.

        Parameters
        ----------
        argument : dict
            The dictionary to check.
        key_type : type
            The expected type of the keys (if not empty).
        value_type : type
            The expected type of the values (if not empty).
        param : str
            The dictionary parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not empty and contains a key type other than `key_type`.
            If `argument` is not empty and contains a value type other than `value_type`.

        """
        Condition.not_none(argument, param, ex_type)
        Condition.list_type(list(argument.keys()), key_type, f"{param} keys", ex_type)
        Condition.list_type(list(argument.values()), value_type, f"{param} values", ex_type)

    @staticmethod
    cdef void is_in(
        object element,
        object collection,
        str param1,
        str param2,
        ex_type = None,
    ):
        """
        Check the element is contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The elements parameter name.
        param2 : str
            The collections name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is not contained in the `collection`.

        """
        Condition.not_none(collection, param2, ex_type)

        if element in collection:
            return  # Check passed

        raise make_exception(
            ex_default=KeyError,
            ex_type=ex_type,
            msg=f"\'{param1}\' {element} not contained in \'{param2}\' collection",
        )

    @staticmethod
    cdef void not_in(
        object element,
        object collection,
        str param1,
        str param2,
        ex_type = None,
    ):
        """
        Check the element is not contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The elements parameter name.
        param2 : str
            The collections name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is contained in the `collection`.

        """
        Condition.not_none(collection, param2, ex_type)

        if element not in collection:
            return  # Check passed

        raise make_exception(
            ex_default=KeyError,
            ex_type=ex_type,
            msg=f"\'{param1}\' {element} already contained in \'{param2}\' collection",
        )

    @staticmethod
    cdef void not_empty(object collection, str param, ex_type = None):
        """
        Check the collection is not empty.

        Parameters
        ----------
        collection : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is empty.

        """
        Condition.not_none(collection, param, ex_type)

        if collection:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' collection was empty",
        )

    @staticmethod
    cdef void empty(object collection, str param, ex_type = None):
        """
        Check the collection is empty.

        Parameters
        ----------
        collection : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is not empty.

        """
        Condition.not_none(collection, param)

        if not collection:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' collection was not empty",
        )

    @staticmethod
    cdef void positive(double value, str param, ex_type = None):
        """
        Check the real number value is positive (> 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `value` is not positive (> 0).

        """
        if value > 0:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' not a positive real, was {value:_}",
        )

    @staticmethod
    cdef void positive_int(value: int, str param, ex_type = None):
        """
        Check the integer value is a positive integer (> 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `value` is not positive (> 0).

        """
        if value > 0:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' not a positive integer, was {value:_}",
        )

    @staticmethod
    cdef void not_negative(double value, str param, ex_type = None):
        """
        Check the real number value is not negative (< 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        if value >= 0.0:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' not greater than or equal to zero (>= 0), was {value:_}",
        )

    @staticmethod
    cdef void not_negative_int(value: int, str param, ex_type = None):
        """
        Check the integer value is not negative (< 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        if value >= 0:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' not greater than or equal to zero (>= 0), was {value:_}",
        )

    @staticmethod
    cdef void in_range(
        double value,
        double start,
        double end,
        str param,
        ex_type = None,
    ):
        """
        Check the real number value is within the specified range (inclusive).

        This function accounts for potential floating-point precision issues by using a small
        epsilon value of 1e-15.

        Parameters
        ----------
        value : scalar
            The value to check.
        start : scalar
            The start of the range.
        end : scalar
            The end of the range.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        cdef double epsilon = 1e-15  # Epsilon to account for floating-point precision issues
        if start - epsilon <= value <= end + epsilon:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' out of range [{start:_}, {end:_}], was {value:_}",
        )

    @staticmethod
    cdef void in_range_int(
        value,
        start,
        end,
        str param,
        ex_type = None,
    ):
        """
        Check the integer value is within the specified range (inclusive).

        Parameters
        ----------
        value : int
            The value to check.
        start : int
            The start of the range.
        end : int
            The end of the range.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        if start <= value <= end:
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' out of range [{start:_}, {end:_}], was {value:_}",
        )

    @staticmethod
    cdef void valid_string(str argument, str param, ex_type = None):
        """
        Check the string argument is valid (not ``None``, empty or whitespace).

        Parameters
        ----------
        argument : str
            The string argument to check.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `argument` is ``None``, empty or whitespace.

        """
        Condition.not_none(argument, param, ex_type)

        if argument != "" and not argument.isspace():
            return  # Check passed

        raise make_exception(
            ex_default=ValueError,
            ex_type=ex_type,
            msg=f"\'{param}\' string was invalid, was \'{argument}\'",
        )


class PyCondition:

    @staticmethod
    def is_true(bint predicate, str fail_msg, ex_type = None):
        """
        Check the condition predicate is True.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is False.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is False.

        """
        Condition.is_true(predicate, fail_msg, ex_type)

    @staticmethod
    def is_false(bint predicate, str fail_msg, ex_type = None):
        """
        Check the condition predicate is False.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is True
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is True.

        """
        Condition.is_false(predicate, fail_msg, ex_type)

    @staticmethod
    def none(argument, str param, ex_type = None):
        """
        Check the argument is ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None``.

        """
        Condition.none(argument, param, ex_type)

    @staticmethod
    def not_none(argument, str param, ex_type = None):
        """
        Check the argument is not ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is ``None``.

        """
        Condition.not_none(argument, param, ex_type)

    @staticmethod
    def type(argument, expected, str param, ex_type = None):
        """
        Check the argument is of the specified type.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : object
            The expected class type.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not of the expected type.

        """
        Condition.type(argument, expected, param, ex_type)

    @staticmethod
    def type_or_none(argument, expected, str param, ex_type = None):
        """
        Check the argument is of the specified type, or is ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : object
            The expected class type (if not ``None``).
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None`` and not of the expected type.

        """
        Condition.type_or_none(argument, expected, param, ex_type)

    @staticmethod
    def callable(argument, str param, ex_type = None):
        """
        Check the object is of type `Callable`.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not of type `Callable`.

        """
        Condition.callable(argument, param, ex_type)

    @staticmethod
    def callable_or_none(argument, str param, ex_type = None):
        """
        Check the object is of type `Callable` or ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None`` and not of type `Callable`.

        """
        Condition.callable_or_none(argument, param, ex_type)

    @staticmethod
    def equal(argument1, argument2, str param1, str param2, ex_type = None):
        """
        Check the objects are equal.

        Parameters
        ----------
        argument1 : object
            The first object to check.
        argument2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are not equal.

        """
        Condition.equal(argument1, argument2, param1, param2, ex_type)

    @staticmethod
    def not_equal(argument1, argument2, str param1, str param2, ex_type = None):
        """
        Check the objects are not equal.

        Parameters
        ----------
        argument1 : object
            The first object to check.
        argument2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are equal.

        """
        Condition.not_equal(argument1, argument2, param1, param2, ex_type)

    @staticmethod
    def list_type(argument, expected_type, str param, ex_type = None):
        """
        Check the list only contains types of the given expected type.

        Parameters
        ----------
        argument : list
            The list to check.
        expected_type : type
            The expected element type (if not empty).
        param : str
            The list parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
             If `argument` is not empty and contains a type other than `expected_type`.

        """
        Condition.list_type(argument, expected_type, param, ex_type)

    @staticmethod
    def dict_types(argument, key_type, value_type, str param, ex_type = None):
        """
        Check the dictionary only contains types of the given key and value types to contain.

        Parameters
        ----------
        argument : dict
            The dictionary to check.
        key_type : type
            The expected type of the keys (if not empty).
        value_type : type
            The expected type of the values (if not empty).
        param : str
            The dictionary parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not empty and contains a key type other than `key_type`.
            If `argument` is not empty and contains a value type other than `value_type`.

        """
        Condition.dict_types(argument, key_type, value_type, param, ex_type)

    @staticmethod
    def is_in(object element, collection, str param1, str param2, ex_type = None):
        """
        Check the element is contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The element parameter name.
        param2 : str
            The collection name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is not contained in the `collection`.

        """
        Condition.is_in(element, collection, param1, param2, ex_type)

    @staticmethod
    def not_in(object element, collection, str param1, str param2, ex_type = None):
        """
        Check the element is not contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The elements parameter name.
        param2 : str
            The collections name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is contained in the `collection`.

        """
        Condition.not_in(element, collection, param1, param2, ex_type)

    @staticmethod
    def not_empty(argument, str param, ex_type = None):
        """
        Check the collection is not empty.

        Parameters
        ----------
        argument : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is empty.

        """
        Condition.not_empty(argument, param, ex_type)

    @staticmethod
    def empty(argument, str param, ex_type = None):
        """
        Check the collection is empty.

        Parameters
        ----------
        argument : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is not empty.

        """
        Condition.empty(argument, param, ex_type)

    @staticmethod
    def positive(double value, str param, ex_type = None):
        """
        Check the real number value is positive (> 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `value` is not positive (> 0).

        """
        Condition.positive(value, param, ex_type)

    @staticmethod
    def positive_int(value: int, str param, ex_type = None):
        """
        Check the integer value is a positive integer (> 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `value` is not positive (> 0).

        """
        Condition.positive_int(value, param, ex_type)

    @staticmethod
    def not_negative(double value, str param, ex_type = None):
        """
        Check the real number value is not negative (< 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        Condition.not_negative(value, param, ex_type)

    @staticmethod
    def not_negative_int(value: int, str param, ex_type = None):
        """
        Check the integer value is not negative (< 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        Condition.not_negative_int(value, param, ex_type)

    @staticmethod
    def in_range(double value, double start, double end, str param, ex_type = None):
        """
        Check the real number value is within the specified range (inclusive).

        Parameters
        ----------
        value : scalar
            The value to check.
        start : scalar
            The start of the range.
        end : scalar
            The end of the range.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        Condition.in_range(value, start, end, param, ex_type)

    @staticmethod
    def in_range_int(value: int, start: int, end: int, param, ex_type = None):
        """
        Check the integer value is within the specified range (inclusive).

        Parameters
        ----------
        value : int
            The value to check.
        start : int
            The start of the range.
        end : int
            The end of the range.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        Condition.in_range_int(value, start, end, param, ex_type)

    @staticmethod
    def valid_string(str argument, str param, ex_type = None):
        """
        Check the string argument is valid (not ``None``, empty or whitespace).

        Parameters
        ----------
        argument : str
            The string argument to check.
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `argument` is ``None``, empty or whitespace.

        """
        Condition.valid_string(argument, param, ex_type)
