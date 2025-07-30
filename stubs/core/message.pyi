from collections.abc import Callable
from typing import Any

from nautilus_trader.core.nautilus_pyo3 import UUID4

class Command:
    """
    The base class for all command messages.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    id: UUID4
    ts_init: int

    def __init__(
        self,
        command_id: UUID4,
        ts_init: int,
    ) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state): ...
    def __eq__(self, other: Command) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...

class Document:
    """
    The base class for all document messages.

    Parameters
    ----------
    document_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    document_id: UUID4
    ts_init: int

    def __init__(
        self,
        document_id: UUID4,
        ts_init: int,
    ) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state): ...
    def __eq__(self, other: Document) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...


class Event:
    """
    The abstract base class for all event messages.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        ...
    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        ...


class Request:
    """
    The base class for all request messages.

    Parameters
    ----------
    callback : Callable[[Any], None]
        The delegate to call with the response.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    callback: Callable[[Any], None]
    id: UUID4
    ts_init: int

    def __init__(
        self,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
    ) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state): ...
    def __eq__(self, other: Request) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...


class Response:
    """
    The base class for all response messages.

    Parameters
    ----------
    correlation_id : UUID4
        The correlation ID.
    response_id : UUID4
        The response ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    correlation_id: UUID4
    id: UUID4
    ts_init: int

    def __init__(
        self,
        correlation_id: UUID4,
        response_id: UUID4,
        ts_init: int,
    ) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state): ...
    def __eq__(self, other: Response) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...