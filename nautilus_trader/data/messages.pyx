# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument


cdef class DataCommand(Command):
    """
    The base class for all data commands.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    data_type : type
        The data type for the command.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the command.

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
        UUID4 command_id not None,
        DataType data_type not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        Condition.is_true(client_id or venue, "Both `client_id` and `venue` were None")
        super().__init__(command_id, ts_init)

        self.data_type = data_type
        self.client_id = client_id
        self.venue = venue
        self.params = params or {}

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}{form_params_str(self.params)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeData(DataCommand):
    """
    Represents a command to subscribe to data.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    data_type : type
        The data type for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        DataType data_type not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            data_type,
            client_id,
            venue,
            ts_init,
            params,
        )


cdef class SubscribeInstruments(SubscribeData):
    """
    Represents a command to subscribe to all instruments of a venue.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(Instrument),
            client_id,
            venue,
            ts_init,
            params,
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeInstrument(SubscribeData):
    """
    Represents a command to subscribe to an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(Instrument),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeOrderBook(SubscribeData):
    """
    Represents a command to subscribe to order book deltas of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    book_type : BookType
        The order book type.
    depth : int
        The maximum depth for the subscription.
    managed: bool
        If an order book should be managed by the data engine based on the subscribed feed.
    interval_ms : int
        The interval (milliseconds) between snapshots.
    only_deltas : bool
        If the subscription is for OrderBookDeltas or OrderBook snapshots.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).
    ValueError
        If `interval_ms` is not positive (> 0).
    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        BookType book_type,
        int depth = 0,
        bint managed = True,
        int interval_ms = 1000,
        bint only_deltas = True,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        Condition.positive_int(interval_ms, "interval_ms")
        super().__init__(
            command_id,
            DataType(OrderBookDelta) if only_deltas else DataType(OrderBook),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id
        self.book_type = book_type
        self.depth = depth
        self.managed = managed
        self.interval_ms = interval_ms
        self.only_deltas = only_deltas

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"book_type={self.book_type}, "
            f"depth={self.depth}, "
            f"managed={self.managed}, "
            f"interval_ms={self.interval_ms}, "
            f"only_deltas={self.only_deltas}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"book_type={self.book_type}, "
            f"depth={self.depth}, "
            f"managed={self.managed}, "
            f"interval_ms={self.interval_ms}, "
            f"only_deltas={self.only_deltas}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeQuoteTicks(SubscribeData):
    """
    Represents a command to subscribe to quote ticks.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(QuoteTick),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeTradeTicks(SubscribeData):
    """
    Represents a command to subscribe to trade ticks.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(TradeTick),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeBars(SubscribeData):
    """
    Represents a command to subscribe to bars of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    bar_type : BarType
        The bar type for the subscription.
    await_partial : bool
        If the bar aggregator should await the arrival of a historical partial bar prior to actively aggregating new bars.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        BarType bar_type not None,
        bint await_partial = False,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(Bar),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.bar_type = bar_type
        self.await_partial = await_partial

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.bar_type}, "
            f"await_partial={self.await_partial}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.bar_type}, "
            f"await_partial={self.await_partial}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeInstrumentStatus(SubscribeData):
    """
    Represents a command to subscribe to the status of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(InstrumentStatus),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class SubscribeInstrumentClose(SubscribeData):
    """
    Represents a command to subscribe to the close of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(InstrumentClose),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class UnsubscribeData(DataCommand):
    """
    Represents a command to unsubscribe to data.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    data_type : type
        The data type for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        DataType data_type not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            data_type,
            client_id,
            venue,
            ts_init,
            params,
        )


cdef class UnsubscribeInstruments(UnsubscribeData):
    """
    Represents a command to unsubscribe to all instruments.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(Instrument),
            client_id,
            venue,
            ts_init,
            params,
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id})"
        )


cdef class UnsubscribeInstrument(UnsubscribeData):
    """
    Represents a command to unsubscribe to an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(Instrument),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

        def __str__(self) -> str:
            return (
                f"{type(self).__name__}("
                f"instrument_id={self.instrument_id}, "
                f"client_id={self.client_id}, "
                f"venue={self.venue})"
            )

        def __repr__(self) -> str:
            return (
                f"{type(self).__name__}("
                f"instrument_id={self.instrument_id}, "
                f"client_id={self.client_id}, "
                f"venue={self.venue}, "
                f"id={self.id}{form_params_str(self.params)})"
            )


cdef class UnsubscribeOrderBook(UnsubscribeData):
    """
    Represents a command to unsubscribe to order book updates of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    only_deltas: bool
        If the subscription is for OrderBookDeltas or OrderBook snapshots.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        only_deltas: bool,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(OrderBookDelta) if only_deltas else DataType(OrderBook),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id
        self.only_deltas = only_deltas

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"only_deltas={self.only_deltas}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"only_deltas={self.only_deltas}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class UnsubscribeQuoteTicks(UnsubscribeData):
    """
    Represents a command to unsubscribe to quote ticks of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(QuoteTick),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class UnsubscribeTradeTicks(UnsubscribeData):
    """
    Represents a command to unsubscribe to trade ticks of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(TradeTick),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class UnsubscribeBars(UnsubscribeData):
    """
    Represents a command to unsubscribe to bars of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    bar_type : BarType
        The bar type for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        BarType bar_type not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(Bar),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.bar_type = bar_type

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.bar_type}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.bar_type}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class UnsubscribeInstrumentStatus(UnsubscribeData):
    """
    Represents a command to unsubscribe to instrument status of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(InstrumentStatus),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class UnsubscribeInstrumentClose(UnsubscribeData):
    """
    Represents a command to unsubscribe to instrument close of an instrument.

    Parameters
    ----------
    command_id : UUID4
        The command ID.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        UUID4 command_id not None,
        InstrumentId instrument_id not None,
        ClientId client_id: ClientId | None = None,
        Venue venue: Venue | None = None,
        uint64_t ts_init = 0,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            command_id,
            DataType(InstrumentClose),
            client_id,
            venue,
            ts_init,
            params,
        )
        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class RequestData(Request):
    """
    Represents a request for data.

    Parameters
    ----------
    data_type : type
        The data type for the request.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    limit : int
        The limit on the amount of data to return for the request.
    client_id : ClientId or ``None``
        The data client ID for the request.
    venue : Venue or ``None``
        The venue for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object]
        Additional parameters for the request.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        DataType data_type not None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        int limit,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None,
    ) -> None:
        Condition.is_true(client_id or venue, "Both `client_id` and `venue` were None")
        super().__init__(
            callback,
            request_id,
            ts_init,
        )

        self.data_type = data_type
        self.start = start
        self.end = end
        self.limit = limit
        self.client_id = client_id
        self.venue = venue
        self.params = params or {}

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"data_type={self.data_type}{form_params_str(self.params)}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"data_type={self.data_type}{form_params_str(self.params)}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"callback={self.callback}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class RequestInstrument(RequestData):
    """
    Represents a request for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the request.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    client_id : ClientId or ``None``
        The data client ID for the request.
    venue : Venue or ``None``
        The venue for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object]
        Additional parameters for the request.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None,
    ) -> None:
        super().__init__(
            DataType(Instrument),
            start,
            end,
            0,
            client_id,
            venue,
            callback,
            request_id,
            ts_init,
            params
        )

        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}{form_params_str(self.params)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"callback={self.callback}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class RequestInstruments(RequestData):
    """
    Represents a request for instruments.

    Parameters
    ----------
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    client_id : ClientId or ``None``
        The data client ID for the request.
    venue : Venue or ``None``
        The venue for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object]
        Additional parameters for the request.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        datetime start : datetime | None,
        datetime end : datetime | None,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None,
    ) -> None:
        super().__init__(
            DataType(Instrument),
            start,
            end,
            0,
            client_id,
            venue,
            callback,
            request_id,
            ts_init,
            params
        )

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"start={self.start}, "
            f"end={self.end}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}{form_params_str(self.params)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"start={self.start}, "
            f"end={self.end}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"callback={self.callback}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class RequestOrderBookSnapshot(RequestData):
    """
    Represents a request for an order book snapshot.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the request.
    limit : int
        The limit on the depth of the order book snapshot (default is None).
    client_id : ClientId or ``None``
        The data client ID for the request.
    venue : Venue or ``None``
        The venue for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object]
        Additional parameters for the request.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int limit,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None,
    ) -> None:
        super().__init__(
            DataType(OrderBookDeltas),
            None,
            None,
            limit,
            client_id,
            venue,
            callback,
            request_id,
            ts_init,
            params
        )

        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}{form_params_str(self.params)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"callback={self.callback}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class RequestQuoteTicks(RequestData):
    """
    Represents a request for quote ticks.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the request.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    limit : int
        The limit on the amount of quote ticks received.
    client_id : ClientId or ``None``
        The data client ID for the request.
    venue : Venue or ``None``
        The venue for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object]
        Additional parameters for the request.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        int limit,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None,
    ) -> None:
        super().__init__(
            DataType(QuoteTick),
            start,
            end,
            limit,
            client_id,
            venue,
            callback,
            request_id,
            ts_init,
            params
        )

        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}{form_params_str(self.params)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"callback={self.callback}, "
            f"id={self.id}{form_params_str(self.params)})"
        )

cdef class RequestTradeTicks(RequestData):
    """
    Represents a request for trade ticks.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the request.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    limit : int
        The limit on the amount of trade ticks received.
    client_id : ClientId or ``None``
        The data client ID for the request.
    venue : Venue or ``None``
        The venue for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object]
        Additional parameters for the request.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        int limit,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None,
    ) -> None:
        super().__init__(
            DataType(TradeTick),
            start,
            end,
            limit,
            client_id,
            venue,
            callback,
            request_id,
            ts_init,
            params
        )

        self.instrument_id = instrument_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}{form_params_str(self.params)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"callback={self.callback}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class RequestBars(RequestData):
    """
    Represents a request for bars.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the request.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    limit : int
        The limit on the amount of bars received.
    client_id : ClientId or ``None``
        The data client ID for the request.
    venue : Venue or ``None``
        The venue for the request.
    callback : Callable[[Any], None]
        The delegate to call with the data.
    request_id : UUID4
        The request ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object]
        Additional parameters for the request.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    def __init__(
        self,
        BarType bar_type not None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        int limit,
        ClientId client_id: ClientId | None,
        Venue venue: Venue | None,
        callback not None: Callable[[Any], None],
        UUID4 request_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None,
    ) -> None:
        super().__init__(
            DataType(Bar),
            start,
            end,
            limit,
            client_id,
            venue,
            callback,
            request_id,
            ts_init,
            params
        )

        self.bar_type = bar_type

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.bar_type}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}{form_params_str(self.params)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.bar_type}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"limit={self.limit}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"callback={self.callback}, "
            f"id={self.id}{form_params_str(self.params)})"
        )


cdef class DataResponse(Response):
    """
    Represents a response with data.

    Parameters
    ----------
    client_id : ClientId or ``None``
        The data client ID of the response.
    venue : Venue or ``None``
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
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the response.

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
            dict[str, object] params: dict | None = None,
    ) -> None:
        Condition.is_true(client_id or venue, "Both `client_id` and `venue` were None")
        super().__init__(
            correlation_id,
            response_id,
            ts_init,
        )

        self.client_id = client_id
        self.venue = venue
        self.data_type = data_type
        self.data = data
        self.params = params or {}

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"data_type={self.data_type}, "
            f"correlation_id={self.correlation_id}, "
            f"id={self.id}{form_params_str(self.params)})"
        )
