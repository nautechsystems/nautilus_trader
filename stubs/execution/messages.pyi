from datetime import datetime
from typing import Any


class ExecutionReportCommand(Command):
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
    instrument_id: InstrumentId | None
    start: datetime | None
    end: datetime | None
    params: dict[str, Any]
    def __init__(
        self,
        instrument_id: InstrumentId | None,
        start: datetime | None,
        end: datetime | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class GenerateOrderStatusReport(ExecutionReportCommand):
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

    client_order_id: ClientOrderId | None
    venue_order_id: VenueOrderId | None

    def __init__(
        self,
        instrument_id: InstrumentId | None,
        client_order_id: ClientOrderId | None,
        venue_order_id: VenueOrderId | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class GenerateOrderStatusReports(ExecutionReportCommand):
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
    open_only: bool
    log_receipt_level: LogLevel
    def __init__(
        self,
        instrument_id: InstrumentId | None,
        start: datetime | None,
        end: datetime | None,
        open_only: bool,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
        log_receipt_level: LogLevel = ...,
    ) -> None: ...

class GenerateFillReports(ExecutionReportCommand):
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
    venue_order_id: VenueOrderId | None
    def __init__(
        self,
        instrument_id: InstrumentId | None,
        venue_order_id: VenueOrderId | None,
        start: datetime | None,
        end: datetime | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class GeneratePositionStatusReports(ExecutionReportCommand):
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
        instrument_id: InstrumentId | None,
        start: datetime | None,
        end: datetime | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class TradingCommand(Command):
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
    client_id: ClientId | None
    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    params: dict[str, Any]
    def __init__(
        self,
        client_id: ClientId | None,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class SubmitOrder(TradingCommand):
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
    order: Order
    exec_algorithm_id: ExecAlgorithmId | None
    position_id: PositionId | None
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order: Order,
        command_id: UUID4,
        ts_init: int,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> SubmitOrder:
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
        ...
    @staticmethod
    def to_dict(obj: SubmitOrder) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class SubmitOrderList(TradingCommand):
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

    order_list: OrderList
    exec_algorithm_id: ExecAlgorithmId | None
    position_id: PositionId | None
    has_emulated_order: bool

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order_list: OrderList,
        command_id: UUID4,
        ts_init: int,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> SubmitOrderList:
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
        ...
    @staticmethod
    def to_dict(obj: SubmitOrderList) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class ModifyOrder(TradingCommand):
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
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None
    quantity: Quantity | None
    price: Price | None
    trigger_price: Price | None
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        quantity: Quantity | None,
        price: Price | None,
        trigger_price: Price | None,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> ModifyOrder:
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
        ...
    @staticmethod
    def to_dict(obj: ModifyOrder) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class CancelOrder(TradingCommand):
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
    
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> CancelOrder:
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
        ...
    @staticmethod
    def to_dict(obj: CancelOrder) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class CancelAllOrders(TradingCommand):
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
    order_side: OrderSide
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> CancelAllOrders:
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
        ...
    @staticmethod
    def to_dict(obj: CancelAllOrders) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class BatchCancelOrders(TradingCommand):
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
    cancels: list[CancelOrder]
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        cancels: list,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> BatchCancelOrders:
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
        ...
    @staticmethod
    def to_dict(obj: BatchCancelOrders) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class QueryOrder(TradingCommand):
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
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> QueryOrder:
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
        ...
    @staticmethod
    def to_dict(obj: QueryOrder) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...