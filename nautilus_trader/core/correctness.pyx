# -------------------------------------------------------------------------------------------------
# <copyright file="correctness.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

class ConditionFailed(Exception):
    pass


cdef class Condition:
    """
    Provides static methods for the checking of function or method conditions.
    A condition is a predicate which must be true just prior to the execution
    of some section of code for correct behaviour as per the specification.
    """
    @staticmethod
    cdef true(bint predicate, str description):
        """
        Check the conditions predicate is True.

        :param predicate: The predicate condition to check.
        :param description: The description of the predicate condition.
        :raises ValueError: If the predicate is False.
        """
        if not predicate:
            raise ConditionFailed(f"The predicate {description} was False.")

    @staticmethod
    cdef type(object argument, object is_type, str param_name):
        """
        Check the conditions argument is of the specified type.

        :param argument: The argument to check.
        :param is_type: The expected argument type.
        :param param_name: The parameter name.
        :raises ValueError: If the object is not of the expected type.
        """
        if not isinstance(argument, is_type):
            raise ConditionFailed(f"The {param_name} argument was not of type {is_type}, type was {type(argument)}.")

    @staticmethod
    cdef type_or_none(object argument, object is_type, str param_name):
        """
        Check the conditions argument is of the specified type, or None.

        :param argument: The argument to check.
        :param is_type: The expected argument type if not None.
        :param param_name: The parameter name.
        :raises ValueError: If the object is not of the expected type, and is not None.
        """
        if argument is None:
            return

        if not isinstance(argument, is_type):
            raise ConditionFailed(f"The {param_name} argument was not of type {is_type} or None, type was {type(argument)}.")

    @staticmethod
    cdef is_in(object key, dict dictionary, str param_name, str dict_name):
        """
        Check the conditions key argument is contained within the keys of the 
        specified dictionary.
    
        :param key: The key argument to check.
        :param dictionary: The dictionary which should contain the key argument.
        :param param_name: The key parameter name.
        :param dict_name: The dictionary name.
        :raises ValueError: If the key is not contained in the dictionary keys.
        """
        if key not in dictionary:
            raise ConditionFailed(f"The {param_name} {key} key was not contained within the {dict_name} dictionary.")

    @staticmethod
    cdef not_in(object key, dict dictionary, str param_name, str dict_name):
        """
        Check the conditions key argument is NOT contained within the keys of 
        the specified dictionary.
    
        :param key: The key argument to check.
        :param dictionary: The dictionary which should NOT contain the key argument.
        :param param_name: The key parameter name.
        :param dict_name: The dictionary name.
        :raises ValueError: If the key is already contained in the dictionary keys.
        """
        if key in dictionary:
            raise ConditionFailed(f"The {param_name} {key} key was already contained within the {dict_name} dictionary.")

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
                raise ConditionFailed(f"The {param_name} list contained an element with a type other than {element_type}, type was {type(element)}.")

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
                raise ConditionFailed(f"The {param_name} dictionary contained a key type other than {key_type}. type = {type(key)}")
            if not isinstance(value, value_type):
                raise ConditionFailed(f"The {param_name} dictionary contained a value type other than {value_type}. type = {type(value)}")

    @staticmethod
    cdef none(object argument, str param_name):
        """
        Check the conditions argument is None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the argument is not None.
        """
        if argument is not None:
            raise ConditionFailed(f"The {param_name} argument was not None.")

    @staticmethod
    cdef not_none(object argument, str param_name):
        """
        Check the conditions argument is not None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the argument is None.
        """
        if argument is None:
            raise ConditionFailed(f"The {param_name} argument was None.")

    @staticmethod
    cdef valid_string(str argument, str param_name):
        """
        Check the conditions string argument is not None, empty or whitespace.

        :param argument: The string argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the string argument is None, empty or whitespace.
        """
        if argument is None:
            raise ConditionFailed(f"The {param_name} string argument was None.")
        if argument is '':
            raise ConditionFailed(f"The {param_name} string argument was empty.")
        if argument.isspace():
            raise ConditionFailed(f"The {param_name} string argument was whitespace.")
        if len(argument) > 2048:
            raise ConditionFailed(f"The {param_name} string argument exceeded 2048 chars.")

    @staticmethod
    cdef equal(object argument1, object argument2):
        """
        Check the conditions arguments are equal (the given object must implement .equals).

        :param argument1: The first argument to check.
        :param argument2: The second argument to check.
        :raises ValueError: If the arguments are not equal.
        """
        if not argument1.equals(argument2):
            raise ConditionFailed(f"The arguments were not equal, values = {argument1} and {argument2}.")

    @staticmethod
    cdef equal_lengths(
            list collection1,
            list collection2,
            str collection1_name,
            str collection2_name):
        """
        Check the conditions collections have equal lengths.

        :param collection1: The first collection to check.
        :param collection2: The second collection to check.
        :param collection1_name: The first collections name.
        :param collection2_name: The second collections name.
        :raises ValueError: If the collections lengths are not equal.
        """
        if len(collection1) != len(collection2):
            raise ConditionFailed(
                f"The lengths of {collection1_name} and {collection2_name} were not equal, lengths = {len(collection1)} and {len(collection2)}.")

    @staticmethod
    cdef positive(float value, str param_name):
        """
        Check the conditions value is positive (> 0.)

        :param value: The value to check.
        :param param_name: The name of the value.
        :raises ValueError: If the value is not positive (> 0).
        """
        if value <= 0:
            raise ConditionFailed(f"The {param_name} was not positive, value was {value}.")

    @staticmethod
    cdef not_negative(float value, str param_name):
        """
        Check the conditions value is not negative (>= 0).

        :param value: The value to check.
        :param param_name: The values name.
        :raises ValueError: If the value is negative (< 0).
        """
        if value < 0:
            raise ConditionFailed(f"The {param_name} was negative, value was {value}.")

    @staticmethod
    cdef in_range(
            float value,
            str param_name,
            float start,
            float end):
        """
        Check the conditions value is within the specified range (inclusive).

        :param value: The value to check.
        :param param_name: The values name.
        :param start: The start of the range.
        :param end: The end of the range.
        :raises ValueError: If the value is not in the inclusive range.
        """
        if value < start or value > end:
            raise ConditionFailed(f"The {param_name} was out of range [{start}-{end}], value was {value}.")

    @staticmethod
    cdef not_empty(object argument, str param_name):
        """
        Check the conditions iterable is not empty.

        :param argument: The iterable to check.
        :param param_name: The iterables name.
        :raises ValueError: If the iterable argument is empty.
        """
        if len(argument) == 0:
            raise ConditionFailed(f"The {param_name} was an empty collection.")

    @staticmethod
    cdef empty(object argument, str param_name):
        """
        Check the conditions iterable is empty.

        :param argument: The iterable to check.
        :param param_name: The iterables name.
        :raises ValueError: If the iterable argument is not empty.
        """
        if len(argument) > 0:
            raise ConditionFailed(f"The {param_name} was not an empty collection.")


class PyCondition:

    @staticmethod
    def true(predicate, description):
        Condition.true(predicate, description)

    @staticmethod
    def type(argument, is_type, param_name):
        Condition.type(argument, is_type, param_name)

    @staticmethod
    def type_or_none(argument, is_type, param_name):
        Condition.type_or_none(argument, is_type, param_name)

    @staticmethod
    def is_in(object key, dict dictionary, str param_name, str dict_name):
        Condition.is_in(key, dictionary, param_name, dict_name)

    @staticmethod
    def not_in(object key, dict dictionary, str param_name, str dict_name):
        Condition.not_in(key, dictionary, param_name, dict_name)

    @staticmethod
    def list_type(argument, element_type, param_name):
        Condition.list_type(argument, element_type, param_name)

    @staticmethod
    def dict_types(argument, key_type, value_type, param_name):
        Condition.dict_types(argument, key_type, value_type, param_name)

    @staticmethod
    def none(argument, param_name):
        Condition.none(argument, param_name)

    @staticmethod
    def not_none(argument, param_name):
        Condition.not_none(argument, param_name)

    @staticmethod
    def valid_string(argument, param_name):
        Condition.valid_string(argument, param_name)

    @staticmethod
    def equal(argument1, argument2):
        Condition.equal(argument1, argument2)

    @staticmethod
    def equal_lengths(
            collection1,
            collection2,
            collection1_name,
            collection2_name):
        Condition.equal_lengths(collection1,
                                collection2,
                                collection1_name,
                                collection2_name)

    @staticmethod
    def positive(value, param_name):
        Condition.positive(value, param_name)

    @staticmethod
    def not_negative(value, param_name):
        Condition.not_negative(value, param_name)

    @staticmethod
    def in_range(
            value,
            param_name,
            start,
            end):
        Condition.in_range(value, param_name, start, end)

    @staticmethod
    def not_empty(argument, param_name):
        Condition.not_empty(argument, param_name)

    @staticmethod
    def empty(argument, param_name):
        Condition.empty(argument, param_name)
