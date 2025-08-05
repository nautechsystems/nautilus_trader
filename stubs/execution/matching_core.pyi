from collections.abc import Callable

from nautilus_trader.model.enums import OrderSide
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import InstrumentId
from stubs.model.objects import Price
from stubs.model.orders.base import Order

class MatchingCore:
    """
    Provides a generic order matching core.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the matching core.
    price_increment : Price
        The minimum price increment (tick size) for the matching core.
    trigger_stop_order : Callable[[Order], None]
        The callable when a stop order is triggered.
    fill_market_order : Callable[[Order], None]
        The callable when a market order is filled.
    fill_limit_order : Callable[[Order], None]
        The callable when a limit order is filled.
    """

    bid_raw: int
    ask_raw: int
    last_raw: int
    is_bid_initialized: bool
    is_ask_initialized: bool
    is_last_initialized: bool

    _instrument_id: InstrumentId
    _price_increment: Price
    _price_precision: int
    _trigger_stop_order = Callable
    _fill_market_order = Callable
    _fill_limit_order = Callable
    _orders: dict[ClientOrderId, Order]
    _orders_bid: list[Order]
    _orders_ask: list[Order]

    def __init__(
        self,
        instrument_id: InstrumentId,
        price_increment: Price,
        trigger_stop_order: Callable,
        fill_market_order: Callable,
        fill_limit_order: Callable,
    ) -> None: ...
    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the instrument ID for the matching core.

        Returns
        -------
        InstrumentId

        """
    @property
    def price_precision(self) -> int:
        """
        Return the instruments price precision for the matching core.

        Returns
        -------
        int

        """
    @property
    def price_increment(self) -> Price:
        """
        Return the instruments minimum price increment (tick size) for the matching core.

        Returns
        -------
        Price

        """
    @property
    def bid(self) -> Price | None:
        """
        Return the current bid price for the matching core.

        Returns
        -------
        Price or ``None``

        """
    @property
    def ask(self) -> Price | None:
        """
        Return the current ask price for the matching core.

        Returns
        -------
        Price or ``None``

        """
    @property
    def last(self) -> Price | None:
        """
        Return the current last price for the matching core.

        Returns
        -------
        Price or ``None``

        """
    def get_order(self, client_order_id: ClientOrderId) -> Order: ...
    def order_exists(self, client_order_id: ClientOrderId) -> bool: ...
    def get_orders(self) -> list[Order]: ...
    def get_orders_bid(self) -> list[Order]: ...
    def get_orders_ask(self) -> list[Order]: ...
    def reset(self) -> None: ...
    def add_order(self, order: Order) -> None: ...
    def delete_order(self, order: Order) -> None: ...
    def iterate(self, timestamp_ns: int) -> None: ...
    def match_order(self, order: Order, initial: bool = False) -> None:
        """
        Match the given order.

        Parameters
        ----------
        order : Order
            The order to match.
        initial : bool, default False
            If this is an initial match.

        Raises
        ------
        TypeError
            If the `order.order_type` is an invalid type for the core (e.g. `MARKET`).

        """
    def match_limit_order(self, order: Order) -> None: ...
    def match_stop_market_order(self, order: Order) -> None: ...
    def match_stop_limit_order(self, order: Order, initial: bool) -> None: ...
    def match_market_if_touched_order(self, order: Order) -> None: ...
    def match_limit_if_touched_order(self, order: Order, initial: bool) -> None: ...
    def match_trailing_stop_limit_order(self, order: Order, initial: bool) -> None: ...
    def match_trailing_stop_market_order(self, order: Order) -> None: ...
    def is_limit_matched(self, side: OrderSide, price: Price) -> bool: ...
    def is_stop_triggered(self, side: OrderSide, trigger_price: Price) -> bool: ...
    def is_touch_triggered(self, side: OrderSide, trigger_price: Price) -> bool: ...
