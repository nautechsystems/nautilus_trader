"""
The `serialization` subpackage groups all serialization components and serializer
implementations.

Base classes are defined which can allow for other serialization implementations beside
the built-in specification serializers.

"""

from nautilus_trader.serialization.base import register_serializable_type


__all__ = ["register_serializable_type"]
