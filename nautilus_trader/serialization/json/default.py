import decimal
from typing import Callable, Dict


class Default:
    """
    Serialization extensions for orjson.dumps.
    """

    registry: Dict = {}

    @classmethod
    def register_serializer(cls, type_: type, serializer: Callable):
        """Register a new type `type_` for serialization in orjson."""
        assert type_ not in cls.registry
        cls.registry[type_] = serializer

    @classmethod
    def serialize(cls, obj):
        """Serialize for types orjson.dumps can't understand."""
        if type(obj) in cls.registry:
            return cls.registry[type(obj)](obj)
        raise TypeError


Default.register_serializer(type_=decimal.Decimal, serializer=str)
