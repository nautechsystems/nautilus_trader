from collections.abc import Callable
from datetime import datetime
from typing import Any

from stubs.core.message import Command
from stubs.core.message import Request
from stubs.core.message import Response

class DataCommand(Command):
    """
    The base class for all data commands.

    Parameters
    ----------
    data_type : type
        The data type for the command.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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

    data_type: DataType
    client_id: ClientId | None
    venue: Venue | None
    params: dict[str, Any] | None

    def __init__(
        self,
        data_type: DataType,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class SubscribeData(DataCommand):
    """
    Represents a command to subscribe to data.

    Parameters
    ----------
    data_type : type
        The data type for the subscription.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    instrument_id: InstrumentId | None

    def __init__(
        self,
        data_type: DataType,
        instrument_id: InstrumentId | None,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def to_request(
        self,
        start: datetime | None,
        end: datetime | None,
        callback: Callable[[Any], None],
    ) -> RequestData:
        """
        Convert this subscribe message to a request message.

        Parameters
        ----------
        start : datetime
            The start datetime (UTC) of request time range (inclusive).
        end : datetime
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        callback : Callable[[Any], None]
            The delegate to call with the data.

        Returns
        -------
        RequestQuoteTicks
            The converted request message.
        """
        ...

class SubscribeInstruments(SubscribeData):
    """
    Represents a command to subscribe to all instruments of a venue.

    Parameters
    ----------
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def to_request(
        self,
        start: datetime | None,
        end: datetime | None,
        callback: Callable[[Any], None],
    ) -> RequestInstruments:
        """
        Convert this subscribe message to a request message.

        Parameters
        ----------
        start : datetime
            The start datetime (UTC) of request time range (inclusive).
        end : datetime
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        callback : Callable[[Any], None]
            The delegate to call with the data.

        Returns
        -------
        RequestInstruments
            The converted request message.
        """
        ...

class SubscribeInstrument(SubscribeData):
    """
    Represents a command to subscribe to an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class SubscribeOrderBook(SubscribeData):
    """
    Represents a command to subscribe to order book deltas for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    book_type : BookType
        The order book type.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    depth : int, optional, default 0
        The maximum depth for the subscription.
    managed: bool, optional, default True
        If an order book should be managed by the data engine based on the subscribed feed.
    interval_ms : int, optional, default 1000
        The interval (milliseconds) between snapshots.
    only_deltas : bool, optional, default True
        If the subscription is for OrderBookDeltas or OrderBook snapshots.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).
    ValueError
        If `interval_ms` is not positive (> 0).
    """

    book_type: BookType
    depth: int
    managed: bool
    interval_ms: int
    only_deltas: bool

    def __init__(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        depth: int = 0,
        managed: bool = True,
        interval_ms: int = 1000,
        only_deltas: bool = True,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class SubscribeQuoteTicks(SubscribeData):
    """
    Represents a command to subscribe to quote ticks.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def to_request(
        self,
        start: datetime | None,
        end: datetime | None,
        callback: Callable[[Any], None],
    ) -> RequestQuoteTicks:
        """
        Convert this subscribe message to a request message.

        Parameters
        ----------
        start : datetime
            The start datetime (UTC) of request time range (inclusive).
        end : datetime
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        callback : Callable[[Any], None]
            The delegate to call with the data.

        Returns
        -------
        RequestQuoteTicks
            The converted request message.
        """
        ...

class SubscribeTradeTicks(SubscribeData):
    """
    Represents a command to subscribe to trade ticks.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def to_request(
        self,
        start: datetime | None,
        end: datetime | None,
        callback: Callable[[Any], None],
    ) -> RequestTradeTicks:
        """
        Convert this subscribe message to a request message.

        Parameters
        ----------
        start : datetime
            The start datetime (UTC) of request time range (inclusive).
        end : datetime
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        callback : Callable[[Any], None]
            The delegate to call with the data.

        Returns
        -------
        RequestTradeTicks
            The converted request message.
        """
        ...

class SubscribeMarkPrices(SubscribeData):
    """
    Represents a command to subscribe to mark prices.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class SubscribeIndexPrices(SubscribeData):
    """
    Represents a command to subscribe to index prices.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class SubscribeBars(SubscribeData):
    """
    Represents a command to subscribe to bars for an instrument.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    await_partial : bool
        If the bar aggregator should await the arrival of a historical partial bar prior to actively aggregating new bars.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    bar_type: BarType
    await_partial: bool

    def __init__(
        self,
        bar_type: BarType,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        await_partial: bool = False,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def to_request(
        self,
        start: datetime | None,
        end: datetime | None,
        callback: Callable[[Any], None],
    ) -> RequestBars:
        """
        Convert this subscribe message to a request message.

        Parameters
        ----------
        start : datetime
            The start datetime (UTC) of request time range (inclusive).
        end : datetime
            The end datetime (UTC) of request time range.
            The inclusiveness depends on individual data client implementation.
        callback : Callable[[Any], None]
            The delegate to call with the data.

        Returns
        -------
        RequestBars
            The converted request message.
        """
        ...

class SubscribeInstrumentStatus(SubscribeData):
    """
    Represents a command to subscribe to the status of an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class SubscribeInstrumentClose(SubscribeData):
    """
    Represents a command to subscribe to the close of an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeData(DataCommand):
    """
    Represents a command to unsubscribe to data.

    Parameters
    ----------
    data_type : type
        The data type for the subscription.
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        data_type: DataType,
        instrument_id: InstrumentId | None,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class UnsubscribeInstruments(UnsubscribeData):
    """
    Represents a command to unsubscribe to all instruments.

    Parameters
    ----------
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeInstrument(UnsubscribeData):
    """
    Represents a command to unsubscribe to an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeOrderBook(UnsubscribeData):
    """
    Represents a command to unsubscribe from order book updates for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    only_deltas: bool
        If the subscription is for OrderBookDeltas or OrderBook snapshots.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    only_deltas: bool

    def __init__(
        self,
        instrument_id: InstrumentId,
        only_deltas: bool,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeQuoteTicks(UnsubscribeData):
    """
    Represents a command to unsubscribe from quote ticks for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeTradeTicks(UnsubscribeData):
    """
    Represents a command to unsubscribe from trade ticks for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeMarkPrices(UnsubscribeData):
    """
    Represents a command to unsubscribe from mark prices for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeIndexPrices(UnsubscribeData):
    """
    Represents a command to unsubscribe from index prices for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeBars(UnsubscribeData):
    """
    Represents a command to unsubscribe from bars for an instrument.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the subscription.

    Raises
    ------
    ValueError
        If both `client_id` and `venue` are both ``None`` (not enough routing info).

    """

    bar_type: BarType

    def __init__(
        self,
        bar_type: BarType,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeInstrumentStatus(UnsubscribeData):
    """
    Represents a command to unsubscribe from instrument status.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class UnsubscribeInstrumentClose(UnsubscribeData):
    """
    Represents a command to unsubscribe from instrument close for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the subscription.
    client_id : ClientId or ``None``
        The data client ID for the command.
    venue : Venue or ``None``
        The venue for the command.
    command_id : UUID4
        The command ID.
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
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class RequestData(Request):
    """
    Represents a request for data.

    Parameters
    ----------
    data_type : type
        The data type for the request.
    instrument_id : InstrumentId
        The instrument ID for the request.
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

    data_type: DataType
    instrument_id: InstrumentId | None
    start: datetime | None
    end: datetime | None
    limit: int
    client_id: ClientId | None
    venue: Venue | None
    params: dict[str, Any] | None

    def __init__(
        self,
        data_type: DataType,
        instrument_id: InstrumentId | None,
        start: datetime | None,
        end: datetime | None,
        limit: int,
        client_id: ClientId | None,
        venue: Venue | None,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None,
    ) -> None: ...
    def with_dates(self, start: datetime, end: datetime, ts_init: int): ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class RequestInstrument(RequestData):
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
        instrument_id: InstrumentId,
        start: datetime | None,
        end: datetime | None,
        client_id: ClientId | None,
        venue: Venue | None,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class RequestInstruments(RequestData):
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
        start: datetime | None,
        end: datetime | None,
        client_id: ClientId | None,
        venue: Venue | None,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None,
    ) -> None: ...
    def with_dates(self, start: datetime, end: datetime, ts_init: int): ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class RequestOrderBookSnapshot(RequestData):
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
        instrument_id: InstrumentId,
        limit: int,
        client_id: ClientId | None,
        venue: Venue | None,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class RequestQuoteTicks(RequestData):
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
        instrument_id: InstrumentId,
        start: datetime | None,
        end: datetime | None,
        limit: int,
        client_id: ClientId | None,
        venue: Venue | None,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None,
    ) -> None: ...
    def with_dates(self, start: datetime, end: datetime, ts_init: int): ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class RequestTradeTicks(RequestData):
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
        instrument_id: InstrumentId,
        start: datetime | None,
        end: datetime | None,
        limit: int,
        client_id: ClientId | None,
        venue: Venue | None,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None,
    ) -> None: ...
    def with_dates(self, start: datetime, end: datetime, ts_init: int): ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...


class RequestBars(RequestData):
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
        bar_type: BarType,
        start: datetime | None,
        end: datetime | None,
        limit: int,
        client_id: ClientId | None,
        venue: Venue | None,
        callback: Callable[[Any], None],
        request_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None,
    ) -> None: ...
    def with_dates(self, start: datetime, end: datetime, ts_init: int): ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class DataResponse(Response):
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

    client_id: ClientId | None
    venue: Venue | None
    data_type: DataType
    data: Any
    params: dict[str, Any] | None

    def __init__(
            self,
            client_id: ClientId | None,
            venue: Venue | None,
            data_type: DataType,
            data: Any,
            correlation_id: UUID4,
            response_id: UUID4,
            ts_init: int,
            params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

