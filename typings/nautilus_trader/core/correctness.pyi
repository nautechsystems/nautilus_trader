from typing import (
    Any,
    Collection,
    Dict,
    List,
    Optional,
    Type,
    TypeVar,
    Union,
)

T = TypeVar("T")
K = TypeVar("K")
V = TypeVar("V")

class PyCondition:
    @staticmethod
    def is_true(
        predicate: bool, fail_msg: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def is_false(
        predicate: bool, fail_msg: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def none(
        argument: Any, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def not_none(
        argument: Any, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def type(
        argument: Any,
        expected: Union[Type[T], tuple[Type[T], ...]],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def type_or_none(
        argument: Optional[Any],
        expected: Union[Type[T], tuple[Type[T], ...]],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def callable(
        argument: Any, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def callable_or_none(
        argument: Optional[Any], param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def equal(
        argument1: Any,
        argument2: Any,
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def not_equal(
        argument1: Any,
        argument2: Any,
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def list_type(
        argument: List[T],
        expected_type: Type[T],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def dict_types(
        argument: Dict[K, V],
        key_type: Type[K],
        value_type: Type[V],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def is_in(
        element: T,
        collection: Collection[T],
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def not_in(
        element: T,
        collection: Collection[T],
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def not_empty(
        argument: Collection[Any], param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def empty(
        argument: Collection[Any], param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def positive(
        value: float, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def positive_int(
        value: int, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def not_negative(
        value: float, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def not_negative_int(
        value: int, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def in_range(
        value: float,
        start: float,
        end: float,
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def in_range_int(
        value: int,
        start: int,
        end: int,
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def valid_string(
        argument: str, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...

# The Condition class is the Cython implementation
class Condition:
    @staticmethod
    def is_true(
        predicate: bool, fail_msg: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def is_false(
        predicate: bool, fail_msg: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def none(
        argument: Any, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def not_none(
        argument: Any, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def type(
        argument: Any,
        expected: Union[Type[T], tuple[Type[T], ...]],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def type_or_none(
        argument: Optional[Any],
        expected: Union[Type[T], tuple[Type[T], ...]],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def callable(
        argument: Any, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def callable_or_none(
        argument: Optional[Any], param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def equal(
        argument1: Any,
        argument2: Any,
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def not_equal(
        argument1: Any,
        argument2: Any,
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def list_type(
        argument: List[T],
        expected_type: Type[T],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def dict_types(
        argument: Dict[K, V],
        key_type: Type[K],
        value_type: Type[V],
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def is_in(
        element: T,
        collection: Collection[T],
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def not_in(
        element: T,
        collection: Collection[T],
        param1: str,
        param2: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def not_empty(
        argument: Collection[Any], param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def empty(
        argument: Collection[Any], param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def positive(
        value: float, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def positive_int(
        value: int, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def not_negative(
        value: float, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def not_negative_int(
        value: int, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...
    @staticmethod
    def in_range(
        value: float,
        start: float,
        end: float,
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def in_range_int(
        value: int,
        start: int,
        end: int,
        param: str,
        ex_type: Optional[Type[Exception]] = None,
    ) -> None: ...
    @staticmethod
    def valid_string(
        argument: str, param: str, ex_type: Optional[Type[Exception]] = None
    ) -> None: ...

def make_exception(
    ex_default: Type[Exception], ex_type: Optional[Type[Exception]], msg: str
) -> Exception: ...
