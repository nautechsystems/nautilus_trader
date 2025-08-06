import builtins
from typing import Any

class Condition:

    @staticmethod
    def is_true(predicate: bool, fail_msg: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def is_false(predicate: bool, fail_msg: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def none(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_none(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def type(
        argument: Any,
        expected: Any,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def type_or_none(
        argument: Any,
        expected: Any,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def callable(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def callable_or_none(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def equal(
        argument1: Any,
        argument2: Any,
        param1: str,
        param2: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def not_equal(
        object1: Any,
        object2: Any,
        param1: str,
        param2: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def list_type(
        argument: list,
        expected_type: type,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def dict_types(
        argument: dict,
        key_type: type,
        value_type: type,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def is_in(
        element: Any,
        collection: Any,
        param1: str,
        param2: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def not_in(
        element: Any,
        collection: Any,
        param1: str,
        param2: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def not_empty(collection: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def empty(collection: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def positive(value: float, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def positive_int(value: int, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_negative(value: float, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_negative_int(value: int, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def in_range(
        value: float,
        start: float,
        end: float,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def in_range_int(
        value: int,
        start: int,
        end: int,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def valid_string(argument: str, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...


class PyCondition:

    @staticmethod
    def is_true(predicate: bool, fail_msg: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def is_false(predicate: bool, fail_msg: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def none(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_none(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def type(argument: Any, expected: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def type_or_none(argument: Any, expected: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def callable(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def callable_or_none(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def equal(argument1: Any, argument2: Any, param1: str, param2: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_equal(argument1: Any, argument2: Any, param1: str, param2: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def list_type(argument: list, expected_type: type, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def dict_types(argument: dict, key_type: type, value_type: type, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def is_in(element: Any, collection: Any, param1: str, param2: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_in(element: Any, collection: Any, param1: str, param2: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_empty(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def empty(argument: Any, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def positive(value: float, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def positive_int(value: int, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_negative(value: float, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def not_negative_int(value: int, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
    @staticmethod
    def in_range(
        value: float,
        start: float,
        end: float,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def in_range_int(
        value: int,
        start: int,
        end: int,
        param: str,
        ex_type: builtins.type[Exception] | None = None,
    ) -> None: ...
    @staticmethod
    def valid_string(argument: str, param: str, ex_type: builtins.type[Exception] | None = None) -> None: ...
