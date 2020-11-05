# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.identifiers cimport TraderId


cdef class KillSwitch(Command):
    """
    Represents a command to aggressively shutdown the platform.
    """

    def __init__(
            self,
            TraderId trader_id not None,
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `KillSwitch` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(command_id, command_timestamp)

        self.trader_id = trader_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"id={self.id}, "
                f"timestamp={self.timestamp})")


cdef class Connect(Command):
    """
    Represents a command for a service to connect.
    """

    def __init__(
            self,
            Venue venue,  # Can be None
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `Connect` class.

        Parameters
        ----------
        venue : Venue
            The venue to connect to. If None then command is to connect to all
            venues.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(command_id, command_timestamp)

        self.venue = venue

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"venue={self.venue}, "
                f"id={self.id}, "
                f"timestamp={self.timestamp})")


cdef class Disconnect(Command):
    """
    Represents a command for a service to disconnect.
    """

    def __init__(
            self,
            Venue venue,  # Can be None
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `Disconnect` class.

        Parameters
        ----------
        venue : Venue
            The venue to disconnect from. If None then command is to disconnect
            from all venues.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(command_id, command_timestamp)

        self.venue = venue

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"venue={self.venue}, "
                f"id={self.id}, "
                f"timestamp={self.timestamp})")


cdef class Subscribe(Command):
    """
    Represents a command to subscribe to data.
    """

    def __init__(
            self,
            type data_type not None,
            dict metadata not None,
            object handler not None,
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `Subscribe` class.

        Parameters
        ----------
        data_type : type
            The data type for the subscription.
        metadata : type
            The metadata for the subscription.
        handler : callable
            The handler for the subscription.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(
            command_id,
            command_timestamp,
        )

        self.data_type = data_type
        self.metadata = metadata
        self.handler = handler


cdef class Unsubscribe(Command):
    """
    Represents a command to unsubscribe from data.
    """

    def __init__(
            self,
            type data_type not None,
            dict metadata not None,
            object handler not None,
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `Unsubscribe` class.

        Parameters
        ----------
        data_type : type
            The data type to unsubscribe from.
        metadata : type
            The metadata of the subscription.
        handler : callable
            The handler for the subscription.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(
            command_id,
            command_timestamp,
        )

        self.data_type = data_type
        self.metadata = metadata
        self.handler = handler


cdef class DataRequest(Request):
    """
    Represents a request for data.
    """

    def __init__(
            self,
            type data_type not None,
            dict metadata not None,
            object callback not None,
            UUID request_id not None,
            datetime request_timestamp not None,
    ):
        """
        Initialize a new instance of the `DataRequest` class.

        Parameters
        ----------
        data_type : type
            The data type for the request.
        metadata : type
            The metadata for the request.
        callback : callable
            The callback to receive the data.
        request_id : UUID
            The request identifier.
        request_timestamp : datetime
            The request timestamp.

        """
        super().__init__(
            request_id,
            request_timestamp,
        )

        self.data_type = data_type
        self.metadata = metadata
        self.callback = callback


cdef class DataResponse(Response):
    """
    Represents a response with data.
    """

    def __init__(
            self,
            type data_type not None,
            dict metadata not None,
            list data not None,
            UUID correlation_id not None,
            UUID response_id not None,
            datetime response_timestamp not None,
    ):
        """
        Initialize a new instance of the `DataResponse` class.

        Parameters
        ----------
        data_type : type
            The data type of the response.
        metadata : dict
            The metadata of the response.
        data : list
            The data of the response.
        correlation_id : UUID
            The correlation identifier.
        response_id : UUID
            The response identifier.
        response_timestamp : datetime
            The response timestamp.

        """
        super().__init__(
            correlation_id,
            response_id,
            response_timestamp,
        )

        self.data_type = data_type
        self.metadata = metadata
        self.data = data
