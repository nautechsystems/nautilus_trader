#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="precondition.pyx" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False


cdef str PRE_FAILED = "Precondition Failed"


cdef class Precondition:
    """
    Provides static methods for the checking of function or method preconditions.
    A precondition is a condition or predicate that must always be true just prior
    to the execution of some section of code, or before an operation in a formal
    specification.
    """
    @staticmethod
    cdef true(bint predicate, str description):
        """
        Check the preconditions predicate is True.

        :param predicate: The predicate condition to check.
        :param description: The description of the predicate condition.
        :raises ValueError: If the predicate is False.
        """
        if not predicate:
            raise ValueError(f"{PRE_FAILED} (the predicate {description} was False).")

    @staticmethod
    cdef type(object argument, object is_type, str param_name):
        """
        Check the preconditions argument is of the specified type.

        :param argument: The argument to check.
        :param is_type: The expected argument type.
        :param param_name: The parameter name.
        :raises ValueError: If the object is not of the expected type.
        """
        if not isinstance(argument, is_type):
            raise ValueError(f"{PRE_FAILED} (the {param_name} argument was not of type {is_type}). "
                             f"type = {type(argument)}")

    @staticmethod
    cdef type_or_none(object argument, object is_type, str param_name):
        """
        Check the preconditions argument is of the specified type, or None.

        :param argument: The argument to check.
        :param is_type: The expected argument type if not None.
        :param param_name: The parameter name.
        :raises ValueError: If the object is not of the expected type, and is not None.
        """
        if argument is None:
            return

        if not isinstance(argument, is_type):
            raise ValueError(f"{PRE_FAILED} (the {param_name} argument was not of type {is_type}). "
                             f"type = {type(argument)}")

    @staticmethod
    cdef is_in(object key, dict dictionary, str param_name, str dict_name):
        """
        Check the preconditions key argument is contained within the keys of the 
        specified dictionary.
    
        :param key: The key argument to check.
        :param dictionary: The dictionary which should contain the key argument.
        :param param_name: The key parameter name.
        :param dict_name: The dictionary name.
        :raises ValueError: If the key is not contained in the dictionary.
        """
        if key not in dictionary:
            raise ValueError(f"{PRE_FAILED} (the {param_name} {key} was not contained within the "
                             f"keys of {dict_name}.)")

    @staticmethod
    cdef not_in(object key, dict dictionary, str param_name, str dict_name):
        """
        Check the preconditions key argument is NOT contained within the keys of 
        the specified dictionary.
    
        :param key: The key argument to check.
        :param dictionary: The dictionary which should NOT contain the key argument.
        :param param_name: The key parameter name.
        :param dict_name: The dictionary name.
        :raises ValueError: If the key is not contained in the dictionary.
        """
        if key in dictionary:
            raise ValueError(f"{PRE_FAILED} (the {param_name} {key} was already contained within the "
                             f"keys of {dict_name}.)")

    @staticmethod
    cdef list_type(list argument, type element_type, str param_name):
        """
        Check the list only contains types of the given type to contain.

        :param argument: The list argument to check.
        :param element_type: The expected element type if not empty.
        :param param_name: The parameter name.
        :raises ValueError: If the list contains a type other than the given type to contain.
        """
        for element in argument:
            if not isinstance(element, element_type):
                raise ValueError(f"{PRE_FAILED} (the {param_name} list contained an element with a type other than {element_type}). "
                                f"type = {type(element)}")

    @staticmethod
    cdef dict_types(dict argument, type key_type, type value_type, str param_name):
        """
        Check the dictionary only contains types of the given key and value types to contain.

        :param argument: The dictionary argument to check.
        :param key_type: The expected type of the keys if not empty.
        :param value_type: The expected type of the values if not empty.
        :param param_name: The parameter name.
        :raises ValueError: If the dictionary contains a key type other than the given key_type to contain.
        :raises ValueError: If the dictionary contains a value type other than the given value_type to contain.
        """
        for key, value in argument.items():
            if not isinstance(key, key_type):
                raise ValueError(f"{PRE_FAILED} (the {param_name} dictionary contained a key type other than {key_type}). "
                                f"type = {type(key)}")
            if not isinstance(value, value_type):
                raise ValueError(f"{PRE_FAILED} (the {param_name} dictionary contained a value type other than {value_type}). "
                                f"type = {type(value)}")

    @staticmethod
    cdef none(object argument, str param_name):
        """
        Check the preconditions argument is None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the argument is not None.
        """
        if argument is not None:
            raise ValueError(f"{PRE_FAILED} (the {param_name} argument was not None).")

    @staticmethod
    cdef not_none(object argument, str param_name):
        """
        Check the preconditions argument is not None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the argument is None.
        """
        if argument is None:
            raise ValueError(f"{PRE_FAILED} (the {param_name} argument was None).")

    @staticmethod
    cdef valid_string(str argument, str param_name):
        """
        Check the preconditions string argument is not None, empty or whitespace.

        :param argument: The string argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the string argument is None, empty or whitespace.
        """
        if argument is None:
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument was None).")
        if argument is '':
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument was empty).")
        if argument.isspace():
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument was whitespace).")
        if len(argument) > 1024:
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument exceeded 1024 chars).")

    @staticmethod
    cdef equal(object argument1, object argument2):
        """
        Check the preconditions arguments are equal (the given object must implement .equals).

        :param argument1: The first argument to check.
        :param argument2: The second argument to check.
        :raises ValueError: If the arguments are not equal.
        """
        if not argument1.equals(argument2):
            raise ValueError(f"{PRE_FAILED} (the arguments were not equal). "
                             f"values = {argument1} and {argument2}")

    @staticmethod
    cdef equal_lengths(
            list collection1,
            list collection2,
            str collection1_name,
            str collection2_name):
        """
        Check the preconditions collections have equal lengths.

        :param collection1: The first collection to check.
        :param collection2: The second collection to check.
        :param collection1_name: The first collections name.
        :param collection2_name: The second collections name.
        :raises ValueError: If the collections lengths are not equal.
        """
        if len(collection1) != len(collection2):
            raise ValueError((
                f"{PRE_FAILED} "
                f"(the lengths of {collection1_name} and {collection2_name} were not equal)."
                f"values = {len(collection1)} and {len(collection2)}"))

    @staticmethod
    cdef positive(double value, str param_name):
        """
        Check the preconditions value is positive (> 0.)

        :param value: The value to check.
        :param param_name: The name of the value.
        :raises ValueError: If the value is not positive (> 0).
        """
        if value <= 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was not positive). value = {value}")

    @staticmethod
    cdef not_negative(double value, str param_name):
        """
        Check the preconditions value is not negative (>= 0).

        :param value: The value to check.
        :param param_name: The values name.
        :raises ValueError: If the value is negative (< 0).
        """
        if value < 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was negative). value = {value}")

    @staticmethod
    cdef in_range(
            double value,
            str param_name,
            double start,
            double end):
        """
        Check the preconditions value is within the specified range (inclusive).

        :param value: The value to check.
        :param param_name: The values name.
        :param start: The start of the range.
        :param end: The end of the range.
        :raises ValueError: If the value is not in the inclusive range.
        """
        if value < start or value > end:
            raise ValueError(
                f"{PRE_FAILED} (the {param_name} was out of range [{start}-{end}]). value = {value}")

    @staticmethod
    cdef not_empty(object argument, str param_name):
        """
        Check the preconditions iterable is not empty.

        :param argument: The iterable to check.
        :param param_name: The iterables name.
        :raises ValueError: If the iterable argument is empty.
        """
        if len(argument) == 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was an empty collection).")

    @staticmethod
    cdef empty(object argument, str param_name):
        """
        Check the preconditions iterable is empty.

        :param argument: The iterable to check.
        :param param_name: The iterables name.
        :raises ValueError: If the iterable argument is not empty.
        """
        if len(argument) > 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was not an empty collection).")


class PyPrecondition:

    @staticmethod
    def true(predicate, description):
        Precondition.true(predicate, description)

    @staticmethod
    def type(argument, is_type, param_name):
        Precondition.type(argument, is_type, param_name)

    @staticmethod
    def type_or_none(argument, is_type, param_name):
        Precondition.type_or_none(argument, is_type, param_name)

    @staticmethod
    def is_in(object key, dict dictionary, str param_name, str dict_name):
        Precondition.is_in(key, dictionary, param_name, dict_name)

    @staticmethod
    def not_in(object key, dict dictionary, str param_name, str dict_name):
        Precondition.not_in(key, dictionary, param_name, dict_name)

    @staticmethod
    def list_type(argument, element_type, param_name):
        Precondition.list_type(argument, element_type, param_name)

    @staticmethod
    def dict_types(argument, key_type, value_type, param_name):
        Precondition.dict_types(argument, key_type, value_type, param_name)

    @staticmethod
    def none(argument, param_name):
        Precondition.none(argument, param_name)

    @staticmethod
    def not_none(argument, param_name):
        Precondition.not_none(argument, param_name)

    @staticmethod
    def valid_string(argument, param_name):
        Precondition.valid_string(argument, param_name)

    @staticmethod
    def equal(argument1, argument2):
        Precondition.equal(argument1, argument2)

    @staticmethod
    def equal_lengths(
            collection1,
            collection2,
            collection1_name,
            collection2_name):
        Precondition.equal_lengths(collection1,
                                   collection2,
                                   collection1_name,
                                   collection2_name)

    @staticmethod
    def positive(value, param_name):
        Precondition.positive(value, param_name)

    @staticmethod
    def not_negative(value, param_name):
        Precondition.not_negative(value, param_name)

    @staticmethod
    def in_range(
            value,
            param_name,
            start,
            end):
        Precondition.in_range(value, param_name, start, end)

    @staticmethod
    def not_empty(argument, param_name):
        Precondition.not_empty(argument, param_name)

    @staticmethod
    def empty(argument, param_name):
        Precondition.empty(argument, param_name)
