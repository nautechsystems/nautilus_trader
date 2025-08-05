from collections.abc import Callable
from typing import Any

_OBJECT_TO_DICT_MAP: dict[str, Callable[[Any], dict]] = ...
_OBJECT_FROM_DICT_MAP: dict[str, Callable[[dict], Any]] = ...
_EXTERNAL_PUBLISHABLE_TYPES: set = ...


def register_serializable_type(
    cls: type,
    to_dict: Callable[[Any], dict[str, Any]],
    from_dict: Callable[[dict[str, Any]], Any],
) -> None:
    """
    Register the given type with the global serialization type maps.

    The `type` will also be registered as an external publishable type and
    will be published externally on the message bus unless also added to
    the `MessageBusConfig.types_filter`.

    Parameters
    ----------
    cls : type
        The type to register.
    to_dict : Callable[[Any], dict[str, Any]]
        The delegate to instantiate a dict of primitive types from an object.
    from_dict : Callable[[dict[str, Any]], Any]
        The delegate to instantiate an object from a dict of primitive types.

    Raises
    ------
    TypeError
        If `to_dict` or `from_dict` are not of type `Callable`.
    KeyError
        If `type` already registered with the global type maps.

    """


class Serializer:
    """
    The base class for all serializers.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self) -> None:
        ...

    def serialize(self, obj: object) -> bytes:
        """Abstract method (implement in subclass)."""

    def deserialize(self, obj_bytes: bytes) -> object:
        """Abstract method (implement in subclass)."""

