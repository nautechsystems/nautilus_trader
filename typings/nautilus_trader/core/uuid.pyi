class UUID4:
    """
    Represents a pseudo-random UUID (universally unique identifier)
    version 4 based on a 128-bit label as specified in RFC 4122.

    Parameters
    ----------
    value : str, optional
        The UUID value. If ``None`` then a value will be generated.

    Warnings
    --------
    - Panics at runtime if `value` is not ``None`` and not a valid UUID.

    References
    ----------
    https://en.wikipedia.org/wiki/Universally_unique_identifier
    """
    def __init__(self, value: str | None = None) -> None: ...
    def __eq__(self, other: UUID4) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def value(self) -> str: ...
