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

from datetime import datetime as py_datetime
from datetime import timezone
from typing import Any

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.rust.common cimport LogLevel
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.functions cimport order_side_from_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker


cpdef datetime _ns_to_datetime(object value):
    if isinstance(value, datetime):
        return value
    if isinstance(value, (int, float)):
        return py_datetime.fromtimestamp(value / 1_000_000_000, tz=timezone.utc)
    return value


cdef class ExecutionReportCommand(Command):
    """
    The base class for all execution report commands.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the command.
    start : datetime, optional
        The start datetime (UTC) of request time range (inclusive).
    end : datetime, optional
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the command.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        InstrumentId instrument_id : InstrumentId | None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        UUID4 command_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(command_id, ts_init)

        self.instrument_id = instrument_id
        self.start = start
        self.end = end
        self.params = params or {}


cdef class GenerateOrderStatusReport(ExecutionReportCommand):
    """
    Command to generate an order status report.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the command.
    client_order_id : ClientOrderId
        The client order ID to update.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue) to query.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the command.
    """

    def __init__(
        self,
        InstrumentId instrument_id : InstrumentId | None,
        ClientOrderId client_order_id : ClientOrderId | None,
        VenueOrderId venue_order_id: VenueOrderId | None,
        UUID4 command_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            instrument_id,
            None,
            None,
            command_id,
            ts_init,
            params,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"client_order_id={self.client_order_id}, "
            f"venue_order_id={self.venue_order_id}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef GenerateOrderStatusReport from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str i = values["instrument_id"]
        cdef str c = values["client_order_id"]
        cdef str v = values["venue_order_id"]
        return GenerateOrderStatusReport(
            instrument_id=InstrumentId.from_str_c(i) if i is not None else None,
            client_order_id=ClientOrderId(c) if c is not None else None,
            venue_order_id=VenueOrderId(v) if v is not None else None,
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
            params=values.get("params"),
        )

    @staticmethod
    cdef dict to_dict_c(GenerateOrderStatusReport obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "GenerateOrderStatusReport",
            "instrument_id": obj.instrument_id.to_str() if obj.instrument_id is not None else None,
            "client_order_id": obj.client_order_id.to_str() if obj.client_order_id is not None else None,
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
            "params": obj.params,
        }

    @staticmethod
    def from_dict(dict values) -> GenerateOrderStatusReport:
        """
        Return a generate order status report command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        GenerateOrderStatusReport

        """
        return GenerateOrderStatusReport.from_dict_c(values)

    @staticmethod
    def to_dict(GenerateOrderStatusReport obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return GenerateOrderStatusReport.to_dict_c(obj)


cdef class GenerateOrderStatusReports(ExecutionReportCommand):
    """
    Command to generate order status reports.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the command.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    open_only : bool
        If True then only open orders will be requested.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the command.
    log_receipt_level : LogLevel, default 'INFO'
        The log level for logging received reports. Must be either `LogLevel.DEBUG` or `LogLevel.INFO`.
    """

    def __init__(
        self,
        InstrumentId instrument_id : InstrumentId | None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        bint open_only,
        UUID4 command_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None = None,
        LogLevel log_receipt_level = LogLevel.INFO,
    ) -> None:
        super().__init__(
            instrument_id,
            start,
            end,
            command_id,
            ts_init,
            params,
        )

        self.open_only = open_only
        self.log_receipt_level = log_receipt_level

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"open_only={self.open_only}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef GenerateOrderStatusReports from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str i = values["instrument_id"]
        return GenerateOrderStatusReports(
            instrument_id=InstrumentId.from_str_c(i) if i is not None else None,
            start=_ns_to_datetime(values["start"]),
            end=_ns_to_datetime(values["end"]),
            open_only=values["open_only"],
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
            params=values.get("params"),
        )

    @staticmethod
    cdef dict to_dict_c(GenerateOrderStatusReports obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "GenerateOrderStatusReports",
            "instrument_id": obj.instrument_id.to_str() if obj.instrument_id is not None else None,
            "start": obj.start,
            "end": obj.end,
            "open_only": obj.open_only,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
            "params": obj.params,
        }

    @staticmethod
    def from_dict(dict values) -> GenerateOrderStatusReports:
        """
        Return a generate order status reports command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        GenerateOrderStatusReports

        """
        return GenerateOrderStatusReports.from_dict_c(values)

    @staticmethod
    def to_dict(GenerateOrderStatusReports obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return GenerateOrderStatusReports.to_dict_c(obj)


cdef class GenerateFillReports(ExecutionReportCommand):
    """
    Command to generate fill reports.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the command.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue) to query.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the command.
    """

    def __init__(
        self,
        InstrumentId instrument_id : InstrumentId | None,
        VenueOrderId venue_order_id: VenueOrderId | None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        UUID4 command_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            instrument_id,
            start,
            end,
            command_id,
            ts_init,
            params,
        )

        self.venue_order_id = venue_order_id

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"venue_order_id={self.venue_order_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef GenerateFillReports from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str i = values["instrument_id"]
        cdef str v = values["venue_order_id"]
        return GenerateFillReports(
            instrument_id=InstrumentId.from_str_c(i) if i is not None else None,
            venue_order_id=VenueOrderId(v) if v is not None else None,
            start=_ns_to_datetime(values["start"]),
            end=_ns_to_datetime(values["end"]),
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
            params=values.get("params"),
        )

    @staticmethod
    cdef dict to_dict_c(GenerateFillReports obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "GenerateFillReports",
            "instrument_id": obj.instrument_id.to_str() if obj.instrument_id is not None else None,
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "start": obj.start,
            "end": obj.end,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
            "params": obj.params,
        }

    @staticmethod
    def from_dict(dict values) -> GenerateFillReports:
        """
        Return a generate fill reports command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        GenerateFillReports

        """
        return GenerateFillReports.from_dict_c(values)

    @staticmethod
    def to_dict(GenerateFillReports obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return GenerateFillReports.to_dict_c(obj)


cdef class GeneratePositionStatusReports(ExecutionReportCommand):
    """
    Command to generate position status reports.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the command.
    start : datetime
        The start datetime (UTC) of request time range (inclusive).
    end : datetime
        The end datetime (UTC) of request time range.
        The inclusiveness depends on individual data client implementation.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the command.
    """

    def __init__(
        self,
        InstrumentId instrument_id : InstrumentId | None,
        datetime start : datetime | None,
        datetime end : datetime | None,
        UUID4 command_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            instrument_id,
            start,
            end,
            command_id,
            ts_init,
            params,
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"start={self.start}, "
            f"end={self.end}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef GeneratePositionStatusReports from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str i = values["instrument_id"]
        return GeneratePositionStatusReports(
            instrument_id=InstrumentId.from_str_c(i) if i is not None else None,
            start=_ns_to_datetime(values["start"]),
            end=_ns_to_datetime(values["end"]),
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
            params=values.get("params"),
        )

    @staticmethod
    cdef dict to_dict_c(GeneratePositionStatusReports obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "GeneratePositionStatusReports",
            "instrument_id": obj.instrument_id.to_str() if obj.instrument_id is not None else None,
            "start": obj.start,
            "end": obj.end,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
            "params": obj.params,
        }

    @staticmethod
    def from_dict(dict values) -> GeneratePositionStatusReports:
        """
        Return a generate position status reports command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        GeneratePositionStatusReports

        """
        return GeneratePositionStatusReports.from_dict_c(values)

    @staticmethod
    def to_dict(GeneratePositionStatusReports obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return GeneratePositionStatusReports.to_dict_c(obj)


cdef class GenerateExecutionMassStatus(ExecutionReportCommand):
    """
    Command to generate an execution mass status report.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    client_id : ClientId
        The client ID for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    venue : Venue, optional
        The venue for the command.
    params : dict[str, object], optional
        Additional parameters for the command.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        ClientId client_id not None,
        UUID4 command_id not None,
        uint64_t ts_init,
        Venue venue: Venue | None = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            instrument_id=None,
            start=None,
            end=None,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.trader_id = trader_id
        self.client_id = client_id
        self.venue = venue

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id}, "
            f"client_id={self.client_id}, "
            f"venue={self.venue}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef GenerateExecutionMassStatus from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str venue = values["venue"]
        return GenerateExecutionMassStatus(
            trader_id=TraderId(values["trader_id"]),
            client_id=ClientId(values["client_id"]),
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
            venue=Venue(venue) if venue else None,
            params=values.get("params"),
        )

    @staticmethod
    cdef dict to_dict_c(GenerateExecutionMassStatus obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "GenerateExecutionMassStatus",
            "trader_id": obj.trader_id.to_str(),
            "client_id": obj.client_id.to_str(),
            "venue": obj.venue.to_str() if obj.venue is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
            "params": obj.params,
        }

    @staticmethod
    def from_dict(dict values) -> GenerateExecutionMassStatus:
        """
        Return a generate execution mass status command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        GenerateExecutionMassStatus

        """
        return GenerateExecutionMassStatus.from_dict_c(values)

    @staticmethod
    def to_dict(GenerateExecutionMassStatus obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return GenerateExecutionMassStatus.to_dict_c(obj)


cdef class TradingCommand(Command):
    """
    The base class for all trading related commands.

    Parameters
    ----------
    client_id : ClientId or ``None``
        The execution client ID for the command.
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    params : dict[str, object], optional
        Additional parameters for the command.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id: ClientId | None,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        UUID4 command_id not None,
        uint64_t ts_init,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(command_id, ts_init)

        self.client_id = client_id
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.params = params or {}


cdef class SubmitOrder(TradingCommand):
    """
    Represents a command to submit the given order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    order : Order
        The order to submit.
    command_id : UUID4
        The commands ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    position_id : PositionId, optional
        The position ID for the command.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_D_68.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        Order order not None,
        UUID4 command_id not None,
        uint64_t ts_init,
        PositionId position_id: PositionId | None = None,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=order.instrument_id,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.order = order
        self.exec_algorithm_id = order.exec_algorithm_id
        self.position_id = position_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"order={self.order}, "
            f"position_id={self.position_id})" # Can be None
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.order.client_order_id.to_str()}, "
            f"order={self.order}, "
            f"position_id={self.position_id}, "  # Can be None
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef SubmitOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str p = values["position_id"]
        cdef Order order = OrderUnpacker.unpack_c(values["order"]),
        return SubmitOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            order=order,
            position_id=PositionId(p) if p is not None else None,
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(SubmitOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "SubmitOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "order": OrderInitialized.to_dict_c(obj.order.init_event_c()),
            "position_id": obj.position_id.to_str() if obj.position_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> SubmitOrder:
        """
        Return a submit order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        SubmitOrder

        """
        return SubmitOrder.from_dict_c(values)

    @staticmethod
    def to_dict(SubmitOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return SubmitOrder.to_dict_c(obj)


cdef class SubmitOrderList(TradingCommand):
    """
    Represents a command to submit an order list consisting of an order batch/bulk
    of related parent-child contingent orders.

    This command can correspond to a `NewOrderList <E> message` for the FIX
    protocol.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    order_list : OrderList
        The order list to submit.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    position_id : PositionId, optional
        The position ID for the command.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_E_69.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        OrderList order_list not None,
        UUID4 command_id not None,
        uint64_t ts_init,
        PositionId position_id: PositionId | None = None,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=order_list.instrument_id,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.order_list = order_list
        self.exec_algorithm_id = order_list.first.exec_algorithm_id
        self.position_id = position_id
        self.has_emulated_order = True if any(o.emulation_trigger != TriggerType.NO_TRIGGER for o in order_list.orders) else False

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"order_list={self.order_list}, "
            f"position_id={self.position_id})" # Can be None
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"order_list={self.order_list}, "
            f"position_id={self.position_id}, " # Can be None
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef SubmitOrderList from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str p = values["position_id"]
        cdef OrderList order_list = OrderList(
            order_list_id=OrderListId(values["order_list_id"]),
            orders=[OrderUnpacker.unpack_c(o_dict) for o_dict in values["orders"]],
        )
        return SubmitOrderList(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            order_list=order_list,
            position_id=PositionId(p) if p is not None else None,
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(SubmitOrderList obj):
        Condition.not_none(obj, "obj")
        cdef Order o
        return {
            "type": "SubmitOrderList",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "order_list_id": str(obj.order_list.id),
            "orders": [OrderInitialized.to_dict_c(o.init_event_c()) for o in obj.order_list.orders],
            "position_id": obj.position_id.to_str() if obj.position_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> SubmitOrderList:
        """
        Return a submit order list command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        SubmitOrderList

        """
        return SubmitOrderList.from_dict_c(values)

    @staticmethod
    def to_dict(SubmitOrderList obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return SubmitOrderList.to_dict_c(obj)


cdef class ModifyOrder(TradingCommand):
    """
    Represents a command to modify the properties of an existing order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    client_order_id : ClientOrderId
        The client order ID to update.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue) to update.
    quantity : Quantity or ``None``
        The quantity for the order update.
    price : Price or ``None``
        The price for the order update.
    trigger_price : Price or ``None``
        The trigger price for the order update.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: VenueOrderId | None,
        Quantity quantity: Quantity | None,
        Price price: Price | None,
        Price trigger_price: Price | None,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.quantity = quantity
        self.price = price
        self.trigger_price = trigger_price

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"quantity={self.quantity.to_formatted_str() if self.quantity is not None else None}, "
            f"price={self.price.to_formatted_str() if self.price is not None else None}, "
            f"trigger_price={self.trigger_price.to_formatted_str() if self.trigger_price is not None else None})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"quantity={self.quantity.to_formatted_str() if self.quantity is not None else None}, "
            f"price={self.price.to_formatted_str() if self.price is not None else None}, "
            f"trigger_price={self.trigger_price.to_formatted_str() if self.trigger_price is not None else None}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef ModifyOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str v = values["venue_order_id"]
        cdef str q = values["quantity"]
        cdef str p = values["price"]
        cdef str t = values["trigger_price"]
        return ModifyOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            quantity=Quantity.from_str_c(q) if q is not None else None,
            price=Price.from_str_c(p) if p is not None else None,
            trigger_price=Price.from_str_c(t) if t is not None else None,
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(ModifyOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "ModifyOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "quantity": str(obj.quantity) if obj.quantity is not None else None,
            "price": str(obj.price) if obj.price is not None else None,
            "trigger_price": str(obj.trigger_price) if obj.trigger_price is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> ModifyOrder:
        """
        Return a modify order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        ModifyOrder

        """
        return ModifyOrder.from_dict_c(values)

    @staticmethod
    def to_dict(ModifyOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return ModifyOrder.to_dict_c(obj)


cdef class CancelOrder(TradingCommand):
    """
    Represents a command to cancel an order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    client_order_id : ClientOrderId
        The client order ID to cancel.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue) to cancel.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_F_70.html
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: VenueOrderId | None,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        if client_id is None:
            client_id = ClientId(instrument_id.venue.value)
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id})"  # Can be None
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef CancelOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str v = values["venue_order_id"]
        return CancelOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(CancelOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "CancelOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> CancelOrder:
        """
        Return a cancel order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        CancelOrder

        """
        return CancelOrder.from_dict_c(values)

    @staticmethod
    def to_dict(CancelOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return CancelOrder.to_dict_c(obj)


cdef class CancelAllOrders(TradingCommand):
    """
    Represents a command to cancel all orders for an instrument.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    order_side : OrderSide
        The order side for the command.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        OrderSide order_side,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.order_side = order_side

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"order_side={order_side_to_str(self.order_side)})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"order_side={order_side_to_str(self.order_side)}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef CancelAllOrders from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        return CancelAllOrders(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            order_side=order_side_from_str(values["order_side"]),
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(CancelAllOrders obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "CancelAllOrders",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "order_side": order_side_to_str(obj.order_side),
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> CancelAllOrders:
        """
        Return a cancel order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        CancelAllOrders

        """
        return CancelAllOrders.from_dict_c(values)

    @staticmethod
    def to_dict(CancelAllOrders obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return CancelAllOrders.to_dict_c(obj)


cdef class BatchCancelOrders(TradingCommand):
    """
    Represents a command to batch cancel orders working on a venue for an instrument.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    cancels : list[CancelOrder]
        The inner list of cancel order commands.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.

    Raises
    ------
    ValueError
        If `cancels` is empty.
    ValueError
        If `cancels` contains a type other than `CancelOrder`.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        list cancels,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        Condition.not_empty(cancels, "cancels")
        Condition.list_type(cancels, CancelOrder, "cancels")
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.cancels = cancels

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"cancels={self.cancels})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"cancels={self.cancels}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef BatchCancelOrders from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str client_id = values["client_id"]
        return BatchCancelOrders(
            client_id=ClientId(client_id) if client_id is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            cancels=[CancelOrder.from_dict_c(cancel) for cancel in values["cancels"]],
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(BatchCancelOrders obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "BatchCancelOrders",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "cancels": [CancelOrder.to_dict_c(cancel) for cancel in obj.cancels],
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> BatchCancelOrders:
        """
        Return a batch cancel order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        BatchCancelOrders

        """
        return BatchCancelOrders.from_dict_c(values)

    @staticmethod
    def to_dict(BatchCancelOrders obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return BatchCancelOrders.to_dict_c(obj)


cdef class QueryOrder(TradingCommand):
    """
    Represents a command to query an order.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    strategy_id : StrategyId
        The strategy ID for the command.
    instrument_id : InstrumentId
        The instrument ID for the command.
    client_order_id : ClientOrderId
        The client order ID for the order to query.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue) to query.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id: VenueOrderId | None,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        if client_id is None:
            client_id = ClientId(instrument_id.venue.value)
        super().__init__(
            client_id=client_id,
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            ts_init=ts_init,
            params=params,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id})"  # Can be None
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef QueryOrder from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        cdef str v = values["venue_order_id"]
        return QueryOrder(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            client_order_id=ClientOrderId(values["client_order_id"]),
            venue_order_id=VenueOrderId(v) if v is not None else None,
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(QueryOrder obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "QueryOrder",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "client_order_id": obj.client_order_id.to_str(),
            "venue_order_id": obj.venue_order_id.to_str() if obj.venue_order_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> QueryOrder:
        """
        Return a query order command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        QueryOrder

        """
        return QueryOrder.from_dict_c(values)

    @staticmethod
    def to_dict(QueryOrder obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return QueryOrder.to_dict_c(obj)


cdef class QueryAccount(Command):
    """
    Represents a command to query an account.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the command.
    account_id : AccountId
        The account ID to query.
    command_id : UUID4
        The command ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_id : ClientId, optional
        The execution client ID for the command.
    params : dict[str, object], optional
        Additional parameters for the command.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        AccountId account_id not None,
        UUID4 command_id not None,
        uint64_t ts_init,
        ClientId client_id = None,
        dict[str, object] params: dict | None = None,
    ) -> None:
        super().__init__(command_id, ts_init)

        self.client_id = client_id
        self.trader_id = trader_id
        self.account_id = account_id

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "  # Can be None
            f"trader_id={self.trader_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"command_id={self.id.to_str()}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    cdef QueryAccount from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str c = values["client_id"]
        return QueryAccount(
            client_id=ClientId(c) if c is not None else None,
            trader_id=TraderId(values["trader_id"]),
            account_id=AccountId(values["account_id"]),
            command_id=UUID4.from_str_c(values["command_id"]),
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(QueryAccount obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "QueryAccount",
            "client_id": obj.client_id.to_str() if obj.client_id is not None else None,
            "trader_id": obj.trader_id.to_str(),
            "account_id": obj.account_id.to_str() if obj.account_id is not None else None,
            "command_id": obj.id.to_str(),
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> QueryAccount:
        """
        Return a query account command from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        QueryAccount

        """
        return QueryAccount.from_dict_c(values)

    @staticmethod
    def to_dict(QueryAccount obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return QueryAccount.to_dict_c(obj)
