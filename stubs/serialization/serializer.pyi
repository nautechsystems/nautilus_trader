from collections.abc import Callable

from stubs.serialization.base import Serializer

class MsgSpecSerializer(Serializer):

    timestamps_as_str: bool
    timestamps_as_iso8601: bool

    def __init__(
        self,
        encoding: Callable,
        timestamps_as_str: bool = False,
        timestamps_as_iso8601: bool = False,
    ) -> None: ...
    def serialize(self, obj: object) -> bytes: ...
    def deserialize(self, obj_bytes: bytes) -> object: ...
