# -------------------------------------------------------------------------------------------------
# <copyright file="correctness.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

cdef class Condition:
    @staticmethod
    cdef true(bint predicate, str description)
    @staticmethod
    cdef none(object argument, str param_name: str)
    @staticmethod
    cdef not_none(object argument, str param_name)
    @staticmethod
    cdef type(object argument, object is_type, str param_name)
    @staticmethod
    cdef type_or_none(object argument, object is_type, str param_name)
    @staticmethod
    cdef list_type(list argument, type type_to_contain, str param_name)
    @staticmethod
    cdef dict_types(dict argument, type key_type, type value_type, str param_name)
    @staticmethod
    cdef is_in(object item, list collection, str param_name, str collection_name)
    @staticmethod
    cdef not_in(object item, list collection, str param_name, str collection_name)
    @staticmethod
    cdef key_is_in(object key, dict dictionary, str param_name, str dict_name)
    @staticmethod
    cdef key_not_in(object key, dict dictionary, str param_name, str dict_name)
    @staticmethod
    cdef valid_string(unicode argument, str param_name)
    @staticmethod
    cdef equal(object argument1, object argument2)
    @staticmethod
    cdef lists_equal_length(list list1, list list2, str list1_name, str list2_name)
    @staticmethod
    cdef dicts_equal_length(dict dict1, dict dict2, str dict1_name, str dict2_name)
    @staticmethod
    cdef positive(float value, str param_name)
    @staticmethod
    cdef not_negative(float value, str param_name)
    @staticmethod
    cdef in_range(
            float value,
            str param_name,
            float start,
            float end)
    @staticmethod
    cdef not_empty(object argument, str param_name)
    @staticmethod
    cdef empty(object argument, str param_name)
