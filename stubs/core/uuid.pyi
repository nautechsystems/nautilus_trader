from typing import Any

class UUID4:
    """
    Represents a Universally Unique Identifier (UUID)
    version 4 based on a 128-bit label as specified in RFC 4122.

    References
    ----------
    https://en.wikipedia.org/wiki/Universally_unique_identifier
    """

    def __init__(self) -> None: ...
    def __getstate__(self) -> Any: ...
    def __setstate__(self, state: Any) -> None: ...
    def __eq__(self, other: UUID4) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def value(self) -> str: ...
    @staticmethod
    def from_str(value: str) -> UUID4:
        """
        Create a new UUID4 from the given string value.

        Parameters
        ----------
        value : str
            The UUID value.

        Returns
        -------
        UUID4

        Raises
        ------
        ValueError
            If `value` is not a valid UUID version 4 RFC 4122 string.

        """
        ...