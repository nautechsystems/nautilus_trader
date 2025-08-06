from collections.abc import Callable

from stubs.serialization.base import Serializer

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

    timestamps_as_str: bool
    timestamps_as_iso8601: bool

    def __init__(
        self,
        encoding: Callable,
        timestamps_as_str: bool = False,
        timestamps_as_iso8601: bool = False,
    ) -> None: ...
    def serialize(self, obj: object) -> bytes:
        """
        Serialize the given object to `MessagePack` specification bytes.

        Parameters
        ----------
        obj : object
            The object to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If `obj` cannot be serialized.

        """
        ...
    def deserialize(self, obj_bytes: bytes) -> object:
        """
        Deserialize the given `MessagePack` specification bytes to an object.

        Parameters
        ----------
        obj_bytes : bytes
            The object bytes to deserialize.

        Returns
        -------
        Instrument

        Raises
        ------
        RuntimeError
            If `obj_bytes` cannot be deserialized.

        """
        ...
