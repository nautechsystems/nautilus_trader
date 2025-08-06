from collections.abc import Callable
from datetime import datetime
from typing import Any

from nautilus_trader.model.enums import BookType
from stubs.core.message import Command
from stubs.core.message import Request
from stubs.core.message import Response
from stubs.core.uuid import UUID4
from stubs.model.data import BarType
from stubs.model.data import DataType
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Venue

class DataCommand(Command):

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
    ) -> RequestData: ...

class SubscribeInstruments(SubscribeData):

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
    ) -> RequestInstruments: ...

class SubscribeInstrument(SubscribeData):

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
    ) -> RequestQuoteTicks: ...

class SubscribeTradeTicks(SubscribeData):

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
    ) -> RequestTradeTicks: ...

class SubscribeMarkPrices(SubscribeData):

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
    ) -> RequestBars: ...

class SubscribeInstrumentStatus(SubscribeData):

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

    def __init__(
        self,
        instrument_id: InstrumentId,
        client_id: ClientId | None,
        venue: Venue | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ... # skip-validate
    def __repr__(self) -> str: ...

class UnsubscribeOrderBook(UnsubscribeData):

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

