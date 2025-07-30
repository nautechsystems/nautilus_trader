from typing import Any
from typing import Type


class Condition:
    """
    Provides checking of function or method conditions.

    A condition is a predicate which must be true just prior to the execution of
    some section of code - for correct behavior as per the design specification.

    If a check fails, then an Exception is thrown with a descriptive message.
    """

    @staticmethod
    def is_true(predicate: bool, fail_msg: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the condition predicate is True.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is False.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is False.

        """
        ...

    @staticmethod
    def is_false(predicate: bool, fail_msg: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the condition predicate is False.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is True.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is True.

        """
        ...

    @staticmethod
    def none(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the argument is ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None``.

        """
        ...

    @staticmethod
    def not_none(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the argument is not ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is ``None``.

        """
        ...

    @staticmethod
    def type(
        argument: Any,
        expected: Any,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the argument is of the specified type.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : type or tuple of types
            The expected type(s).
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `object` is not of the expected type.

        """
        ...

    @staticmethod
    def type_or_none(
        argument: Any,
        expected: Any,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the argument is of the specified type, or is ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : type or tuple of types
            The expected type(s) (if not ``None``).
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `object` is not ``None`` and not of the expected type.

        """
        ...

    @staticmethod
    def callable(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the object is of type `Callable`.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The object parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not of type `Callable`.

        """
        ...

    @staticmethod
    def callable_or_none(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the object is of type `Callable` or ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The object parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None`` and not of type `Callable`.

        """
        ...

    @staticmethod
    def equal(
        argument1: Any,
        argument2: Any,
        param1: str,
        param2: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the objects are equal.

        Parameters
        ----------
        argument1 : object
            The first object to check.
        argument2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are not equal.

        """
        ...

    @staticmethod
    def not_equal(
        object1: Any,
        object2: Any,
        param1: str,
        param2: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the objects are not equal.

        Parameters
        ----------
        object1 : object
            The first object to check.
        object2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are equal.

        """
        ...

    @staticmethod
    def list_type(
        argument: list,
        expected_type: type,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the list only contains types of the given expected type.

        Parameters
        ----------
        argument : list
            The list to check.
        expected_type : type
            The expected element type (if not empty).
        param : str
            The list parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
             If `argument` is not empty and contains a type other than `expected_type`.

        """
        ...

    @staticmethod
    def dict_types(
        argument: dict,
        key_type: type,
        value_type: type,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the dictionary only contains types of the given key and value types to contain.

        Parameters
        ----------
        argument : dict
            The dictionary to check.
        key_type : type
            The expected type of the keys (if not empty).
        value_type : type
            The expected type of the values (if not empty).
        param : str
            The dictionary parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not empty and contains a key type other than `key_type`.
            If `argument` is not empty and contains a value type other than `value_type`.

        """
        ...

    @staticmethod
    def is_in(
        element: Any,
        collection: Any,
        param1: str,
        param2: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the element is contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The elements parameter name.
        param2 : str
            The collections name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is not contained in the `collection`.

        """
        ...

    @staticmethod
    def not_in(
        element: Any,
        collection: Any,
        param1: str,
        param2: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the element is not contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The elements parameter name.
        param2 : str
            The collections name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is contained in the `collection`.

        """
        ...

    @staticmethod
    def not_empty(collection: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the collection is not empty.

        Parameters
        ----------
        collection : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is empty.

        """
        ...

    @staticmethod
    def empty(collection: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the collection is empty.

        Parameters
        ----------
        collection : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is not empty.

        """
        ...

    @staticmethod
    def positive(value: float, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the real number value is positive (> 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `value` is not positive (> 0).

        """
        ...

    @staticmethod
    def positive_int(value: int, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the integer value is a positive integer (> 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not positive (> 0).

        """
        ...

    @staticmethod
    def not_negative(value: float, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the real number value is not negative (< 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        ...

    @staticmethod
    def not_negative_int(value: int, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the integer value is not negative (< 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        ...

    @staticmethod
    def in_range(
        value: float,
        start: float,
        end: float,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the real number value is within the specified range (inclusive).

        This function accounts for potential floating-point precision issues by using a small
        epsilon value of 1e-15.

        Parameters
        ----------
        value : scalar
            The value to check.
        start : scalar
            The start of the range.
        end : scalar
            The end of the range.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        ...

    @staticmethod
    def in_range_int(
        value: int,
        start: int,
        end: int,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the integer value is within the specified range (inclusive).

        Parameters
        ----------
        value : int
            The value to check.
        start : int
            The start of the range.
        end : int
            The end of the range.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        ...

    @staticmethod
    def valid_string(argument: str, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the string argument is valid (not ``None``, empty or whitespace).

        Parameters
        ----------
        argument : str
            The string argument to check.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `argument` is ``None``, empty or whitespace.

        """
        ...


class PyCondition:
    """
    Provides checking of function or method conditions.

    A condition is a predicate which must be true just prior to the execution of
    some section of code - for correct behavior as per the design specification.

    If a check fails, then an Exception is thrown with a descriptive message.
    """

    @staticmethod
    def is_true(predicate: bool, fail_msg: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the condition predicate is True.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is False.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is False.

        """
        ...

    @staticmethod
    def is_false(predicate: bool, fail_msg: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the condition predicate is False.

        Parameters
        ----------
        predicate : bool
            The condition predicate to check.
        fail_msg : str
            The failure message when the predicate is True
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If `predicate` condition is True.

        """
        ...

    @staticmethod
    def none(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the argument is ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None``.

        """
        ...

    @staticmethod
    def not_none(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the argument is not ``None``.

        Parameters
        ----------
        argument : object
            The argument to check.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is ``None``.

        """
        ...

    @staticmethod
    def type(argument: Any, expected: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the argument is of the specified type.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : object
            The expected class type.
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not of the expected type.

        """
        ...

    @staticmethod
    def type_or_none(argument: Any, expected: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the argument is of the specified type, or is ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        expected : object
            The expected class type (if not ``None``).
        param : str
            The arguments parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None`` and not of the expected type.

        """
        ...

    @staticmethod
    def callable(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the object is of type `Callable`.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not of type `Callable`.

        """
        ...

    @staticmethod
    def callable_or_none(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the object is of type `Callable` or ``None``.

        Parameters
        ----------
        argument : object
            The object to check.
        param : str
            The objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not ``None`` and not of type `Callable`.

        """
        ...

    @staticmethod
    def equal(argument1: Any, argument2: Any, param1: str, param2: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the objects are equal.

        Parameters
        ----------
        argument1 : object
            The first object to check.
        argument2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are not equal.

        """
        ...

    @staticmethod
    def not_equal(argument1: Any, argument2: Any, param1: str, param2: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the objects are not equal.

        Parameters
        ----------
        argument1 : object
            The first object to check.
        argument2 : object
            The second object to check.
        param1 : str
            The first objects parameter name.
        param2 : str
            The second objects parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
            If objects are equal.

        """
        ...

    @staticmethod
    def list_type(argument: list, expected_type: type, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the list only contains types of the given expected type.

        Parameters
        ----------
        argument : list
            The list to check.
        expected_type : type
            The expected element type (if not empty).
        param : str
            The list parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
             If `argument` is not empty and contains a type other than `expected_type`.

        """
        ...

    @staticmethod
    def dict_types(argument: dict, key_type: type, value_type: type, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the dictionary only contains types of the given key and value types to contain.

        Parameters
        ----------
        argument : dict
            The dictionary to check.
        key_type : type
            The expected type of the keys (if not empty).
        value_type : type
            The expected type of the values (if not empty).
        param : str
            The dictionary parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        TypeError
            If `argument` is not empty and contains a key type other than `key_type`.
            If `argument` is not empty and contains a value type other than `value_type`.

        """
        ...

    @staticmethod
    def is_in(element: Any, collection: Any, param1: str, param2: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the element is contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The element parameter name.
        param2 : str
            The collection name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is not contained in the `collection`.

        """
        ...

    @staticmethod
    def not_in(element: Any, collection: Any, param1: str, param2: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the element is not contained within the specified collection.

        Parameters
        ----------
        element : object
            The element to check.
        collection : iterable
            The collection to check.
        param1 : str
            The elements parameter name.
        param2 : str
            The collections name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        KeyError
            If `element` is contained in the `collection`.

        """
        ...

    @staticmethod
    def not_empty(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the collection is not empty.

        Parameters
        ----------
        argument : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is empty.

        """
        ...

    @staticmethod
    def empty(argument: Any, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the collection is empty.

        Parameters
        ----------
        argument : iterable
            The collection to check.
        param : str
            The collection parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `collection` is not empty.

        """
        ...

    @staticmethod
    def positive(value: float, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the real number value is positive (> 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
             If `value` is not positive (> 0).

        """
        ...

    @staticmethod
    def positive_int(value: int, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the integer value is a positive integer (> 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not positive (> 0).

        """
        ...

    @staticmethod
    def not_negative(value: float, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the real number value is not negative (< 0).

        Parameters
        ----------
        value : scalar
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        ...

    @staticmethod
    def not_negative_int(value: int, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the integer value is not negative (< 0).

        Parameters
        ----------
        value : int
            The value to check.
        param : str
            The name of the values parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is negative (< 0).

        """
        ...

    @staticmethod
    def in_range(
        value: float,
        start: float,
        end: float,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the real number value is within the specified range (inclusive).

        This function accounts for potential floating-point precision issues by using a small
        epsilon value of 1e-15.

        Parameters
        ----------
        value : scalar
            The value to check.
        start : scalar
            The start of the range.
        end : scalar
            The end of the range.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        ...

    @staticmethod
    def in_range_int(
        value: int,
        start: int,
        end: int,
        param: str,
        ex_type: Type[Exception] | None = None,
    ) -> None:
        """
        Check the integer value is within the specified range (inclusive).

        Parameters
        ----------
        value : int
            The value to check.
        start : int
            The start of the range.
        end : int
            The end of the range.
        param : str
            The name of the value parameter.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `value` is not within the range (inclusive of the end points).

        """
        ...

    @staticmethod
    def valid_string(argument: str, param: str, ex_type: Type[Exception] | None = None) -> None:
        """
        Check the string argument is valid (not ``None``, empty or whitespace).

        Parameters
        ----------
        argument : str
            The string argument to check.
        param : str
            The argument parameter name.
        ex_type : Exception, optional
            The custom exception type to be raised on a failed check.

        Raises
        -------
        ValueError
              If `argument` is ``None``, empty or whitespace.

        """
        ...