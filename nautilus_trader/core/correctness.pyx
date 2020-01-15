# -------------------------------------------------------------------------------------------------
# <copyright file="correctness.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.object cimport PyCallable_Check


cdef class ConditionFailed(Exception):
    """
    Represents a failed condition check.
    """


cdef class Condition:
    """
    Provides static methods for the checking of function or method conditions.
    A condition is a predicate which must be true just prior to the execution
    of some section of code - for correct behaviour as per the design specification.
    """
    @staticmethod
    cdef void true(bint predicate, str description) except *:
        """
        Check the condition predicate is True.

        :param predicate: The condition predicate to check.
        :param description: The description of the condition predicate.
        :raises ConditionFailed: If the condition predicate is False.
        """
        if not predicate:
            raise ConditionFailed(f"The condition predicate \'{description}\' was False")

    @staticmethod
    cdef void none(object argument, str param) except *:
        """
        Check the argument is None.

        :param argument: The argument to check.
        :param param: The arguments parameter name.
        :raises ConditionFailed: If the argument is not None.
        """
        if argument is not None:
            raise ConditionFailed(f"The \'{param}\' argument was not None")

    @staticmethod
    cdef void not_none(object argument, str param) except *:
        """
        Check the argument is not None.

        :param argument: The argument to check.
        :param param: The arguments parameter name.
        :raises ConditionFailed: If the argument is None.
        """
        if argument is None:
            raise ConditionFailed(f"The \'{param}\' argument was None")

    @staticmethod
    cdef void type(object argument, object expected, str param) except *:
        """
        Check the argument is of the specified type.

        :param argument: The object to check.
        :param expected: The expected class type.
        :param param: The arguments parameter name.
        :raises ConditionFailed: If the object is not of the expected type.
        """
        if not isinstance(argument, expected):
            raise ConditionFailed(f"The \'{param}\' argument was not of type {expected}, was {type(argument)}")

    @staticmethod
    cdef void type_or_none(object argument, object expected, str param) except *:
        """
        Check the argument is of the specified type, or is None.

        :param argument: The object to check.
        :param expected: The expected class type (if not None).
        :param param: The arguments parameter name.
        :raises ConditionFailed: If the object is not None and not of the expected type.
        """
        if argument is None:
            return

        Condition.type(argument, expected, param)

    @staticmethod
    cdef void callable(object argument, str param) except *:
        """
        Check the object is callable.

        :param argument: The object to check.
        :param param: The objects parameter name.
        :raises ConditionFailed: If the argument is not callable.
        """
        if not PyCallable_Check(argument):
            raise ConditionFailed(f"The \'{param}\' object was not callable.")

    @staticmethod
    cdef void callable_or_none(object argument, str param) except *:
        """
        Check the object is callable or None.

        :param argument: The object to check.
        :param param: The objects parameter name.
        :raises ConditionFailed: If the argument is not None and not callable.
        """
        if argument is None:
            return

        Condition.callable(argument, param)

    @staticmethod
    cdef void equals(object object1, object object2, str param1, str param2) except *:
        """
        Check the objects are equal.
        Note: The given objects must implement the cdef .equals() method.

        :param object1: The first object to check.
        :param object2: The second object to check.
        :param param1: The first objects parameter name.
        :param param2: The first objects parameter name.
        :raises ConditionFailed: If the objects are not equal.
        """
        if not object1.equals(object2):
            raise ConditionFailed(f"The \'{param1}\' {type(object1)} was not equal to the \'{param2}\' {type(object2)}")

    @staticmethod
    cdef void list_type(list list, type expected_type, str param) except *:
        """
        Check the list only contains types of the given expected type.

        :param list: The list to check.
        :param expected_type: The expected element type (if not empty).
        :param param: The lists parameter name.
        :raises ConditionFailed: If the list is not empty and contains a type other than the expected type.
        """
        Condition.not_none(list, param)

        for element in list:
            if not isinstance(element, expected_type):
                raise ConditionFailed(f"The \'{param}\' list contained an element with a type other than {expected_type}, was {type(element)}")

    @staticmethod
    cdef void dict_types(dict dictionary, type key_type, type value_type, str param) except *:
        """
        Check the dictionary only contains types of the given key and value types to contain.

        :param dictionary: The dictionary to check.
        :param key_type: The expected type of the keys (if not empty).
        :param value_type: The expected type of the values (if not empty).
        :param param: The dictionaries parameter name.
        :raises ConditionFailed: If the dictionary is not empty and contains a key type other than the key_type.
        :raises ConditionFailed: If the dictionary is not empty and contains a value type other than the value_type.
        """
        Condition.not_none(dictionary, param)

        for key, value in dictionary.items():
            if not isinstance(key, key_type):
                raise ConditionFailed(f"The \'{param}\' dictionary contained a key type other than {key_type}, was {type(key)}")
            if not isinstance(value, value_type):
                raise ConditionFailed(f"The \'{param}\' dictionary contained a value type other than {value_type}, was {type(value)}")

    @staticmethod
    cdef void is_in(object element, object collection, str param1, str param2) except *:
        """
        Check the element is contained within the specified collection.
    
        :param element: The element to check.
        :param collection: The collection to check.
        :param param1: The elements parameter name.
        :param param2: The collections name.
        :raises ConditionFailed: If the element is not contained in the collection.
        """
        Condition.not_none(collection, param2)

        if element not in collection:
            raise ConditionFailed(f"The \'{param1}\' {element} was not contained in the {param2} collection")

    @staticmethod
    cdef void not_in(object element, object collection, str param1, str param2) except *:
        """
        Check the element is not contained within the specified collection.
    
        :param element: The element to check.
        :param collection: The collection to check.
        :param param1: The element name.
        :param param2: The collections parameter name.
        :raises ConditionFailed: If the element is already contained in the collection.
        """
        Condition.not_none(collection, param2)

        if element in collection:
            raise ConditionFailed(f"The \'{param1}\' {element} was already contained in the \'{param2}\' collection")

    @staticmethod
    cdef void not_empty(object collection, str param) except *:
        """
        Check the collection is not empty.

        :param collection: The collection to check.
        :param param: The collections parameter name.
        :raises ConditionFailed: If the collection is empty.
        """
        Condition.not_none(collection, param)

        if not collection:
            raise ConditionFailed(f"The \'{param}\' collection was empty")

    @staticmethod
    cdef void empty(object collection, str param) except *:
        """
        Check the collection is empty.

        :param collection: The collection to check.
        :param param: The collections parameter name.
        :raises ConditionFailed: If the collection is not empty.
        """
        Condition.not_none(collection, param)

        if collection:
            raise ConditionFailed(f"The \'{param}\' collection was not empty")

    @staticmethod
    cdef void equal_length(
            object collection1,
            object collection2,
            str param1,
            str param2) except *:
        """
        Check the collections have equal lengths.

        :param collection1: The first collection to check.
        :param collection2: The second collection to check.
        :param param1: The first collections parameter name.
        :param param2: The second collections parameter name.
        :raises ConditionFailed: If the collection lengths are not equal.
        """
        Condition.not_none(collection1, param1)
        Condition.not_none(collection2, param2)

        if len(collection1) != len(collection2):
            raise ConditionFailed(
                f"The length of \'{param1}\' was not equal to \'{param2}\', lengths were {len(collection1)} and {len(collection2)}")

    @staticmethod
    cdef void positive(double value, str param) except *:
        """
        Check the real number value is positive (> 0).

        :param value: The value to check.
        :param param: The name of the values parameter.
        :raises ConditionFailed: If the value is not positive (> 0).
        """
        if value <= 0.:
            raise ConditionFailed(f"The \'{param}\' was not a positive real, was {value}")

    @staticmethod
    cdef void positive_int(int value, str param) except *:
        """
        Check the integer value is a positive integer (> 0).

        :param value: The value to check.
        :param param: The name of the values parameter.
        :raises ConditionFailed: If the value is not positive (> 0).
        """
        if value <= 0:
            raise ConditionFailed(f"The \'{param}\' was not a positive integer, was {value}")

    @staticmethod
    cdef void not_negative(double value, str param) except *:
        """
        Check the real number value is not negative (< 0).

        :param value: The value to check.
        :param param: The name of the values parameter.
        :raises ConditionFailed: If the value is a negative integer (< 0).
        """
        if value < 0.:
            raise ConditionFailed(f"The \'{param}\' was a negative real, was {value}")

    @staticmethod
    cdef void not_negative_int(int value, str param) except *:
        """
        Check the integer value is not negative (< 0).

        :param value: The value to check.
        :param param: The name of the values parameter.
        :raises ConditionFailed: If the value is a negative integer (< 0).
        """
        if value < 0:
            raise ConditionFailed(f"The \'{param}\' was a negative integer, was {value}")

    @staticmethod
    cdef void in_range(double value, double start, double end, str param) except *:
        """
        Check the real number value is within the specified range (inclusive).

        :param value: The value to check.
        :param start: The start of the range.
        :param end: The end of the range.
        :param param: The name of the values parameter.
        :raises ConditionFailed: If the value is not in the inclusive range.
        """
        if value < start or value > end:
            raise ConditionFailed(f"The \'{param}\' was out of range [{start}-{end}], was {value}")

    @staticmethod
    cdef void in_range_int(int value, int start, int end, str param) except *:
        """
        Check the integer value is within the specified range (inclusive).

        :param value: The value to check.
        :param start: The start of the range.
        :param end: The end of the range.
        :param param: The name of the values parameter.
        :raises ConditionFailed: If the value is not in the inclusive range.
        """
        if value < start or value > end:
            raise ConditionFailed(f"The \'{param}\' was out of range [{start}-{end}], was {value}")

    @staticmethod
    cdef void valid_string(str argument, str param) except *:
        """
        Check the string argument is not None, empty or whitespace.

        :param argument: The string argument to check.
        :param param: The arguments parameter name.
        :raises ConditionFailed: If the string argument is None, empty or whitespace.
        """
        Condition.not_none(argument, param)

        if argument == '':
            raise ConditionFailed(f"The \'{param}\' string argument was empty")
        if argument.isspace():
            raise ConditionFailed(f"The \'{param}\' string argument was whitespace")

    @staticmethod
    cdef void valid_port(int value, str param) except *:
        """
        Check the port integer value is valid in range [0, 65535].

        :param value: The integer value to check.
        :param param: The name of the ports parameter.
        :raises ConditionFailed: If the value is not in range [0, 65535].
        """
        Condition.in_range_int(value, 0, 65535, param)


class PyCondition:

    @staticmethod
    def true(predicate, description):
        Condition.true(predicate, description)

    @staticmethod
    def none(argument, param):
        Condition.none(argument, param)

    @staticmethod
    def not_none(argument, param):
        Condition.not_none(argument, param)

    @staticmethod
    def type(argument, expected_type, param):
        Condition.type(argument, expected_type, param)

    @staticmethod
    def type_or_none(argument, expected_type, param):
        Condition.type_or_none(argument, expected_type, param)

    @staticmethod
    def callable(argument, param):
        Condition.callable(argument, param)

    @staticmethod
    def callable_or_none(argument, param):
        Condition.callable_or_none(argument, param)

    @staticmethod
    def equals(argument1, argument2, param1, param2):
        Condition.equals(argument1, argument2, param1, param2)

    @staticmethod
    def list_type(list, expected_type, param):
        Condition.list_type(list, expected_type, param)

    @staticmethod
    def dict_types(dictionary, key_type, value_type, param):
        Condition.dict_types(dictionary, key_type, value_type, param)

    @staticmethod
    def is_in(object element, object collection, str param1, str param2):
        Condition.is_in(element, collection, param1, param2)

    @staticmethod
    def not_in(object element, object collection, str param1, str param2):
        Condition.not_in(element, collection, param1, param2)

    @staticmethod
    def not_empty(argument, param):
        Condition.not_empty(argument, param)

    @staticmethod
    def empty(argument, param):
        Condition.empty(argument, param)

    @staticmethod
    def equal_length(collection1, collection2, param1, param2):
        Condition.equal_length(collection1, collection2, param1, param2)

    @staticmethod
    def positive(value, param):
        Condition.positive(value, param)

    @staticmethod
    def positive_int(value, param):
        Condition.positive_int(value, param)

    @staticmethod
    def not_negative(value, param):
        Condition.not_negative(value, param)

    @staticmethod
    def not_negative_int(value, param):
        Condition.not_negative_int(value, param)

    @staticmethod
    def in_range(value, start, end, param):
        Condition.in_range(value, start, end, param)

    @staticmethod
    def in_range_int(value, start, end, param):
        Condition.in_range_int(value, start, end, param)

    @staticmethod
    def valid_string(argument, param):
        Condition.valid_string(argument, param)

    @staticmethod
    def valid_port(int value, param):
        Condition.valid_port(value, param)
