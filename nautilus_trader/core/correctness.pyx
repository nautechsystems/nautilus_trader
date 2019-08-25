# -------------------------------------------------------------------------------------------------
# <copyright file="correctness.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

class ConditionFailed(Exception):
    """
    Represents a failed condition check.
    """


cdef class Condition:
    """
    Provides static methods for the checking of function or method conditions.
    A condition is a predicate which must be true just prior to the execution
    of some section of code for correct behaviour as per the design specification.
    """
    @staticmethod
    cdef true(bint predicate, str description):
        """
        Check the predicate is True.

        :param predicate: The predicate condition to check.
        :param description: The description of the predicate condition.
        :raises ConditionFailed: If the predicate is False.
        """
        if not predicate:
            raise ConditionFailed(f"The predicate {description} was False.")

    @staticmethod
    cdef none(object argument, str param_name):
        """
        Check the argument is None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ConditionFailed: If the argument is not None.
        """
        if argument is not None:
            raise ConditionFailed(f"The {param_name} argument was not None.")

    @staticmethod
    cdef not_none(object argument, str param_name):
        """
        Check the argument is not None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ConditionFailed: If the argument is None.
        """
        if argument is None:
            raise ConditionFailed(f"The {param_name} argument was None.")

    @staticmethod
    cdef valid_string(str argument, str param_name):
        """
        Check the string is not None, empty or whitespace.

        :param argument: The string argument to check.
        :param param_name: The parameter name.
        :raises ConditionFailed: If the string argument is None, empty or whitespace.
        """
        if argument is None:
            raise ConditionFailed(f"The {param_name} string argument was None.")
        if argument == '':
            raise ConditionFailed(f"The {param_name} string argument was empty.")
        if argument.isspace():
            raise ConditionFailed(f"The {param_name} string argument was whitespace.")

    @staticmethod
    cdef equal(object object1, object object2):
        """
        Check the objects are equal.
        
        Note: The given objects must implement the cdef .equals() method.

        :param object1: The first object to check.
        :param object2: The second object to check.
        :raises ConditionFailed: If the objects are not equal.
        """
        if not object1.equals(object2):
            raise ConditionFailed(f"The {object1} object was not equal to the {object2} object.")

    @staticmethod
    cdef type(object argument, object is_type, str param_name):
        """
        Check the argument is of the specified type.

        :param argument: The argument to check.
        :param is_type: The expected argument type.
        :param param_name: The parameter name.
        :raises ConditionFailed: If the object is not of the expected type.
        """
        if not isinstance(argument, is_type):
            raise ConditionFailed(f"The {param_name} argument was not of type {is_type}, type was {type(argument)}.")

    @staticmethod
    cdef type_or_none(object argument, object is_type, str param_name):
        """
        Check the argument is of the specified type, or is None.

        :param argument: The argument to check.
        :param is_type: The expected argument type if it is not None.
        :param param_name: The parameter name.
        :raises ConditionFailed: If the object is not of the expected type, and is not None.
        """
        if argument is None:
            return

        if not isinstance(argument, is_type):
            raise ConditionFailed(f"The {param_name} argument was not of type {is_type} or None, type was {type(argument)}.")

    @staticmethod
    cdef list_type(list collection, type element_type, str collection_name):
        """
        Check the list only contains types of the given element_type to contain.

        :param collection: The list to check.
        :param element_type: The expected element type if not empty.
        :param collection_name: The parameter name.
        :raises ConditionFailed: If the list contains a type other than the given type to contain.
        """
        for element in collection:
            if not isinstance(element, element_type):
                raise ConditionFailed(f"The {collection_name} list contained an element with a type other than {element_type}, type was {type(element)}.")

    @staticmethod
    cdef dict_types(dict collection, type key_type, type value_type, str collection_name):
        """
        Check the dictionary only contains types of the given key and value types to contain.

        :param collection: The dictionary to check.
        :param key_type: The expected type of the keys if dictionary is not empty.
        :param value_type: The expected type of the values if dictionary is not empty.
        :param collection_name: The dictionary name.
        :raises ConditionFailed: If the dictionary contains a key type other than the given key_type to contain.
        :raises ConditionFailed: If the dictionary contains a value type other than the given value_type to contain.
        """
        for key, value in collection.items():
            if not isinstance(key, key_type):
                raise ConditionFailed(f"The {collection_name} dictionary contained a key type other than {key_type}. type = {type(key)}")
            if not isinstance(value, value_type):
                raise ConditionFailed(f"The {collection_name} dictionary contained a value type other than {value_type}. type = {type(value)}")

    @staticmethod
    cdef is_in(object element, object collection, str element_name, str collection_name):
        """
        Check the element is contained within the specified collection.
    
        :param element: The element to check.
        :param collection: The collection to check.
        :param element_name: The elements name.
        :param collection_name: The collections name.
        :raises ConditionFailed: If the element is not contained in the collection.
        """
        if element not in collection:
            raise ConditionFailed(f"The {element_name} {element} was not contained in the {collection_name} collection.")

    @staticmethod
    cdef not_in(object element, object collection, str element_name, str collection_name):
        """
        Check the element is not contained within the specified collection.
    
        :param element: The element to check.
        :param collection: The collection to check.
        :param element_name: The element name.
        :param collection_name: The collections name.
        :raises ConditionFailed: If the element is already contained in the collection.
        """
        if element in collection:
            raise ConditionFailed(f"The {element_name} {element} was already contained in the {collection_name} collection.")

    @staticmethod
    cdef not_empty(object collection, str param_name):
        """
        Check the collection is not empty.

        :param collection: The collection to check.
        :param param_name: The collections name.
        :raises ConditionFailed: If the collection is empty.
        """
        if len(collection) == 0:
            raise ConditionFailed(f"The {param_name} was empty.")

    @staticmethod
    cdef empty(object collection, str param_name):
        """
        Check the collection is empty.

        :param collection: The collection to check.
        :param param_name: The collections name.
        :raises ConditionFailed: If the collection is not empty.
        """
        if len(collection) > 0:
            raise ConditionFailed(f"The {param_name} was not empty.")

    @staticmethod
    cdef equal_length(
            object collection1,
            object collection2,
            str collection1_name,
            str collection2_name):
        """
        Check the collections have equal lengths.

        :param collection1: The first collection to check.
        :param collection2: The second collection to check.
        :param collection1_name: The first collections name.
        :param collection2_name: The second collections name.
        :raises ConditionFailed: If the collection lengths are not equal.
        """
        if len(collection1) != len(collection2):
            raise ConditionFailed(
                f"The length of {collection1_name} was not equal to {collection2_name} (lengths were {len(collection1)} and {len(collection2)}).")

    @staticmethod
    cdef positive(float value, str param_name):
        """
        Check the float value is positive (> 0)

        :param value: The value to check.
        :param param_name: The name of the value.
        :raises ConditionFailed: If the value is not positive (> 0).
        """
        if value <= 0:
            raise ConditionFailed(f"The {param_name} was not positive, value was {value}.")

    @staticmethod
    cdef not_negative(float value, str param_name):
        """
        Check the float value is not negative (>= 0).

        :param value: The value to check.
        :param param_name: The values name.
        :raises ConditionFailed: If the value is negative (< 0).
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
        Check the float value is within the specified range (inclusive).

        :param value: The value to check.
        :param param_name: The values name.
        :param start: The start of the range.
        :param end: The end of the range.
        :raises ConditionFailed: If the value is not in the inclusive range.
        """
        if value < start or value > end:
            raise ConditionFailed(f"The {param_name} was out of range [{start}-{end}], value was {value}.")


class PyCondition:

    @staticmethod
    def true(predicate, description):
        Condition.true(predicate, description)

    @staticmethod
    def none(argument, param_name):
        Condition.none(argument, param_name)

    @staticmethod
    def not_none(argument, param_name):
        Condition.not_none(argument, param_name)

    @staticmethod
    def type(argument, is_type, param_name):
        Condition.type(argument, is_type, param_name)

    @staticmethod
    def type_or_none(argument, is_type, param_name):
        Condition.type_or_none(argument, is_type, param_name)

    @staticmethod
    def list_type(collection, element_type, collection_name):
        Condition.list_type(collection, element_type, collection_name)

    @staticmethod
    def dict_types(collection, key_type, value_type, collection_name):
        Condition.dict_types(collection, key_type, value_type, collection_name)

    @staticmethod
    def is_in(object element, object collection, str element_name, str collection_name):
        Condition.is_in(element, collection, element_name, collection_name)

    @staticmethod
    def not_in(object element, object collection, str element_name, str collection_name):
        Condition.not_in(element, collection, element_name, collection_name)

    @staticmethod
    def valid_string(argument, param_name):
        Condition.valid_string(argument, param_name)

    @staticmethod
    def equal(argument1, argument2):
        Condition.equal(argument1, argument2)

    @staticmethod
    def not_empty(argument, param_name):
        Condition.not_empty(argument, param_name)

    @staticmethod
    def empty(argument, param_name):
        Condition.empty(argument, param_name)

    @staticmethod
    def equal_length(collection1, collection2, collection1_name, collection2_name):
        Condition.equal_length(collection1,
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
