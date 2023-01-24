# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from typing import Any, Callable

from nautilus_trader.core.uuid cimport UUID4


cdef class Command:
    """
    The base class for all command messages.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID4 command_id not None,
        uint64_t ts_init,
    ):
        self.id = command_id
        self.ts_init = ts_init

    def __eq__(self, Command other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id}, ts_init={self.ts_init})"


cdef class Document:
    """
    The base class for all document messages.

    Parameters
    ----------
    document_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID4 document_id not None,
        uint64_t ts_init,
    ):
        self.id = document_id
        self.ts_init = ts_init

    def __eq__(self, Document other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id}, ts_init={self.ts_init})"


cdef class Event:
    """
    The base class for all event messages.

    Parameters
    ----------
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        self.id = event_id
        self.ts_event = ts_event
        self.ts_init = ts_init

    def __eq__(self, Event other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id}, ts_event={self.ts_event}, ts_init={self.ts_init})"


cdef class Request:
    """
    The base class for all request messages.

    Parameters
    ----------
    callback : Callable[[Any], None]
        The delegate to call with the response.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
    ):
        self.callback = callback
        self.id = request_id
        self.ts_init = ts_init

    def __eq__(self, Request other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id}, callback={self.callback}, ts_init={self.ts_init})"


cdef class Response:
    """
    The base class for all response messages.

    Parameters
    ----------
    correlation_id : UUID4
        The correlation ID.
    response_id : UUID4
        The response ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        UUID4 correlation_id not None,
        UUID4 response_id not None,
        uint64_t ts_init,
    ):
        self.correlation_id = correlation_id
        self.id = response_id
        self.ts_init = ts_init

    def __eq__(self, Response other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"correlation_id={self.correlation_id}, "
            f"id={self.id}, "
            f"ts_init={self.ts_init})"
        )
