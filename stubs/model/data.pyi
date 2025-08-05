from datetime import timedelta
from enum import Enum
from typing import Any

import numpy as np

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from stubs.core.data import Data
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import TradeId
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class BarAggregation(Enum): # skip-validate
    TICK = 1
    TICK_IMBALANCE = 2
    TICK_RUNS = 3
    VOLUME = 4
    VOLUME_IMBALANCE = 5
    VOLUME_RUNS = 6
    VALUE = 7
    VALUE_IMBALANCE = 8
    VALUE_RUNS = 9
    MILLISECOND = 10
    SECOND = 11
    MINUTE = 12
    HOUR = 13
    DAY = 14
    WEEK = 15
    MONTH = 16


def capsule_to_list(capsule) -> list[Data]: ...
def capsule_to_data(capsule) -> Data: ...


class BarSpecification:
    """
    Represents a bar aggregation specification including a step, aggregation
    method/rule and price type.

    Parameters
    ----------
    step : int
        The step for binning samples for bar aggregation (> 0).
    aggregation : BarAggregation
        The type of bar aggregation.
    price_type : PriceType
        The price type to use for aggregation.

    Raises
    ------
    ValueError
        If `step` is not positive (> 0).

    """

    def __init__(self, step: int, aggregation: BarAggregation, price_type: PriceType) -> None: ...
    def __getstate__(self) -> tuple[int, BarAggregation, PriceType]: ...
    def __setstate__(self, state: tuple[int, BarAggregation, PriceType]) -> None: ...
    def __eq__(self, other: BarSpecification) -> bool: ...
    def __lt__(self, other: BarSpecification) -> bool: ...
    def __le__(self, other: BarSpecification) -> bool: ...
    def __gt__(self, other: BarSpecification) -> bool: ...
    def __ge__(self, other: BarSpecification) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def step(self) -> int:
        """
        Return the step size for the specification.

        Returns
        -------
        int

        """
        ...
    @property
    def aggregation(self) -> BarAggregation:
        """
        Return the aggregation for the specification.

        Returns
        -------
        BarAggregation

        """
        ...
    @property
    def price_type(self) -> PriceType:
        """
        Return the price type for the specification.

        Returns
        -------
        PriceType

        """
        ...
    @property
    def timedelta(self) -> timedelta:
        """
        Return the timedelta for the specification.

        Returns
        -------
        timedelta

        Raises
        ------
        ValueError
            If `aggregation` is not a time aggregation, or is``MONTH`` (which is ambiguous).

        """
        ...
    @staticmethod
    def from_str(value: str) -> BarSpecification:
        """
        Return a bar specification parsed from the given string.

        Parameters
        ----------
        value : str
            The bar specification string to parse.

        Examples
        --------
        String format example is '200-TICK-MID'.

        Returns
        -------
        BarSpecification

        Raises
        ------
        ValueError
            If `value` is not a valid string.

        """
        ...
    @staticmethod
    def from_timedelta(duration: timedelta, price_type: PriceType) -> BarSpecification:
        """
        Return a bar specification parsed from the given timedelta and price_type.

        Parameters
        ----------
        duration : timedelta
            The bar specification timedelta to parse.
        price_type : PriceType
            The bar specification price_type.

        Examples
        --------
        BarSpecification.from_timedelta(datetime.timedelta(minutes=5), PriceType.LAST).

        Returns
        -------
        BarSpecification

        Raises
        ------
        ValueError
            If `duration` is not rounded step of aggregation.

        """
        ...
    @staticmethod
    def check_time_aggregated(aggregation: BarAggregation) -> bool:
        """
        Check the given aggregation is a type of time aggregation.

        Parameters
        ----------
        aggregation : BarAggregation
            The aggregation type to check.

        Returns
        -------
        bool
            True if time aggregated, else False.

        """
        ...
    @staticmethod
    def check_threshold_aggregated(aggregation: BarAggregation) -> bool:
        """
        Check the given aggregation is a type of threshold aggregation.

        Parameters
        ----------
        aggregation : BarAggregation
            The aggregation type to check.

        Returns
        -------
        bool
            True if threshold aggregated, else False.

        """
        ...
    @staticmethod
    def check_information_aggregated(aggregation: BarAggregation) -> bool:
        """
        Check the given aggregation is a type of information aggregation.

        Parameters
        ----------
        aggregation : BarAggregation
            The aggregation type to check.

        Returns
        -------
        bool
            True if information aggregated, else False.

        """
        ...
    def is_time_aggregated(self) -> bool:
        """
        Return a value indicating whether the aggregation method is time-driven.

        - ``SECOND``
        - ``MINUTE``
        - ``HOUR``
        - ``DAY``
        - ``WEEK``
        - ``MONTH``

        Returns
        -------
        bool

        """
        ...
    def is_threshold_aggregated(self) -> bool:
        """
        Return a value indicating whether the bar aggregation method is
        threshold-driven.

        - ``TICK``
        - ``TICK_IMBALANCE``
        - ``VOLUME``
        - ``VOLUME_IMBALANCE``
        - ``VALUE``
        - ``VALUE_IMBALANCE``

        Returns
        -------
        bool

        """
        ...
    def is_information_aggregated(self) -> bool:
        """
        Return a value indicating whether the aggregation method is
        information-driven.

        - ``TICK_RUNS``
        - ``VOLUME_RUNs``
        - ``VALUE_RUNS``

        Returns
        -------
        bool

        """
        ...

class BarType:
    """
    Represents a bar type including the instrument ID, bar specification and
    aggregation source.

    Parameters
    ----------
    instrument_id : InstrumentId
        The bar type's instrument ID.
    bar_spec : BarSpecification
        The bar type's specification.
    aggregation_source : AggregationSource, default EXTERNAL
        The bar type aggregation source. If ``INTERNAL`` the `DataEngine`
        will subscribe to the necessary ticks and aggregate bars accordingly.
        Else if ``EXTERNAL`` then bars will be subscribed to directly from
        the venue / data provider.

    Notes
    -----
    It is expected that all bar aggregation methods other than time will be
    internally aggregated.

    """

    def __init__(self, instrument_id: InstrumentId, bar_spec: BarSpecification, aggregation_source: AggregationSource = AggregationSource.EXTERNAL) -> None: ...
    def __getstate__(self) -> tuple[Any, ...]: ...
    def __setstate__(self, state: tuple[Any, ...]) -> None: ...
    def __eq__(self, other: BarType) -> bool: ...
    def __lt__(self, other: BarType) -> bool: ...
    def __le__(self, other: BarType) -> bool: ...
    def __gt__(self, other: BarType) -> bool: ...
    def __ge__(self, other: BarType) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the instrument ID for the bar type.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def spec(self) -> BarSpecification:
        """
        Return the specification for the bar type.

        Returns
        -------
        BarSpecification

        """
        ...
    @property
    def aggregation_source(self) -> AggregationSource:
        """
        Return the aggregation source for the bar type.

        Returns
        -------
        AggregationSource

        """
        ...
    @staticmethod
    def from_str(value: str) -> BarType:
        """
        Return a bar type parsed from the given string.

        Parameters
        ----------
        value : str
            The bar type string to parse.

        Returns
        -------
        BarType

        Raises
        ------
        ValueError
            If `value` is not a valid string.

        """
        ...
    def is_externally_aggregated(self) -> bool:
        """
        Return a value indicating whether the bar aggregation source is ``EXTERNAL``.

        Returns
        -------
        bool

        """
        ...
    def is_internally_aggregated(self) -> bool:
        """
        Return a value indicating whether the bar aggregation source is ``INTERNAL``.

        Returns
        -------
        bool

        """
        ...
    @staticmethod
    def new_composite(instrument_id: InstrumentId, bar_spec: BarSpecification, aggregation_source: AggregationSource, composite_step: int, composite_aggregation: BarAggregation, composite_aggregation_source: AggregationSource) -> BarType: ...
    def is_standard(self) -> bool:
        """
        Return a value indicating whether the bar type corresponds to `BarType::Standard` in Rust.

        Returns
        -------
        bool

        """
        ...
    def is_composite(self) -> bool:
        """
        Return a value indicating whether the bar type corresponds to `BarType::Composite` in Rust.

        Returns
        -------
        bool

        """
        ...
    def standard(self) -> BarType: ...
    def composite(self) -> BarType: ...

class Bar(Data):
    """
    Represents an aggregated bar.

    Parameters
    ----------
    bar_type : BarType
        The bar type for this bar.
    open : Price
        The bars open price.
    high : Price
        The bars high price.
    low : Price
        The bars low price.
    close : Price
        The bars close price.
    volume : Quantity
        The bars volume.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
    is_revision : bool, default False
        If this bar is a revision of a previous bar with the same `ts_event`.

    Raises
    ------
    ValueError
        If `high` is not >= `low`.
    ValueError
        If `high` is not >= `close`.
    ValueError
        If `low` is not <= `close`.

    """

    is_revision: bool
    def __init__(self, bar_type: BarType, open: Price, high: Price, low: Price, close: Price, volume: Quantity, ts_event: int, ts_init: int, is_revision: bool = False) -> None: ...
    def __getstate__(self) -> tuple[Any, ...]: ...
    def __setstate__(self, state: tuple[Any, ...]) -> None: ...
    def __eq__(self, other: Bar) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def bar_type(self) -> BarType:
        """
        Return the bar type of bar.

        Returns
        -------
        BarType

        """
        ...
    @property
    def open(self) -> Price:
        """
        Return the open price of the bar.

        Returns
        -------
        Price

        """
        ...
    @property
    def high(self) -> Price:
        """
        Return the high price of the bar.

        Returns
        -------
        Price

        """
        ...
    @property
    def low(self) -> Price:
        """
        Return the low price of the bar.

        Returns
        -------
        Price

        """
        ...
    @property
    def close(self) -> Price:
        """
        Return the close price of the bar.

        Returns
        -------
        Price

        """
        ...
    @property
    def volume(self) -> Quantity:
        """
        Return the volume of the bar.

        Returns
        -------
        Quantity

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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
    def from_raw_arrays_to_list(
        bar_type: BarType, 
        price_prec: int, 
        size_prec: int, 
        opens: np.ndarray, # skip-validate
        highs: np.ndarray, # skip-validate
        lows: np.ndarray, # skip-validate
        closes: np.ndarray, # skip-validate
        volumes: np.ndarray, # skip-validate
        ts_events: np.ndarray, # skip-validate
        ts_inits: np.ndarray # skip-validate
    ) -> list[Bar]: ...
    @staticmethod
    def from_raw(bar_type: BarType, open: PriceRaw, high: PriceRaw, low: PriceRaw, close: PriceRaw, price_prec: int, volume: QuantityRaw, size_prec: int, ts_event: int, ts_init: int) -> Bar: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> Bar:
        """
        Return a bar parsed from the given values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        Bar

        """
        ...
    @staticmethod
    def to_dict(obj: Bar) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def to_pyo3_list(bars: list[Bar]) -> list[nautilus_pyo3.Bar]:
        """
        Return pyo3 Rust bars converted from the given legacy Cython objects.

        Parameters
        ----------
        bars : list[Bar]
            The legacy Cython bars to convert from.

        Returns
        -------
        list[nautilus_pyo3.Bar]

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_bars: list[nautilus_pyo3.Bar]) -> list[Bar]:
        """
        Return legacy Cython bars converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_bars : list[nautilus_pyo3.Bar]
            The pyo3 Rust bars to convert from.

        Returns
        -------
        list[Bar]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_bar: nautilus_pyo3.Bar) -> Bar:
        """
        Return a legacy Cython bar converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_bar : nautilus_pyo3.Bar
            The pyo3 Rust bar to convert from.

        Returns
        -------
        Bar

        """
        ...
    def to_pyo3(self) -> nautilus_pyo3.Bar:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.Bar

        """
        ...
    def is_single_price(self) -> bool:
        """
        If the OHLC are all equal to a single price.

        Returns
        -------
        bool

        """
        ...

class DataType:
    """
    Represents a data type including metadata.

    Parameters
    ----------
    type : type
        The `Data` type of the data.
    metadata : dict
        The data types metadata.

    Raises
    ------
    ValueError
        If `type` is not either a subclass of `Data` or meets the `Data` contract.
    TypeError
        If `metadata` contains a key or value which is not hashable.

    Warnings
    --------
    This class may be used as a key in hash maps throughout the system, thus
    the key and value contents of metadata must themselves be hashable.

    """

    type: type
    metadata: dict
    topic: str
    def __init__(self, type: type, metadata: dict[Any, Any] | None = None) -> None: ...
    def __eq__(self, other: DataType) -> bool: ...
    def __lt__(self, other: DataType) -> bool: ...
    def __le__(self, other: DataType) -> bool: ...
    def __gt__(self, other: DataType) -> bool: ...
    def __ge__(self, other: DataType) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

class CustomData(Data):
    """
    Provides a wrapper for custom data which includes data type information.

    Parameters
    ----------
    data_type : DataType
        The data type.
    data : Data
        The data object to wrap.

    """

    data_type: DataType
    data: Data
    def __init__(self, data_type: DataType, data: Data) -> None: ...
    def __repr__(self) -> str: ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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

NULL_ORDER: BookOrder

class BookOrder:
    """
    Represents an order in a book.

    Parameters
    ----------
    side : OrderSide {``BUY``, ``SELL``}
        The order side.
    price : Price
        The order price.
    size : Quantity
        The order size.
    order_id : uint64_t
        The order ID.

    """

    def __init__(self, side: OrderSide, price: Price, size: Quantity, order_id: int) -> None: ...
    def __getstate__(self) -> tuple[OrderSide, int, int, int, int, int]: ...
    def __setstate__(self, state: tuple[OrderSide, int, int, int, int, int]) -> None: ...
    def __eq__(self, other: BookOrder) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def price(self) -> Price:
        """
        Return the book orders price.

        Returns
        -------
        Price

        """
        ...
    @property
    def size(self) -> Quantity:
        """
        Return the book orders size.

        Returns
        -------
        Quantity

        """
        ...
    @property
    def side(self) -> OrderSide:
        """
        Return the book orders side.

        Returns
        -------
        OrderSide

        """
        ...
    @property
    def order_id(self) -> int:
        """
        Return the book orders side.

        Returns
        -------
        uint64_t

        """
        ...
    def exposure(self) -> float:
        """
        Return the total exposure for this order (price * size).

        Returns
        -------
        double

        """
        ...
    def signed_size(self) -> float:
        """
        Return the signed size of the order (negative for ``SELL``).

        Returns
        -------
        double

        """
        ...
    @staticmethod
    def from_raw(side: OrderSide, price_raw: PriceRaw, price_prec: int, size_raw: QuantityRaw, size_prec: int, order_id: int) -> BookOrder:
        """
        Return an book order from the given raw values.

        Parameters
        ----------
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        price_raw : int
            The order raw price (as a scaled fixed-point integer).
        price_prec : uint8_t
            The order price precision.
        size_raw : int
            The order raw size (as a scaled fixed-point integer).
        size_prec : uint8_t
            The order size precision.
        order_id : uint64_t
            The order ID.

        Returns
        -------
        BookOrder

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> BookOrder:
        """
        Return an order from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        BookOrder

        """
        ...
    @staticmethod
    def to_dict(obj: BookOrder) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class OrderBookDelta(Data):
    """
    Represents a single update/difference on an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    action : BookAction {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}
        The order book delta action.
    order : BookOrder or ``None``
        The book order for the delta.
    flags : uint8_t
        The record flags bit field, indicating event end and data information.
        A value of zero indicates no flags.
    sequence : uint64_t
        The unique sequence number for the update.
        If no sequence number provided in the source data then use a value of zero.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `action` is `ADD` or `UPDATE` and `order.size` is not positive (> 0).

    """

    def __init__(self, instrument_id: InstrumentId, action: BookAction, order: BookOrder | None, flags: int, sequence: int, ts_event: int, ts_init: int) -> None: ...
    def __getstate__(self) -> tuple[Any, ...]: ...
    def __setstate__(self, state: tuple[Any, ...]) -> None: ...
    def __eq__(self, other: OrderBookDelta) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the deltas book instrument ID.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def action(self) -> BookAction:
        """
        Return the deltas book action {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}

        Returns
        -------
        BookAction

        """
        ...
    @property
    def is_add(self) -> BookAction:
        """
        If the deltas book action is an ``ADD``.

        Returns
        -------
        bool

        """
        ...
    @property
    def is_update(self) -> BookAction:
        """
        If the deltas book action is an ``UPDATE``.

        Returns
        -------
        bool

        """
        ...
    @property
    def is_delete(self) -> BookAction:
        """
        If the deltas book action is a ``DELETE``.

        Returns
        -------
        bool

        """
        ...
    @property
    def is_clear(self) -> BookAction:
        """
        If the deltas book action is a ``CLEAR``.

        Returns
        -------
        bool

        """
        ...
    @property
    def order(self) -> BookOrder | None:
        """
        Return the deltas book order for the action.

        Returns
        -------
        BookOrder

        """
        ...
    @property
    def flags(self) -> int:
        """
        Return the flags for the delta.

        Returns
        -------
        uint8_t

        """
        ...
    @property
    def sequence(self) -> int:
        """
        Return the sequence number for the delta.

        Returns
        -------
        uint64_t

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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
    def list_from_capsule(capsule: Any) -> list[OrderBookDelta]: ...
    @staticmethod
    def capsule_from_list(items: list[OrderBookDelta]) -> Any: ...
    @staticmethod
    def from_raw(instrument_id: InstrumentId, action: BookAction, side: OrderSide, price_raw: PriceRaw, price_prec: int, size_raw: QuantityRaw, size_prec: int, order_id: int, flags: int, sequence: int, ts_event: int, ts_init: int) -> OrderBookDelta:
        """
        Return an order book delta from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The trade instrument ID.
        action : BookAction {``ADD``, ``UPDATE``, ``DELETE``, ``CLEAR``}
            The order book delta action.
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        price_raw : int
            The order raw price (as a scaled fixed-point integer).
        price_prec : uint8_t
            The order price precision.
        size_raw : int
            The order raw size (as a scaled fixed-point integer).
        size_prec : uint8_t
            The order size precision.
        order_id : uint64_t
            The order ID.
        flags : uint8_t
            The record flags bit field, indicating event end and data information.
            A value of zero indicates no flags.
        sequence : uint64_t
            The unique sequence number for the update.
            If no sequence number provided in the source data then use a value of zero.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        OrderBookDelta

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderBookDelta:
        """
        Return an order book delta from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDelta

        """
        ...
    @staticmethod
    def to_dict(obj: OrderBookDelta) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def clear(instrument_id: InstrumentId, sequence: int, ts_event: int, ts_init: int) -> OrderBookDelta:
        """
        Return an order book delta which acts as an initial ``CLEAR``.

        Returns
        -------
        OrderBookDelta

        """
        ...
    @staticmethod
    def to_pyo3_list(deltas: list[OrderBookDelta]) -> list[nautilus_pyo3.OrderBookDelta]:
        """
        Return pyo3 Rust order book deltas converted from the given legacy Cython objects.

        Parameters
        ----------
        pyo3_deltas : list[OrderBookDelta]
            The pyo3 Rust order book deltas to convert from.

        Returns
        -------
        list[nautilus_pyo3.OrderBookDelta]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_delta: nautilus_pyo3.OrderBookDelta) -> OrderBookDelta:
        """
        Return a legacy Cython order book delta converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_delta : nautilus_pyo3.OrderBookDelta
            The pyo3 Rust order book delta to convert from.

        Returns
        -------
        OrderBookDelta

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_deltas: list[nautilus_pyo3.OrderBookDelta]) -> list[OrderBookDelta]:
        """
        Return legacy Cython order book deltas converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_deltas : list[nautilus_pyo3.OrderBookDelta]
            The pyo3 Rust order book deltas to convert from.

        Returns
        -------
        list[OrderBookDelta]

        """
        ...

class OrderBookDeltas(Data):
    """
    Represents a batch of `OrderBookDelta` updates for an `OrderBook`.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    deltas : list[OrderBookDelta]
        The batch of order book changes.

    Raises
    ------
    ValueError
        If `deltas` is an empty list.

    """

    def __init__(self, instrument_id: InstrumentId, deltas: list[OrderBookDelta]) -> None: ...
    def __getstate__(self) -> tuple[str, bytes]: ...
    def __setstate__(self, state: tuple[str, bytes]) -> None: ...
    def __del__(self) -> None: ...
    def __eq__(self, other: OrderBookDeltas) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the deltas book instrument ID.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def deltas(self) -> list[OrderBookDelta]:
        """
        Return the contained deltas.

        Returns
        -------
        list[OrderBookDeltas]

        """
        ...
    @property
    def is_snapshot(self) -> bool:
        """
        If the deltas is a snapshot.

        Returns
        -------
        bool

        """
        ...
    @property
    def flags(self) -> int:
        """
        Return the flags for the last delta.

        Returns
        -------
        uint8_t

        """
        ...
    @property
    def sequence(self) -> int:
        """
        Return the sequence number for the last delta.

        Returns
        -------
        uint64_t

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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
    def from_dict(values: dict[str, Any]) -> OrderBookDeltas:
        """
        Return order book deltas from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDeltas

        """
        ...
    @staticmethod
    def to_dict(obj: OrderBookDeltas) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def batch(data: list[OrderBookDelta]) -> list[OrderBookDeltas]:
        """
        Groups the given list of `OrderBookDelta` records into batches, creating `OrderBookDeltas`
        objects when an `F_LAST` flag is encountered.

        The method iterates through the `data` list and appends each `OrderBookDelta` to the current
        batch. When an `F_LAST` flag is found, it indicates the end of a batch. The batch is then
        appended to the list of completed batches and a new batch is started.

        Returns
        -------
        list[OrderBookDeltas]

        Raises
        ------
        ValueError
            If `data` is empty.
        TypeError
            If `data` is not a list of `OrderBookDelta`.

        Warnings
        --------
        UserWarning
            If there are remaining deltas in the final batch after the last `F_LAST` flag.

        """
        ...
    def to_capsule(self) -> Any: ...
    def to_pyo3(self) -> nautilus_pyo3.OrderBookDeltas:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.OrderBookDeltas

        """
        ...

class OrderBookDepth10(Data):
    """
    Represents a self-contained order book update with a fixed depth of 10 levels per side.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    bids : list[BookOrder]
        The bid side orders for the update.
    asks : list[BookOrder]
        The ask side orders for the update.
    bid_counts : list[uint32_t]
        The count of bid orders per level for the update. Can be zeros if data not available.
    ask_counts : list[uint32_t]
        The count of ask orders per level for the update. Can be zeros if data not available.
    flags : uint8_t
        The record flags bit field, indicating event end and data information.
        A value of zero indicates no flags.
    sequence : uint64_t
        The unique sequence number for the update.
        If no sequence number provided in the source data then use a value of zero.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `bids`, `asks`, `bid_counts`, `ask_counts` lengths are greater than 10.
    ValueError
        If `bids`, `asks`, `bid_counts`, `ask_counts` lengths are not equal.

    """

    def __init__(self, instrument_id: InstrumentId, bids: list[BookOrder], asks: list[BookOrder], bid_counts: list[int], ask_counts: list[int], flags: int, sequence: int, ts_event: int, ts_init: int) -> None: ...
    def __getstate__(self) -> tuple[str, bytes, bytes, bytes, bytes, int, int, int, int]: ...
    def __setstate__(self, state: tuple[str, bytes, bytes, bytes, bytes, int, int, int, int]) -> None: ...
    def __eq__(self, other: OrderBookDepth10) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the depth updates book instrument ID.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def bids(self) -> list[BookOrder]:
        """
        Return the bid orders for the update.

        Returns
        -------
        list[BookOrder]

        """
        ...
    @property
    def asks(self) -> list[BookOrder]:
        """
        Return the ask orders for the update.

        Returns
        -------
        list[BookOrder]

        """
        ...
    @property
    def bid_counts(self) -> list[int]:
        """
        Return the count of bid orders per level for the update.

        Returns
        -------
        list[uint32_t]

        """
        ...
    @property
    def ask_counts(self) -> list[int]:
        """
        Return the count of ask orders level for the update.

        Returns
        -------
        list[uint32_t]

        """
        ...
    @property
    def flags(self) -> int:
        """
        Return the flags for the depth update.

        Returns
        -------
        uint8_t

        """
        ...
    @property
    def sequence(self) -> int:
        """
        Return the sequence number for the depth update.

        Returns
        -------
        uint64_t

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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
    def list_from_capsule(capsule: Any) -> list[OrderBookDepth10]: ...
    @staticmethod
    def capsule_from_list(items: list[OrderBookDepth10]) -> Any: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderBookDepth10:
        """
        Return order book depth from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        OrderBookDepth10

        """
        ...
    @staticmethod
    def to_dict(obj: OrderBookDepth10) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_depth: nautilus_pyo3.OrderBookDepth10) -> OrderBookDepth10:
        """
        Return a legacy Cython order book depth converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_depth : nautilus_pyo3.OrderBookDepth10
            The pyo3 Rust order book depth to convert from.

        Returns
        -------
        OrderBookDepth10

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_depths: list[nautilus_pyo3.OrderBookDepth10]) -> list[OrderBookDepth10]:
        """
        Return legacy Cython order book depths converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_depths : nautilus_pyo3.OrderBookDepth10
            The pyo3 Rust order book depths to convert from.

        Returns
        -------
        list[OrderBookDepth10]

        """
        ...

class InstrumentStatus(Data):
    """
    Represents an event that indicates a change in an instrument market status.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the status change.
    action : MarketStatusAction
        The instrument market status action.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the status event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    reason : str, optional
        Additional details about the cause of the status change.
    trading_event : str, optional
        Further information about the status change (if provided).
    is_trading : bool, optional
        The state of trading in the instrument.
    is_quoting : bool, optional
        The state of quoting in the instrument.
    is_short_sell_restricted : bool, optional
        The state of short sell restrictions for the instrument (if applicable).

    """

    instrument_id: InstrumentId
    action: MarketStatusAction
    ts_event: int
    ts_init: int
    reason: str | None
    trading_event: str | None
    _is_trading: bool | None
    _is_quoting: bool | None
    _is_short_sell_restricted: bool | None
    def __init__(self, instrument_id: InstrumentId, action: MarketStatusAction, ts_event: int, ts_init: int, reason: str | None = None, trading_event: str | None = None, is_trading: bool | None = None, is_quoting: bool | None = None, is_short_sell_restricted: bool | None = None) -> None: ...
    def __eq__(self, other: InstrumentStatus) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def is_trading(self) -> bool | None:
        """
        Return the state of trading in the instrument (if known).

        returns
        -------
        bool or ``None``

        """
        ...
    @property
    def is_quoting(self) -> bool | None:
        """
        Return the state of quoting in the instrument (if known).

        returns
        -------
        bool or ``None``

        """
        ...
    @property
    def is_short_sell_restricted(self) -> bool | None:
        """
        Return the state of short sell restrictions for the instrument (if known and applicable).

        returns
        -------
        bool or ``None``

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> InstrumentStatus:
        """
        Return an instrument status update from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentStatus

        """
        ...
    @staticmethod
    def to_dict(obj: InstrumentStatus) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_status_list: list[nautilus_pyo3.InstrumentStatus]) -> list[InstrumentStatus]:
        """
        Return legacy Cython instrument status converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_status_list : list[nautilus_pyo3.InstrumentStatus]
            The pyo3 Rust instrument status list to convert from.

        Returns
        -------
        list[InstrumentStatus]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_status: nautilus_pyo3.InstrumentStatus) -> InstrumentStatus:
        """
        Return a legacy Cython quote tick converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_status : nautilus_pyo3.InstrumentStatus
            The pyo3 Rust instrument status to convert from.

        Returns
        -------
        InstrumentStatus

        """
        ...
    def to_pyo3(self) -> nautilus_pyo3.InstrumentStatus:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.InstrumentStatus

        """
        ...

class InstrumentClose(Data):
    """
    Represents an instrument close at a venue.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    close_price : Price
        The closing price for the instrument.
    close_type : InstrumentCloseType
        The type of closing price.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the close price event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    instrument_id: InstrumentId
    close_price: Price
    close_type: InstrumentCloseType
    ts_event: int
    ts_init: int
    def __init__(self, instrument_id: InstrumentId, close_price: Price, close_type: InstrumentCloseType, ts_event: int, ts_init: int) -> None: ...
    def __eq__(self, other: InstrumentClose) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> InstrumentClose:
        """
        Return an instrument close price event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        InstrumentClose

        """
        ...
    @staticmethod
    def to_dict(obj: InstrumentClose) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def to_pyo3_list(closes: list[InstrumentClose]) -> list[nautilus_pyo3.InstrumentClose]:
        """
        Return pyo3 Rust index prices converted from the given legacy Cython objects.

        Parameters
        ----------
        closes : list[InstrumentClose]
            The legacy Cython Rust instrument closes to convert from.

        Returns
        -------
        list[nautilus_pyo3.InstrumentClose]

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_closes: list[nautilus_pyo3.InstrumentClose]) -> list[InstrumentClose]:
        """
        Return legacy Cython instrument closes converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_closes : list[nautilus_pyo3.InstrumentClose]
            The pyo3 Rust instrument closes to convert from.

        Returns
        -------
        list[InstrumentClose]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_close: nautilus_pyo3.InstrumentClose) -> InstrumentClose:
        """
        Return a legacy Cython instrument close converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_close : nautilus_pyo3InstrumentClose.
            The pyo3 Rust instrument close to convert from.

        Returns
        -------
        InstrumentClose

        """
        ...
    def to_pyo3(self) -> nautilus_pyo3.IndexPriceUpdate:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.InstrumentClose

        """
        ...

class QuoteTick(Data):
    """
    Represents a single quote tick in a market.

    Contains information about the best top-of-book bid and ask.

    Parameters
    ----------
    instrument_id : InstrumentId
        The quotes instrument ID.
    bid_price : Price
        The top-of-book bid price.
    ask_price : Price
        The top-of-book ask price.
    bid_size : Quantity
        The top-of-book bid size.
    ask_size : Quantity
        The top-of-book ask size.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `bid.precision` != `ask.precision`.
    ValueError
        If `bid_size.precision` != `ask_size.precision`.

    """

    def __init__(self, instrument_id: InstrumentId, bid_price: Price, ask_price: Price, bid_size: Quantity, ask_size: Quantity, ts_event: int, ts_init: int) -> None: ...
    def __getstate__(self) -> tuple[str, int, int, int, int, int, int, int, int, int, int]: ...
    def __setstate__(self, state: tuple[str, int, int, int, int, int, int, int, int, int, int]) -> None: ...
    def __eq__(self, other: QuoteTick) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the tick instrument ID.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def bid_price(self) -> Price:
        """
        Return the top-of-book bid price.

        Returns
        -------
        Price

        """
        ...
    @property
    def ask_price(self) -> Price:
        """
        Return the top-of-book ask price.

        Returns
        -------
        Price

        """
        ...
    @property
    def bid_size(self) -> Quantity:
        """
        Return the top-of-book bid size.

        Returns
        -------
        Quantity

        """
        ...
    @property
    def ask_size(self) -> Quantity:
        """
        Return the top-of-book ask size.

        Returns
        -------
        Quantity

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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
    def from_raw_arrays_to_list(instrument_id: InstrumentId, price_prec: int, size_prec: int, bid_prices_raw: np.ndarray[Any, np.dtype[np.float64]], ask_prices_raw: np.ndarray[Any, np.dtype[np.float64]], bid_sizes_raw: np.ndarray[Any, np.dtype[np.float64]], ask_sizes_raw: np.ndarray[Any, np.dtype[np.float64]], ts_events: np.ndarray[Any, np.dtype[np.uint64]], ts_inits: np.ndarray[Any, np.dtype[np.uint64]]) -> list[QuoteTick]: ...
    @staticmethod
    def list_from_capsule(capsule: Any) -> list[QuoteTick]: ...
    @staticmethod
    def capsule_from_list(items: list[QuoteTick]) -> Any: ...
    @staticmethod
    def from_raw(instrument_id: InstrumentId, bid_price_raw: PriceRaw, ask_price_raw: PriceRaw, bid_price_prec: int, ask_price_prec: int, bid_size_raw: QuantityRaw, ask_size_raw: QuantityRaw, bid_size_prec: int, ask_size_prec: int, ts_event: int, ts_init: int) -> QuoteTick:
        """
        Return a quote tick from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The quotes instrument ID.
        bid_price_raw : int
            The raw top-of-book bid price (as a scaled fixed-point integer).
        ask_price_raw : int
            The raw top-of-book ask price (as a scaled fixed-point integer).
        bid_price_prec : uint8_t
            The bid price precision.
        ask_price_prec : uint8_t
            The ask price precision.
        bid_size_raw : int
            The raw top-of-book bid size (as a scaled fixed-point integer).
        ask_size_raw : int
            The raw top-of-book ask size (as a scaled fixed-point integer).
        bid_size_prec : uint8_t
            The bid size precision.
        ask_size_prec : uint8_t
            The ask size precision.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        QuoteTick

        Raises
        ------
        ValueError
            If `bid_price_prec` != `ask_price_prec`.
        ValueError
            If `bid_size_prec` != `ask_size_prec`.

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> QuoteTick:
        """
        Return a quote tick parsed from the given values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        QuoteTick

        """
        ...
    @staticmethod
    def to_dict(obj: QuoteTick) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_quotes: list[nautilus_pyo3.QuoteTick]) -> list[QuoteTick]:
        """
        Return legacy Cython quotes converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_quotes : list[nautilus_pyo3.QuoteTick]
            The pyo3 Rust quotes to convert from.

        Returns
        -------
        list[QuoteTick]

        """
        ...
    @staticmethod
    def to_pyo3_list(quotes: list[QuoteTick]) -> list[nautilus_pyo3.QuoteTick]:
        """
        Return pyo3 Rust quotes converted from the given legacy Cython objects.

        Parameters
        ----------
        quotes : list[QuoteTick]
            The legacy Cython quotes to convert from.

        Returns
        -------
        list[nautilus_pyo3.QuoteTick]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_quote: nautilus_pyo3.QuoteTick) -> QuoteTick:
        """
        Return a legacy Cython quote tick converted from the given pyo3 Rust object.

        Parameters
  # Corrected return type
        ----------
        pyo3_quote : nautilus_pyo3.QuoteTick
            The pyo3 Rust quote tick to convert from.

        Returns
        -------
        QuoteTick

        """
        ...
    def to_pyo3(self) -> nautilus_pyo3.QuoteTick:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.QuoteTick

        """
        ...
    def extract_price(self, price_type: PriceType) -> Price:
        """
        Extract the price for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type to extract.

        Returns
        -------
        Price

        """
        ...
    def extract_size(self, price_type: PriceType) -> Quantity:
        """
        Extract the size for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type to extract.

        Returns
        -------
        Quantity

        """
        ...

class TradeTick(Data):
    """
    Represents a single trade tick in a market.

    Contains information about a single unique trade which matched buyer and
    seller counterparties.

    Parameters
    ----------
    instrument_id : InstrumentId
        The trade instrument ID.
    price : Price
        The traded price.
    size : Quantity
        The traded size.
    aggressor_side : AggressorSide
        The trade aggressor side.
    trade_id : TradeId
        The trade match ID (assigned by the venue).
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the tick event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `trade_id` is not a valid string.
    ValueError
        If `size` is not positive (> 0).

    """

    def __init__(self, instrument_id: InstrumentId, price: Price, size: Quantity, aggressor_side: AggressorSide, trade_id: TradeId, ts_event: int, ts_init: int) -> None: ...
    def __getstate__(self) -> tuple[str, int, int, int, int, AggressorSide, str, int, int]: ...
    def __setstate__(self, state: tuple[str, int, int, int, int, AggressorSide, str, int, int]) -> None: ...
    def __eq__(self, other: TradeTick) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the ticks instrument ID.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def trade_id(self) -> TradeId:
        """
        Return the ticks trade match ID.

        Returns
        -------
        Price

        """
        ...
    @property
    def price(self) -> Price:
        """
        Return the ticks price.

        Returns
        -------
        Price

        """
        ...
    @property
    def size(self) -> Quantity:
        """
        Return the ticks size.

        Returns
        -------
        Quantity

        """
        ...
    @property
    def aggressor_side(self) -> AggressorSide:
        """
        Return the ticks aggressor side.

        Returns
        -------
        AggressorSide

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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
    def from_raw_arrays_to_list(
        instrument_id: InstrumentId, 
        price_prec: int, 
        size_prec: int, 
        prices_raw: np.ndarray, # skip-validate
        sizes_raw: np.ndarray, # skip-validate
        aggressor_sides: np.ndarray, # skip-validate
        trade_ids: list[str], 
        ts_events: np.ndarray, # skip-validate
        ts_inits: np.ndarray # skip-validate
    ) -> list[TradeTick]: ...
    @staticmethod
    def list_from_capsule(capsule: Any) -> list[TradeTick]: ...
    @staticmethod
    def capsule_from_list(items: list[TradeTick]) -> Any: ...
    @staticmethod
    def from_raw(instrument_id: InstrumentId, price_raw: PriceRaw, price_prec: int, size_raw: QuantityRaw, size_prec: int, aggressor_side: AggressorSide, trade_id: TradeId, ts_event: int, ts_init: int) -> TradeTick:
        """
        Return a trade tick from the given raw values.

        Parameters
        ----------
        instrument_id : InstrumentId
            The trade instrument ID.
        price_raw : int
            The traded raw price (as a scaled fixed-point integer).
        price_prec : uint8_t
            The traded price precision.
        size_raw : int
            The traded raw size (as a scaled fixed-point integer).
        size_prec : uint8_t
            The traded size precision.
        aggressor_side : AggressorSide
            The trade aggressor side.
        trade_id : TradeId
            The trade match ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the tick event occurred.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the data object was initialized.

        Returns
        -------
        TradeTick

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> TradeTick:
        """
        Return a trade tick from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        TradeTick

        """
        ...
    @staticmethod
    def to_dict(obj: TradeTick) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def to_pyo3_list(trades: list[TradeTick]) -> list[nautilus_pyo3.TradeTick]:
        """
        Return pyo3 Rust trades converted from the given legacy Cython objects.

        Parameters
        ----------
        ticks : list[TradeTick]
            The legacy Cython Rust trades to convert from.

        Returns
        -------
        list[nautilus_pyo3.TradeTick]

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_trades: list[nautilus_pyo3.TradeTick]) -> list[TradeTick]:
        """
        Return legacy Cython trades converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_trades : list[nautilus_pyo3.TradeTick]
            The pyo3 Rust trades to convert from.

        Returns
        -------
        list[TradeTick]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_trade: nautilus_pyo3.TradeTick) -> TradeTick:
        """
        Return a legacy Cython trade tick converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_trade : nautilus_pyo3.TradeTick
            The pyo3 Rust trade tick to convert from.

        Returns
        -------
        TradeTick

        """
        ...
    def to_pyo3(self) -> nautilus_pyo3.TradeTick:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.TradeTick

        """
        ...

class MarkPriceUpdate(Data):
    """
    Represents a mark price update.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the mark price.
    value : Price
        The mark price.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the update occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    """

    def __init__(self, instrument_id: InstrumentId, value: Price, ts_event: int, ts_init: int) -> None: ...
    def __eq__(self, other: MarkPriceUpdate) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the instrument ID.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def value(self) -> Price:
        """
        The mark price.

        Returns
        -------
        Price

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

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
    def from_dict(values: dict[str, Any]) -> MarkPriceUpdate:
        """
        Return a mark price from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        MarkPriceUpdate

        """
        ...
    @staticmethod
    def to_dict(obj: MarkPriceUpdate) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def to_pyo3_list(mark_prices: list[MarkPriceUpdate]) -> list[nautilus_pyo3.MarkPriceUpdate]:
        """
        Return pyo3 Rust mark prices converted from the given legacy Cython objects.

        Parameters
        ----------
        mark_prices : list[MarkPriceUpdate]
            The legacy Cython Rust mark prices to convert from.

        Returns
        -------
        list[nautilus_pyo3.MarkPriceUpdate]

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_mark_prices: list[nautilus_pyo3.MarkPriceUpdate]) -> list[MarkPriceUpdate]:
        """
        Return legacy Cython trades converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_mark_prices : list[nautilus_pyo3.MarkPriceUpdate]
            The pyo3 Rust mark prices to convert from.

        Returns
        -------
        list[MarkPriceUpdate]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_mark_price: nautilus_pyo3.MarkPriceUpdate) -> MarkPriceUpdate:
        """
        Return a legacy Cython mark price converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_trade : nautilus_pyo3.MarkPriceUpdate
            The pyo3 Rust mark price to convert from.

        Returns
        -------
        MarkPriceUpdate

        """
        ...
    def to_pyo3(self) -> nautilus_pyo3.MarkPriceUpdate:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.MarkPriceUpdate

        """
        ...

class IndexPriceUpdate(Data):
    """
    Represents an index price update.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the index price.
    value : Price
        The index price.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the update occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    """

    instrument_id: InstrumentId
    value: Price
    ts_event: int
    ts_init: int
    def __init__(self, instrument_id: InstrumentId, value: Price, ts_event: int, ts_init: int) -> None: ...
    def __eq__(self, other: IndexPriceUpdate) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> IndexPriceUpdate:
        """
        Return an index price from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        IndexPriceUpdate

        """
        ...
    @staticmethod
    def to_dict(obj: IndexPriceUpdate) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def to_pyo3_list(index_prices: list[IndexPriceUpdate]) -> list[nautilus_pyo3.IndexPriceUpdate]:
        """
        Return pyo3 Rust index prices converted from the given legacy Cython objects.

        Parameters
        ----------
        mark_prices : list[IndexPriceUpdate]
            The legacy Cython Rust index prices to convert from.

        Returns
        -------
        list[nautilus_pyo3.IndexPriceUpdate]

        """
        ...
    @staticmethod
    def from_pyo3_list(pyo3_index_prices: list[nautilus_pyo3.IndexPriceUpdate]) -> list[IndexPriceUpdate]:
        """
        Return legacy Cython index prices converted from the given pyo3 Rust objects.

        Parameters
        ----------
        pyo3_index_prices : list[nautilus_pyo3.IndexPriceUpdate]
            The pyo3 Rust index prices to convert from.

        Returns
        -------
        list[IndexPriceUpdate]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_index_price: nautilus_pyo3.IndexPriceUpdate) -> IndexPriceUpdate:
        """
        Return a legacy Cython index price converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_trade : nautilus_pyo3.IndexPriceUpdate
            The pyo3 Rust index price to convert from.

        Returns
        -------
        IndexPriceUpdate

        """
        ...
    def to_pyo3(self) -> nautilus_pyo3.IndexPriceUpdate:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.IndexPriceUpdate

        """
        ...
