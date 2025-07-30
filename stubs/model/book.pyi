from nautilus_trader.core.nautilus_pyo3 import BookAction
from nautilus_trader.core.nautilus_pyo3 import BookOrder
from nautilus_trader.core.nautilus_pyo3 import BookType
from nautilus_trader.core.nautilus_pyo3 import Data
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Order
from nautilus_trader.core.nautilus_pyo3 import OrderBookDelta
from nautilus_trader.core.nautilus_pyo3 import OrderBookDeltas
from nautilus_trader.core.nautilus_pyo3 import OrderBookDepth10
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.core.nautilus_pyo3 import OrderStatus


class OrderBook(Data):
    """
    Provides an order book which can handle L1/L2/L3 granularity data.

    Parameters
    ----------
    instrument_id : IntrumentId
        The instrument ID for the order book.
    book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
        The order book type.

    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __getstate__(self): ...
    def __setstate__(self, state): ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the books instrument ID.

        Returns
        -------
        InstrumentId

        """
        ...
    @property
    def book_type(self) -> BookType:
        """
        Return the order book type.

        Returns
        -------
        BookType

        """
        ...
    @property
    def sequence(self) -> int:
        """
        Return the last sequence number for the book.

        Returns
        -------
        int

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
    @property
    def ts_last(self) -> int:
        """
        Return the UNIX timestamp (nanoseconds) when the order book was last updated.

        Returns
        -------
        int

        """
        ...
    @property
    def update_count(self) -> int:
        """
        Return the books update count.

        Returns
        -------
        int

        """
        ...
    def reset(self) -> None:
        """
        Reset the order book (clear all stateful values).
        """
        ...
    def add(self, order: BookOrder, ts_event: int, flags: int = 0, sequence: int = 0) -> None:
        """
        Add the given order to the book.

        Parameters
        ----------
        order : BookOrder
            The order to add.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the book event occurred.
        flags : uint8_t, default 0
            The record flags bit field, indicating event end and data information.
        sequence : uint64_t, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        Raises
        ------
        RuntimeError
            If the book type is L1_MBP.

        """
        ...
    def update(self, order: BookOrder, ts_event: int, flags: int = 0, sequence: int = 0) -> None:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the book event occurred.
        flags : uint8_t, default 0
            The record flags bit field, indicating event end and data information.
        sequence : uint64_t, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        ...
    def delete(self, order: BookOrder, ts_event: int, flags: int = 0, sequence: int = 0) -> None:
        """
        Cancel the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the book event occurred.
        flags : uint8_t, default 0
            The record flags bit field, indicating event end and data information.
        sequence : uint64_t, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        ...
    def clear(self, ts_event: int, sequence: int = 0) -> None:
        """
        Clear the entire order book.
        """
        ...
    def clear_bids(self, ts_event: int, sequence: int = 0) -> None:
        """
        Clear the bids from the order book.
        """
        ...
    def clear_asks(self, ts_event: int, sequence: int = 0) -> None:
        """
        Clear the asks from the order book.
        """
        ...
    def apply_delta(self, delta: OrderBookDelta) -> None:
        """
        Apply the order book delta.

        Parameters
        ----------
        delta : OrderBookDelta
            The delta to apply.

        Raises
        ------
        ValueError
            If `delta.book_type` is not equal to `self.type`.

        """
        ...
    def apply_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Apply the bulk deltas to the order book.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The deltas to apply.

        """
        ...
    def apply_depth(self, depth: OrderBookDepth10) -> None:
        """
        Apply the depth update to the order book.

        Parameters
        ----------
        depth : OrderBookDepth10
            The depth update to apply.

        """
        ...
    def apply(self, data: Data) -> None:
        """
        Apply the given data to the order book.

        Parameters
        ----------
        delta : OrderBookDelta, OrderBookDeltas
            The data to apply.

        """
        ...
    def check_integrity(self) -> None:
        """
        Check book integrity.

        For all order books:
        - The bid side price should not be greater than the ask side price.

        Raises
        ------
        RuntimeError
            If book integrity check fails.

        """
        ...
    def bids(self) -> list[BookLevel]:
        """
        Return the bid levels for the order book.

        Returns
        -------
        list[BookLevel]
            Sorted in descending order of price.

        """
        ...
    def asks(self) -> list[BookLevel]:
        """
        Return the bid levels for the order book.

        Returns
        -------
        list[BookLevel]
            Sorted in ascending order of price.

        """
        ...
    def best_bid_price(self) -> Price | None:
        """
        Return the best bid price in the book (if no bids then returns ``None``).

        Returns
        -------
        double

        """
        ...
    def best_ask_price(self) -> Price | None:
        """
        Return the best ask price in the book (if no asks then returns ``None``).

        Returns
        -------
        double

        """
        ...
    def best_bid_size(self) -> Quantity | None:
        """
        Return the best bid size in the book (if no bids then returns ``None``).

        Returns
        -------
        double

        """
        ...
    def best_ask_size(self) -> Quantity | None:
        """
        Return the best ask size in the book (if no asks then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        ...
    def spread(self) -> float | None:
        """
        Return the top-of-book spread (if no bids or asks then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        ...
    def midpoint(self) -> float | None:
        """
        Return the mid point (if no market exists then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        ...
    def get_avg_px_for_quantity(self, quantity: Quantity, order_side: OrderSide) -> float:
        """
        Return the average price expected for the given `quantity` based on the current state
        of the order book.

        Parameters
        ----------
        quantity : Quantity
            The quantity for the calculation.
        order_side : OrderSide
            The order side for the calculation.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If `order_side` is equal to ``NO_ORDER_SIDE``

        Warnings
        --------
        If no average price can be calculated then will return 0.0 (zero).

        """
        ...
    def get_quantity_for_price(self, price: Price, order_side: OrderSide) -> float:
        """
        Return the current total quantity for the given `price` based on the current state
        of the order book.

        Parameters
        ----------
        price : Price
            The quantity for the calculation.
        order_side : OrderSide
            The order side for the calculation.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If `order_side` is equal to ``NO_ORDER_SIDE``

        """
        ...
    def simulate_fills(self, order: Order, price_prec: int, size_prec: int, is_aggressive: bool) -> list[tuple[Price, Quantity]]:
        """
        Simulate filling the book with the given order.

        Parameters
        ----------
        order : Order
            The order to simulate fills for.
        price_prec : uint8_t
            The price precision for the fills.
        size_prec : uint8_t
            The size precision for the fills (based on the instrument definition).
        is_aggressive : bool
            If the order is an aggressive liquidity taking order.

        """
        ...
    def update_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the order book with the given quote tick.

        This operation is only valid for ``L1_MBP`` books maintaining a top level.

        Parameters
        ----------
        tick : QuoteTick
            The quote tick to update with.

        Raises
        ------
        RuntimeError
            If `book_type` is not ``L1_MBP``.

        """
        ...
    def update_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the order book with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The trade tick to update with.

        Raises
        ------
        RuntimeError
            If `book_type` is not ``L1_MBP``.

        """
        ...
    def pprint(self, num_levels: int = 3) -> str:
        """
        Return a string representation of the order book in a human-readable table format.

        Parameters
        ----------
        num_levels : int
            The number of levels to include.

        Returns
        -------
        str

        """
        ...

class BookLevel:
    """
    Represents an order book price level.

    A price level on one side of the order book with one or more individual orders.

    This class is read-only and cannot be initialized from Python.

    Parameters
    ----------
    price : Price
        The price for the level.
    orders : list[BookOrder]
        The orders for the level.

    Raises
    ------
    ValueError
        If `orders` is empty.
    """

    def __del__(self) -> None: ...
    def __eq__(self, other: BookLevel) -> bool: ...
    def __lt__(self, other: BookLevel) -> bool: ...
    def __le__(self, other: BookLevel) -> bool: ...
    def __gt__(self, other: BookLevel) -> bool: ...
    def __ge__(self, other: BookLevel) -> bool: ...
    def __repr__(self) -> str: ...
    @property
    def side(self) -> OrderSide:
        """
        Return the side for the level.

        Returns
        -------
        OrderSide

        """
        ...
    @property
    def price(self) -> Price:
        """
        Return the price for the level.

        Returns
        -------
        Price

        """
        ...
    def orders(self) -> list[BookOrder]:
        """
        Return the orders for the level.

        Returns
        -------
        list[BookOrder]

        """
        ...
    def size(self) -> float:
        """
        Return the size at this level.

        Returns
        -------
        double

        """
        ...
    def exposure(self) -> float:
        """
        Return the exposure at this level (price * volume).

        Returns
        -------
        double

        """
        ...

def py_should_handle_own_book_order(order: Order) -> bool: ...