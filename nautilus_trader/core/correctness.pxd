# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------


cdef class Condition:

    @staticmethod
    cdef void true(bint predicate, str description, ex_type=*) except *

    @staticmethod
    cdef void false(bint predicate, str description, ex_type=*) except *

    @staticmethod
    cdef void none(object argument, str param, ex_type=*) except *

    @staticmethod
    cdef void not_none(object argument, str param, ex_type=*) except *

    @staticmethod
    cdef void type(
        object argument,
        object expected,
        str param,
        ex_type=*) except *

    @staticmethod
    cdef void type_or_none(
        object argument,
        object expected,
        str param,
        ex_type=*) except *

    @staticmethod
    cdef void callable(object argument, str param, ex_type=*) except *

    @staticmethod
    cdef void callable_or_none(object argument, str param, ex_type=*) except *

    @staticmethod
    cdef void equal(
        object argument1,
        object argument2,
        str param1,
        str param2,
        ex_type=*) except *

    @staticmethod
    cdef void not_equal(
        object argument1,
        object argument2,
        str param1,
        str param2,
        ex_type=*) except *

    @staticmethod
    cdef void list_type(
        list argument,
        type expected_type,
        str param,
        ex_type=*) except *

    @staticmethod
    cdef void dict_types(
        dict argument,
        type key_type,
        type value_type,
        str param,
        ex_type=*) except *

    @staticmethod
    cdef void is_in(
        object element,
        object collection,
        str param1,
        str param2,
        ex_type=*) except *

    @staticmethod
    cdef void not_in(
        object element,
        object collection,
        str param1,
        str param2,
        ex_type=*) except *

    @staticmethod
    cdef void empty(object collection, str param, ex_type=*) except *

    @staticmethod
    cdef void not_empty(object collection, str param, ex_type=*) except *

    @staticmethod
    cdef void positive(double value, str param, ex_type=*) except *

    @staticmethod
    cdef void positive_int(int value, str param, ex_type=*) except *

    @staticmethod
    cdef void not_negative(double value, str param, ex_type=*) except *

    @staticmethod
    cdef void not_negative_int(int value, str param, ex_type=*) except *

    @staticmethod
    cdef void in_range(
        double value,
        double start,
        double end,
        str param,
        ex_type=*) except *

    @staticmethod
    cdef void in_range_int(
        int value,
        int start,
        int end,
        str param,
        ex_type=*) except *

    @staticmethod
    cdef void valid_string(str argument, str param, ex_type=*) except *

    @staticmethod
    cdef void valid_port(int value, str param, ex_type=*) except *
