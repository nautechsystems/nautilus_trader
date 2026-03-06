
cdef inline Exception make_exception(ex_default, ex_type, str msg):
    if type(ex_type) is type(Exception):
        return ex_type(msg)
    else:
        return ex_default(msg)


cdef class Condition:

    @staticmethod
    cdef void is_true(bint predicate, str fail_msg, ex_type=*)

    @staticmethod
    cdef void is_false(bint predicate, str fail_msg, ex_type=*)

    @staticmethod
    cdef void none(object argument, str param, ex_type=*)

    @staticmethod
    cdef void not_none(object argument, str param, ex_type=*)

    @staticmethod
    cdef void type(
        object argument,
        object expected,
        str param,
        ex_type=*,
    )

    @staticmethod
    cdef void type_or_none(
        object argument,
        object expected,
        str param,
        ex_type=*,
    )

    @staticmethod
    cdef void callable(object argument, str param, ex_type=*)

    @staticmethod
    cdef void callable_or_none(object argument, str param, ex_type=*)

    @staticmethod
    cdef void equal(
        object argument1,
        object argument2,
        str param1,
        str param2,
        ex_type=*,
    )

    @staticmethod
    cdef void not_equal(
        object argument1,
        object argument2,
        str param1,
        str param2,
        ex_type=*,
    )

    @staticmethod
    cdef void list_type(
        list argument,
        type expected_type,
        str param,
        ex_type=*,
    )

    @staticmethod
    cdef void dict_types(
        dict argument,
        type key_type,
        type value_type,
        str param,
        ex_type=*,
    )

    @staticmethod
    cdef void is_in(
        object element,
        object collection,
        str param1,
        str param2,
        ex_type=*,
    )

    @staticmethod
    cdef void not_in(
        object element,
        object collection,
        str param1,
        str param2,
        ex_type=*,
    )

    @staticmethod
    cdef void empty(object collection, str param, ex_type=*)

    @staticmethod
    cdef void not_empty(object collection, str param, ex_type=*)

    @staticmethod
    cdef void positive(double value, str param, ex_type=*)

    @staticmethod
    cdef void positive_int(value: int, str param, ex_type=*)

    @staticmethod
    cdef void not_negative(double value, str param, ex_type=*)

    @staticmethod
    cdef void not_negative_int(value: int, str param, ex_type=*)

    @staticmethod
    cdef void in_range(
        double value,
        double start,
        double end,
        str param,
        ex_type=*,
    )

    @staticmethod
    cdef void in_range_int(
        value,
        start,
        end,
        str param,
        ex_type=*,
    )

    @staticmethod
    cdef void valid_string(str argument, str param, ex_type=*)
