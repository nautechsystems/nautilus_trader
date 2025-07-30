import re
import pandas as pd
import pytz
from typing import Any
from nautilus_trader.serialization.base import Serializer

class MsgSpecSerializer(Serializer):
    """
    Provides a serializer for either the 'MessagePack' or 'JSON' specifications.

    Parameters
    ----------
    encoding : Callable
        The msgspec encoding type.
    timestamps_as_str : bool, default False
        If the serializer converts `uint64_t` timestamps to integer strings on serialization,
        and back to `uint64_t` on deserialization.
    timestamps_as_iso8601 : bool, default False
        If the serializer converts `uint64_t` timestamps to ISO 8601 strings on serialization,
        and back to `uint64_t` on deserialization.
    """

    def __init__(
        self,
        encoding: Any,
        timestamps_as_str: bool = False,
        timestamps_as_iso8601: bool = False,
    ) -> None: ...
    def serialize(self, obj: object) -> bytes: ...
    def deserialize(self, obj_bytes: bytes) -> object: ...