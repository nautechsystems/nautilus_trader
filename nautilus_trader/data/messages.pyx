# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Any
from typing import Callable

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.data cimport DataType


cdef class DataCommand(Command):
    """
    The base class for all data commands.

    Parameters
    ----------
    client_id : ClientId, optional with no default so ``None`` must be passed explicitly
        The data client ID for the command.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The venue for the command.
    data_type : type
        The data type for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        DataType data_type not None,
        UUID4 command_id not None,
        uint64_t ts_init,
    ):
        Condition.true(client_id or venue, "Both `client_id` and `venue` were None")
        super().__init__(command_id, ts_init)

        self.client_id = client_id
        self.venue = venue
        self.data_type = data_type

    def __str__(self) -> str:
        return f"{type(self).__name__}({self.data_type})"

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}, "
            f"id={self.id})"
        )


cdef class Subscribe(DataCommand):
    """
    Represents a command to subscribe to data.

    Parameters
    ----------
    client_id : ClientId, optional with no default so ``None`` must be passed explicitly
        The data client ID for the command.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The venue for the command.
    data_type : type
        The data type for the subscription.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        DataType data_type not None,
        UUID4 command_id not None,
        uint64_t ts_init,
    ):
        super().__init__(
            client_id,
            venue,
            data_type,
            command_id,
            ts_init,
        )


cdef class Unsubscribe(DataCommand):
    """
    Represents a command to unsubscribe from data.

    Parameters
    ----------
    client_id : ClientId, optional with no default so ``None`` must be passed explicitly
        The data client ID for the command.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The venue for the command.
    data_type : type
        The data type to unsubscribe from.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        DataType data_type not None,
        UUID4 command_id not None,
        uint64_t ts_init,
    ):
        super().__init__(
            client_id,
            venue,
            data_type,
            command_id,
            ts_init,
        )


cdef class DataRequest(Request):
    """
    Represents a request for data.

    Parameters
    ----------
    client_id : ClientId, optional with no default so ``None`` must be passed explicitly
        The data client ID for the request.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The venue for the request.
    data_type : type
        The data type for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        DataType data_type not None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
    ):
        Condition.true(client_id or venue, "Both `client_id` and `venue` were None")
        super().__init__(
            callback,
            request_id,
            ts_init,
        )

        self.client_id = client_id
        self.venue = venue
        self.data_type = data_type

    def __str__(self) -> str:
        return f"{type(self).__name__}({self.data_type})"

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}, "
            f"callback={self.callback}, "
            f"id={self.id})"
        )


cdef class DataResponse(Response):
    """
    Represents a response with data.

    Parameters
    ----------
    client_id : ClientId, optional with no default so ``None`` must be passed explicitly
        The data client ID of the response.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The venue for the response.
    data_type : type
        The data type of the response.
    data : object
        The data of the response.
    correlation_id : UUID4
        The correlation ID.
    response_id : UUID4
        The response ID.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        DataType data_type,
        data not None,
        UUID4 correlation_id not None,
        UUID4 response_id not None,
        uint64_t ts_init,
    ):
        Condition.true(client_id or venue, "Both `client_id` and `venue` were None")
        super().__init__(
            correlation_id,
            response_id,
            ts_init,
        )

        self.client_id = client_id
        self.venue = venue
        self.data_type = data_type
        self.data = data

    def __str__(self) -> str:
        return f"{type(self).__name__}({self.data_type})"

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}, "
            f"correlation_id={self.correlation_id}, "
            f"id={self.id})"
        )
