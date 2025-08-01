from datetime import timedelta

from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import AccountType
from nautilus_trader.core.nautilus_pyo3 import AggressorSide
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import BatchCancelOrders
from nautilus_trader.core.nautilus_pyo3 import BookType
from nautilus_trader.core.nautilus_pyo3 import CancelAllOrders
from nautilus_trader.core.nautilus_pyo3 import CancelOrder
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import FeeModel
from nautilus_trader.core.nautilus_pyo3 import FillModel
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import InstrumentClose
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import MarketStatus
from nautilus_trader.core.nautilus_pyo3 import MarketStatusAction
from nautilus_trader.core.nautilus_pyo3 import MessageBus
from nautilus_trader.core.nautilus_pyo3 import ModifyOrder
from nautilus_trader.core.nautilus_pyo3 import OmsType
from nautilus_trader.core.nautilus_pyo3 import Order
from nautilus_trader.core.nautilus_pyo3 import OrderBook
from nautilus_trader.core.nautilus_pyo3 import OrderBookDelta
from nautilus_trader.core.nautilus_pyo3 import OrderBookDeltas
from nautilus_trader.core.nautilus_pyo3 import OrderBookDepth10
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TestClock
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.core.nautilus_pyo3 import Venue
from stubs.cache.base import CacheFacade
from stubs.common.component import Logger
from stubs.execution.matching_core import MatchingCore

class OrderMatchingEngine:
    """
    Provides an order matching engine for a single market.

    Parameters
    ----------
    instrument : Instrument
        The market instrument for the matching engine.
    raw_id : uint32_t
        The raw integer ID for the instrument.
    fill_model : FillModel
        The fill model for the matching engine.
    fee_model : FeeModel
        The fee model for the matching engine.
    book_type : BookType
        The order book type for the engine.
    oms_type : OmsType
        The order management system type for the matching engine. Determines
        the generation and handling of venue position IDs.
    account_type : AccountType
        The account type for the matching engine. Determines allowable
        executions based on the instrument.
    msgbus : MessageBus
        The message bus for the matching engine.
    cache : CacheFacade
        The read-only cache for the matching engine.
    clock : TestClock
        The clock for the matching engine.
    logger : Logger
        The logger for the matching engine.
    bar_execution : bool, default True
        If bars should be processed by the matching engine (and move the market).
    trade_execution : bool, default False
        If trades should be processed by the matching engine (and move the market).
    reject_stop_orders : bool, default True
        If stop orders are rejected if already in the market on submitting.
    support_gtd_orders : bool, default True
        If orders with GTD time in force will be supported by the venue.
    support_contingent_orders : bool, default True
        If contingent orders will be supported/respected by the venue.
        If False, then its expected the strategy will be managing any contingent orders.
    use_position_ids : bool, default True
        If venue position IDs will be generated on order fills.
    use_random_ids : bool, default False
        If all venue generated identifiers will be random UUID4's.
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders will be honored.
    auction_match_algo : Callable[[Ladder, Ladder], Tuple[List, List], optional
        The auction matching algorithm.
    bar_adaptive_high_low_ordering : bool, default False
        Determines whether the processing order of bar prices is adaptive based on a heuristic.
        This setting is only relevant when `bar_execution` is True.
        If False, bar prices are always processed in the fixed order: Open, High, Low, Close.
        If True, the processing order adapts with the heuristic:
        - If High is closer to Open than Low then the processing order is Open, High, Low, Close.
        - If Low is closer to Open than High then the processing order is Open, Low, High, Close.

    """

    msgbus: MessageBus
    cache: CacheFacade
    venue: Venue
    instrument: Instrument
    raw_id: int
    book_type: BookType
    oms_type: OmsType
    account_type: AccountType
    market_status: MarketStatus

    _clock: TestClock
    _log: Logger
    _instrument_has_expiration: bool
    _instrument_close: InstrumentClose | None
    _reject_stop_orders: bool
    _support_gtd_orders: bool
    _support_contingent_orders: bool
    _use_position_ids: bool
    _use_random_ids: bool
    _use_reduce_only: bool
    _bar_execution: bool
    _bar_adaptive_high_low_ordering: bool
    _trade_execution: bool
    _fill_model: FillModel
    _fee_model: FeeModel
    _book: OrderBook
    _opening_auction_book: OrderBook
    _closing_auction_book: OrderBook
    _account_ids: dict[TraderId, AccountId]
    _execution_bar_types: dict[InstrumentId, BarType]
    _execution_bar_deltas: dict[BarType, timedelta]
    _cached_filled_qty: dict[ClientOrderId, Quantity]
    _core: MatchingCore
    _target_bid: int
    _target_ask: int
    _target_last: int
    _has_targets: bool
    _last_bid_bar: Bar | None
    _last_ask_bar: Bar | None
    _position_count: int
    _order_count: int
    _execution_count: int

    def __init__(
        self,
        instrument: Instrument,
        raw_id: int,
        fill_model: FillModel,
        fee_model: FeeModel,
        book_type: BookType,
        oms_type: OmsType,
        account_type: AccountType,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: TestClock,
        reject_stop_orders: bool = True,
        support_gtd_orders: bool = True,
        support_contingent_orders: bool = True,
        use_position_ids: bool = True,
        use_random_ids: bool = False,
        use_reduce_only: bool = True,
        bar_execution: bool = True,
        bar_adaptive_high_low_ordering: bool = False,
        trade_execution: bool = False,
        # auction_match_algo = default_auction_match
    ) -> None: ...
    def __repr__(self) -> str: ...
    def reset(self) -> None: ...
    def set_fill_model(self, fill_model: FillModel) -> None:
        """
        Set the fill model to the given model.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        ...
    def update_instrument(self, instrument: Instrument) -> None:
        """
        Update the matching engines current instrument definition with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument definition to update.

        """
        ...
    def best_bid_price(self) -> Price:
        """
        Return the best bid price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        ...
    def best_ask_price(self) -> Price:
        """
        Return the best ask price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        ...
    def get_book(self) -> OrderBook:
        """
        Return the internal order book.

        Returns
        -------
        OrderBook

        """
        ...
    def get_open_orders(self) -> list[Order]:
        """
        Return the open orders in the matching engine.

        Returns
        -------
        list[Order]

        """
        ...
    def get_open_bid_orders(self) -> list[Order]:
        """
        Return the open bid orders in the matching engine.

        Returns
        -------
        list[Order]

        """
        ...
    def get_open_ask_orders(self) -> list[Order]:
        """
        Return the open ask orders at the exchange.

        Returns
        -------
        list[Order]

        """
        ...
    def order_exists(self, client_order_id: ClientOrderId) -> bool: ...
    def process_order_book_delta(self, delta: OrderBookDelta) -> None:
        """
        Process the exchanges market for the given order book delta.

        Parameters
        ----------
        delta : OrderBookDelta
            The order book delta to process.

        """
        ...
    def process_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Process the exchanges market for the given order book deltas.

        Parameters
        ----------
        delta : OrderBookDeltas
            The order book deltas to process.

        """
        ...
    def process_order_book_depth10(self, depth: OrderBookDepth10) -> None:
        """
        Process the exchanges market for the given order book depth.

        Parameters
        ----------
        depth : OrderBookDepth10
            The order book depth to process.

        """
        ...
    def process_quote_tick(self, tick: QuoteTick) -> None:
        """
        Process the exchanges market for the given quote tick.

        The internal order book will only be updated if the venue `book_type` is 'L1_MBP'.

        Parameters
        ----------
        tick : QuoteTick
            The tick to process.

        Raises
        ------
        RuntimeError
            If a price precision does not match the instrument for the matching engine.
        RuntimeError
            If a size precision does not match the instrument for the matching engine.

        """
        ...
    def process_trade_tick(self, tick: TradeTick) -> None:
        """
        Process the exchanges market for the given trade tick.

        The internal order book will only be updated if the venue `book_type` is 'L1_MBP'.

        Parameters
        ----------
        tick : TradeTick
            The tick to process.

        Raises
        ------
        RuntimeError
            If the trades price precision does not match the instrument for the matching engine.
        RuntimeError
            If the trades size precision does not match the instrument for the matching engine.

        """
        ...
    def process_bar(self, bar: Bar) -> None:
        """
        Process the exchanges market for the given bar.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        Raises
        ------
        RuntimeError
            If a price precision does not match the instrument for the matching engine.
        RuntimeError
            If a size precision does not match the instrument for the matching engine.

        """
        ...
    def process_status(self, status: MarketStatusAction) -> None:
        """
        Process the exchange status.

        Parameters
        ----------
        status : MarketStatusAction
            The status action to process.

        """
        ...
    def process_instrument_close(self, close: InstrumentClose) -> None:
        """
        Process the instrument close.

        Parameters
        ----------
        close : InstrumentClose
            The close price to process.

        """
        ...
    def process_auction_book(self, book: OrderBook) -> None: ...
    def process_order(self, order: Order, account_id: AccountId) -> None: ...
    def process_modify(self, command: ModifyOrder, account_id: AccountId) -> None: ...
    def process_cancel(self, command: CancelOrder, account_id: AccountId) -> None: ...
    def process_batch_cancel(self, command: BatchCancelOrders, account_id: AccountId) -> None: ...
    def process_cancel_all(self, command: CancelAllOrders, account_id: AccountId) -> None: ...
    def iterate(self, timestamp_ns: int, aggressor_side: AggressorSide = AggressorSide.NO_AGGRESSOR) -> None:
        """
        Iterate the matching engine by processing the bid and ask order sides
        and advancing time up to the given UNIX `timestamp_ns`.

        Parameters
        ----------
        timestamp_ns : uint64_t
            UNIX timestamp to advance the matching engine time to.
        aggressor_side : AggressorSide, default 'NO_AGGRESSOR'
            The aggressor side for trade execution processing.

        """
        ...
    def determine_limit_price_and_volume(self, order: Order) -> list[tuple[Price, Quantity]]:
        """
        Return the projected fills for the given *limit* order filling passively
        from its limit price.

        The list may be empty if no fills.

        Parameters
        ----------
        order : Order
            The order to determine fills for.

        Returns
        -------
        list[tuple[Price, Quantity]]

        Raises
        ------
        ValueError
            If the `order` does not have a LIMIT `price`.

        """
        ...
    def determine_market_price_and_volume(self, order: Order) -> list[tuple[Price, Quantity]]:
        """
        Return the projected fills for the given *marketable* order filling
        aggressively into the opposite order side.

        The list may be empty if no fills.

        Parameters
        ----------
        order : Order
            The order to determine fills for.

        Returns
        -------
        list[tuple[Price, Quantity]]

        """
        ...
    def fill_market_order(self, order: Order) -> None:
        """
        Fill the given *marketable* order.

        Parameters
        ----------
        order : Order
            The order to fill.

        """
        ...
    def fill_limit_order(self, order: Order) -> None:
        """
        Fill the given limit order.

        Parameters
        ----------
        order : Order
            The order to fill.

        Raises
        ------
        ValueError
            If the `order` does not have a LIMIT `price`.

        """
        ...
    def apply_fills(
        self,
        order: Order,
        fills: list,
        liquidity_side: LiquiditySide,
        venue_position_id: PositionId | None = None,
        position: Position | None = None,
    ) -> None:
        """
        Apply the given list of fills to the given order. Optionally provide
        existing position details.

        - If the `fills` list is empty, an error will be logged.
        - Market orders will be rejected if no opposing orders are available to fulfill them.

        Parameters
        ----------
        order : Order
            The order to fill.
        fills : list[tuple[Price, Quantity]]
            The fills to apply to the order.
        liquidity_side : LiquiditySide
            The liquidity side for the fill(s).
        venue_position_id :  PositionId, optional
            The current venue position ID related to the order (if assigned).
        position : Position, optional
            The current position related to the order (if any).

        Raises
        ------
        ValueError
            If `liquidity_side` is ``NO_LIQUIDITY_SIDE``.

        Warnings
        --------
        The `liquidity_side` will override anything previously set on the order.

        """
        ...
    def fill_order(self, order: Order, last_px: Price, last_qty: Quantity, liquidity_side: LiquiditySide, venue_position_id: PositionId | None = None, position: Position | None = None) -> None:
        """
        Apply the given list of fills to the given order. Optionally provide
        existing position details.

        Parameters
        ----------
        order : Order
            The order to fill.
        last_px : Price
            The fill price for the order.
        last_qty : Quantity
            The fill quantity for the order.
        liquidity_side : LiquiditySide
            The liquidity side for the fill.
        venue_position_id :  PositionId, optional
            The current venue position ID related to the order (if assigned).
        position : Position, optional
            The current position related to the order (if any).

        Raises
        ------
        ValueError
            If `liquidity_side` is ``NO_LIQUIDITY_SIDE``.

        Warnings
        --------
        The `liquidity_side` will override anything previously set on the order.

        """
        ...
    def accept_order(self, order: Order) -> None: ...
    def expire_order(self, order: Order) -> None: ...
    def cancel_order(self, order: Order, cancel_contingencies: bool = True) -> None: ...
    def update_order(self, order: Order, qty: Quantity, price: Price | None = None, trigger_price: Price | None = None, update_contingencies: bool = True) -> None: ...
    def trigger_stop_order(self, order: Order) -> None: ...
    def _generate_order_updated(
        self,
        order: Order,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
    ) -> None: ...

