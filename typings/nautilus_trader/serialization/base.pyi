from typing import Any, Callable, Dict, Set, Type

# Global type maps
_OBJECT_TO_DICT_MAP: Dict[str, Callable[[Any], Dict[str, Any]]]
_OBJECT_FROM_DICT_MAP: Dict[str, Callable[[Dict[str, Any]], Any]]
_EXTERNAL_PUBLISHABLE_TYPES: Set[Type]

def register_serializable_type(
    cls: Type,
    to_dict: Callable[[Any], Dict[str, Any]],
    from_dict: Callable[[Dict[str, Any]], Any],
) -> None: ...

class Serializer:
    def __init__(self) -> None: ...
    def serialize(self, obj: Any) -> bytes: ...
    def deserialize(self, obj_bytes: bytes) -> Any: ...
