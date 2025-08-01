from typing import Any

class OrderEvent(Event):
    """
    The abstract base class for all order events.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    def set_client_order_id(self, client_order_id: ClientOrderId):
        ...

class OrderInitialized(OrderEvent):
    """
    Represents an event where an order has been initialized.

    This is a seed event which can instantiate any order through a creation
    method. This event should contain enough information to be able to send it
    'over the wire' and have a valid order created with exactly the same
    properties as if it had been instantiated locally.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    order_side : OrderSide {``BUY``, ``SELL``}
        The order side.
    order_type : OrderType
        The order type.
    quantity : Quantity
        The order quantity.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}
        The order time in force.
    post_only : bool
        If the order will only provide liquidity (make a market).
    reduce_only : bool
        If the order carries the 'reduce-only' execution instruction.
    quote_quantity : bool
        If the order quantity is denominated in the quote currency.
    options : dict[str, str]
        The order initialization options. Contains mappings for specific
        order parameters.
    emulation_trigger : TriggerType, default ``NO_TRIGGER``
        The type of market price trigger to use for local order emulation.
        - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
        - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
        Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
    trigger_instrument_id : InstrumentId or ``None``
        The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
    contingency_type : ContingencyType
        The order contingency type.
    order_list_id : OrderListId or ``None``
        The order list ID associated with the order.
    linked_order_ids : list[ClientOrderId] or ``None``
        The order linked client order ID(s).
    parent_order_id : ClientOrderId or ``None``
        The orders parent client order ID.
    exec_algorithm_id : ExecAlgorithmId or ``None``
        The execution algorithm ID for the order.
    exec_algorithm_params : dict[str, Any], optional
        The execution algorithm parameters for the order.
    exec_spawn_id : ClientOrderId or ``None``
        The execution algorithm spawning primary client order ID.
    tags : list[str] or ``None``
        The custom user tags for the order.
    event_id : UUID4
        The event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `order_side` is ``NO_ORDER_SIDE``.
    ValueError
        If `contingency_type` is not ``NO_CONTINGENCY``, and `linked_order_ids` is ``None`` or empty.
    ValueError
        If `exec_algorithm_id` is not ``None``, and `exec_spawn_id` is ``None``.
    """

    side: OrderSide
    order_type: OrderType
    quantity: Quantity
    time_in_force: TimeInForce
    post_only: bool
    reduce_only: bool
    quote_quantity: bool
    options: dict[str, Any]
    emulation_trigger: TriggerType
    trigger_instrument_id: InstrumentId | None
    contingency_type: ContingencyType
    order_list_id: OrderListId | None
    linked_order_ids: list[ClientOrderId] | None
    parent_order_id: ClientOrderId | None
    exec_algorithm_id: ExecAlgorithmId | None
    exec_algorithm_params: dict[str, Any] | None
    exec_spawn_id: ClientOrderId | None
    tags: list[str] | None

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        options: dict[str, object],
        emulation_trigger: TriggerType,
        trigger_instrument_id: InstrumentId | None,
        contingency_type: ContingencyType,
        order_list_id: OrderListId | None,
        linked_order_ids: list[ClientOrderId] | None,
        parent_order_id: ClientOrderId | None,
        exec_algorithm_id: ExecAlgorithmId | None,
        exec_algorithm_params: dict[str, object] | None,
        exec_spawn_id: ClientOrderId | None,
        tags: list[str] | None,
        event_id: UUID4,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderInitialized:
        """
        Return an order initialized event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderInitialized

        """
        ...
    @staticmethod
    def to_dict(obj: OrderInitialized) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderDenied(OrderEvent):
    """
    Represents an event where an order has been denied by the Nautilus system.

    This could be due an unsupported feature, a risk limit exceedance, or for
    any other reason that an otherwise valid order is not able to be submitted.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    reason : str
        The order denied reason.
    event_id : UUID4
        The event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: str,
        event_id: UUID4,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reason(self) -> str:
        """
        Return the reason the order was denied.

        Returns
        -------
        str

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderDenied:
        """
        Return an order denied event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderDenied

        """
        ...
    @staticmethod
    def to_dict(obj: OrderDenied) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderEmulated(OrderEvent):
    """
    Represents an event where an order has become emulated by the Nautilus system.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    event_id : UUID4
        The event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        event_id: UUID4,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderEmulated:
        """
        Return an order emulated event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderEmulated

        """
        ...
    @staticmethod
    def to_dict(obj: OrderEmulated) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderReleased(OrderEvent):
    """
    Represents an event where an order was released from the `OrderEmulator` by the Nautilus system.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    released_price : Price
        The price which released the order from the emulator.
    event_id : UUID4
        The event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        released_price: Price,
        event_id: UUID4,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def released_price(self) -> Price:
        """
        The released price for the event.

        Returns
        -------
        Price

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderReleased:
        """
        Return an order released event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderReleased

        """
        ...
    @staticmethod
    def to_dict(obj: OrderReleased) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted by the system to the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    account_id : AccountId
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order submitted event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderSubmitted:
        """
        Return an order submitted event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderSubmitted

        """
        ...
    @staticmethod
    def to_dict(obj: OrderSubmitted) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the trading venue.

    This event often corresponds to a `NEW` OrdStatus <39> field in FIX execution reports.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId
        The venue order ID (assigned by the venue).
    account_id : AccountId
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order accepted event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/tagNum_39.html
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderAccepted:
        """
        Return an order accepted event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderAccepted

        """
        ...
    @staticmethod
    def to_dict(obj: OrderAccepted) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    account_id : AccountId
        The account ID (with the venue).
    reason : str
        The order rejected reason.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order rejected event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reason(self) -> str:
        """
        Return the reason the order was rejected.

        Returns
        -------
        str

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderRejected:
        """
        Return an order rejected event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderRejected

        """
        ...
    @staticmethod
    def to_dict(obj: OrderRejected) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderCanceled(OrderEvent):
    """
    Represents an event where an order has been canceled at the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when order canceled event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderCanceled:
        """
        Return an order canceled event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderCanceled

        """
        ...
    @staticmethod
    def to_dict(obj: OrderCanceled) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired at the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order expired event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderExpired:
        """
        Return an order expired event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderExpired

        """
        ...
    @staticmethod
    def to_dict(obj: OrderExpired) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderTriggered(OrderEvent):
    """
    Represents an event where an order has triggered.

    Applicable to :class:`StopLimit` orders only.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order triggered event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderTriggered:
        """
        Return an order triggered event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderTriggered

        """
        ...
    @staticmethod
    def to_dict(obj: OrderTriggered) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderPendingUpdate(OrderEvent):
    """
    Represents an event where an `ModifyOrder` command has been sent to the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order pending update event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderPendingUpdate:
        """
        Return an order pending update event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderPendingUpdate

        """
        ...
    @staticmethod
    def to_dict(obj: OrderPendingUpdate) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderPendingCancel(OrderEvent):
    """
    Represents an event where a `CancelOrder` command has been sent to the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order pending cancel event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderPendingCancel:
        """
        Return an order pending cancel event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderPendingCancel

        """
        ...
    @staticmethod
    def to_dict(obj: OrderPendingCancel) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderModifyRejected(OrderEvent):
    """
    Represents an event where a `ModifyOrder` command has been rejected by the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    reason : str
        The order update rejected reason.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order update rejected event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reason(self) -> str:
        """
        Return the reason the order was rejected.

        Returns
        -------
        str

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderModifyRejected:
        """
        Return an order update rejected event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderModifyRejected

        """
        ...
    @staticmethod
    def to_dict(obj: OrderModifyRejected) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderCancelRejected(OrderEvent):
    """
    Represents an event where a `CancelOrder` command has been rejected by the
    trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    reason : str
        The order cancel rejected reason.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order cancel rejected event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reason(self) -> str:
        """
        Return the reason the order was rejected.

        Returns
        -------
        str

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderCancelRejected:
        """
        Return an order cancel rejected event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderCancelRejected

        """
        ...
    @staticmethod
    def to_dict(obj: OrderCancelRejected) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderUpdated(OrderEvent):
    """
    Represents an event where an order has been updated at the trading venue.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId or ``None``
        The venue order ID (assigned by the venue).
    account_id : AccountId or ``None``
        The account ID (with the venue).
    quantity : Quantity
        The orders current quantity.
    price : Price or ``None``
        The orders current price.
    trigger_price : Price or ``None``
        The orders current trigger.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order updated event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `quantity` is not positive (> 0).
    """

    quantity: Quantity
    price: Price | None
    trigger_price: Price | None
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        account_id: AccountId | None,
        quantity: Quantity,
        price: Price | None,
        trigger_price: Price | None,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderUpdated:
        """
        Return an order updated event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderUpdated

        """
        ...
    @staticmethod
    def to_dict(obj: OrderUpdated) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderFilled(OrderEvent):
    """
    Represents an event where an order has been filled at the exchange.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    venue_order_id : VenueOrderId
        The venue order ID (assigned by the venue).
    account_id : AccountId
        The account ID (with the venue).
    trade_id : TradeId
        The trade match ID (assigned by the venue).
    position_id : PositionId or ``None``
        The position ID associated with the order fill (assigned by the venue).
    order_side : OrderSide {``BUY``, ``SELL``}
        The execution order side.
    order_type : OrderType
        The execution order type.
    last_qty : Quantity
        The fill quantity for this execution.
    last_px : Price
        The fill price for this execution (not average price).
    currency : Currency
        The currency of the price.
    commission : Money
        The fill commission.
    liquidity_side : LiquiditySide {``NO_LIQUIDITY_SIDE``, ``MAKER``, ``TAKER``}
        The execution liquidity side.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the order filled event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    info : dict[str, object], optional
        The additional fill information.
    reconciliation : bool, default False
        If the event was generated during reconciliation.

    Raises
    ------
    ValueError
        If `order_side` is ``NO_ORDER_SIDE``.
    ValueError
        If `last_qty` is not positive (> 0).
    """

    trade_id: TradeId
    position_id: PositionId | None
    order_side: OrderSide
    order_type: OrderType
    last_qty: Quantity
    last_px: Price
    currency: Currency
    commission: Money
    liquidity_side: LiquiditySide
    info: dict[str, Any]
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        trade_id: TradeId,
        position_id: PositionId | None,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def set_client_order_id(self, client_order_id: ClientOrderId): ...
    @property
    def trader_id(self) -> TraderId:
        """
        The trader ID associated with the event.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def strategy_id(self) -> TraderId:
        """
        The strategy ID associated with the event.

        Returns
        -------
        StrategyId

        """
        ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        The instrument ID associated with the event.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def client_order_id(self) -> ClientOrderId:
        """
        The client order ID associated with the event.

        Returns
        -------
        ClientOrderId

        """
        ...
    @property
    def venue_order_id(self) -> VenueOrderId | None:
        """
        The venue order ID associated with the event.

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    @property
    def account_id(self) -> AccountId | None:
        """
        The account ID associated with the event.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    @property
    def reconciliation(self) -> bool:
        """
        If the event was generated during reconciliation.

        Returns
        -------
        bool

        """
        ...
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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderFilled:
        """
        Return an order filled event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderFilled

        """
        ...
    @staticmethod
    def to_dict(obj: OrderFilled) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @property
    def is_buy(self) -> bool:
        """
        Return whether the fill order side is ``BUY``.

        Returns
        -------
        bool

        """
        ...
    @property
    def is_sell(self) -> bool:
        """
        Return whether the fill order side is ``SELL``.

        Returns
        -------
        bool

        """
        ...