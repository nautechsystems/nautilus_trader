#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="precondition.pyd" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

cdef class Precondition(object):
    @staticmethod
    cdef void true(bint predicate, unicode description)
    @staticmethod
    cdef void is_none(object argument, unicode param_name)
    @staticmethod
    cdef void not_none(object argument, unicode param_name)
    @staticmethod
    cdef void valid_string(unicode argument, unicode param_name)
    @staticmethod
    cdef void equal(object argument1, object argument2)
    @staticmethod
    cdef void equal_lengths(
            list collection1,
            list collection2,
            str collection1_name,
            str collection2_name)
    @staticmethod
    cdef void positive(double value, unicode param_name)
    @staticmethod
    cdef void not_negative(double value, unicode param_name)
    @staticmethod
    cdef void in_range(
            double value,
            str param_name,
            double start,
            double end)
    @staticmethod
    cdef void not_empty(object argument, unicode param_name)
    @staticmethod
    cdef void empty(object argument, unicode param_name)
