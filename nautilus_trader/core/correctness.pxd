# -------------------------------------------------------------------------------------------------
# <copyright file="correctness.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cdef class Condition:
    @staticmethod
    cdef void true(bint predicate, str description) except *
    @staticmethod
    cdef void none(object argument, str param_name) except *
    @staticmethod
    cdef void not_none(object argument, str param_name) except *
    @staticmethod
    cdef void valid_string(str argument, str param_name) except *
    @staticmethod
    cdef void equal(object argument1, object argument2) except *
    @staticmethod
    cdef void type(object argument, object expected_type, str param_name) except *
    @staticmethod
    cdef void type_or_none(object argument, object expected_type, str param_name) except *
    @staticmethod
    cdef void list_type(list list, type expected_type, str list_name) except *
    @staticmethod
    cdef void dict_types(dict dictionary, type key_type, type value_type, str dictionary_name) except *
    @staticmethod
    cdef void is_in(object element, object collection, str element_name, str collection_name) except *
    @staticmethod
    cdef void not_in(object element, object collection, str element_name, str collection_name) except *
    @staticmethod
    cdef void not_empty(object collection, str collection_name) except *
    @staticmethod
    cdef void empty(object collection, str collection_name) except *
    @staticmethod
    cdef void equal_length(object collection1, object collection2, str collection1_name, str collection2_name) except *
    @staticmethod
    cdef void positive(float value, str param_name) except *
    @staticmethod
    cdef void not_negative(float value, str param_name) except *
    @staticmethod
    cdef void in_range(float value, str param_name, float start, float end) except *
