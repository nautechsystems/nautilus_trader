#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="preconditions.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

PRE_FAILED = "Precondition Failed"


cdef class Precondition:
    """
    Provides static methods for the checking of function or method preconditions.
    A precondition is a condition or predicate that must always be true just prior
    to the execution of some section of code, or before an operation in a formal
    specification.
    """
    @staticmethod
    def true(object predicate, str description):
        """
        Check the preconditions predicate is true.

        :param predicate: The predicate condition to check.
        :param description: The description of the predicate condition.
        :raises ValueError: If the predicate is false.
        """
        if not predicate:
            raise ValueError(f"{PRE_FAILED} (the predicate {description} was false).")

    @staticmethod
    def is_none(object argument, str param_name):
        """
        Check the preconditions argument is None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ValueError: The argument is not None.
        """
        if argument is not None:
            raise ValueError(f"{PRE_FAILED} (the {param_name} argument was NOT none).")

    @staticmethod
    def not_none(object argument, str param_name):
        """
        Check the preconditions argument is not None.

        :param argument: The argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the argument is None.
        """
        if argument is None:
            raise ValueError(f"{PRE_FAILED} (the {param_name} argument was none).")

    @staticmethod
    def valid_string(str argument, str param_name):
        """
        Check the preconditions string argument is not None, empty or whitespace.

        :param argument: The string argument to check.
        :param param_name: The parameter name.
        :raises ValueError: If the string argument is None, empty or whitespace.
        """
        if argument is None:
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument was None).")
        if argument is str(""):
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument was empty).")
        if argument.isspace():
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument was whitespace).")
        if len(argument) > 512:
            raise ValueError(f"{PRE_FAILED} (the {param_name} string argument exceeded 512 chars).")

    @staticmethod
    def equal(object argument1, object argument2):
        """
        Check the preconditions arguments are equal.

        :param argument1: The first argument to check.
        :param argument2: The second argument to check.
        :raises ValueError: If the arguments are not equal.
        """
        if argument1 != argument2:
            raise ValueError(f"{PRE_FAILED} (the arguments were NOT equal).")

    @staticmethod
    def equal_lengths(
            object collection1,
            object collection2,
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
                f"(the lengths of {collection1_name} and {collection2_name} were not equal)."))

    @staticmethod
    def positive(double value: double, str param_name: str):
        """
        Check the preconditions value is positive (greater than or equal to zero.)

        :param value: The value to check.
        :param param_name: The name of the value.
        :raises ValueError: If the value is not positive.
        """
        if value <= 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was NOT positive = {value}).")

    @staticmethod
    def not_negative(double value, str param_name):
        """
        Check the preconditions value is positive, and not zero.

        :param value: The value to check.
        :param param_name: The values name.
        :raises ValueError: If the value is not positive, or is equal to zero.
        """
        if value < 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was negative = {value}).")

    @staticmethod
    def in_range(
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
        :raises ValueError: If the value is not in range.
        """
        if value < start or value > end:
            raise ValueError(
                f"{PRE_FAILED} (the {param_name} was out of range [{start} - {end}] = {value}).")

    @staticmethod
    def not_empty(object argument, str param_name):
        """
        Check the preconditions iterable is not empty.

        :param argument: The iterable to check.
        :param param_name: The iterables name.
        :raises ValueError: If the iterable argument is empty.
        """
        if len(argument) == 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was an empty collection).")

    @staticmethod
    def empty(object argument, str param_name):
        """
        Check the preconditions iterable is empty.

        :param argument: The iterable to check.
        :param param_name: The iterables name.
        :raises ValueError: If the iterable argument is not empty.
        """
        if len(argument) > 0:
            raise ValueError(f"{PRE_FAILED} (the {param_name} was NOT an empty collection).")
