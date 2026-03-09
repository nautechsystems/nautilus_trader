from __future__ import annotations

from nautilus_trader.serialization.base import register_serializable_type


class TestObject:
    """
    Represents some generic user object which implements serialization value dicts.
    """

    __test__ = False  # Prevents pytest from collecting this as a test class

    def __init__(self, value):
        self.value = value

    @staticmethod
    def from_dict(values: dict) -> TestObject:
        return TestObject(values["value"])

    @staticmethod
    def to_dict(obj):
        return {"value": obj.value}


class TestSerializationBase:
    def test_register_serializable_type(self):
        # Arrange, Act, Assert
        register_serializable_type(
            cls=TestObject,
            to_dict=TestObject.to_dict,
            from_dict=TestObject.from_dict,
        )

        # Does not raise exception
