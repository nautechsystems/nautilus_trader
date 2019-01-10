#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="precondition.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False


cdef class Precondition:
    @staticmethod
    cdef true(bint predicate, str description)
    @staticmethod
    cdef type(object argument, object is_type, str param_name)
    @staticmethod
    cdef type_or_none(object argument, object is_type, str param_name)
    @staticmethod
    cdef list_type(list argument, type type_to_contain, str param_name)
    @staticmethod
    cdef none(object argument, str param_name: str)
    @staticmethod
    cdef not_none(object argument, str param_name)
    @staticmethod
    cdef valid_string(unicode argument, str param_name)
    @staticmethod
    cdef equal(object argument1, object argument2)
    @staticmethod
    cdef equal_lengths(
            list collection1,
            list collection2,
            str collection1_name,
            str collection2_name)
    @staticmethod
    cdef positive(double value, str param_name)
    @staticmethod
    cdef not_negative(double value, str param_name)
    @staticmethod
    cdef in_range(
            double value,
            str param_name,
            double start,
            double end)
    @staticmethod
    cdef not_empty(object argument, str param_name)
    @staticmethod
    cdef empty(object argument, str param_name)
