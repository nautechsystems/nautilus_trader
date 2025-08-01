from nautilus_trader.core.nautilus_pyo3 import Account
from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithmId
from nautilus_trader.core.nautilus_pyo3 import IndexPriceUpdate
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import MarkPriceUpdate
from nautilus_trader.core.nautilus_pyo3 import Order
from nautilus_trader.core.nautilus_pyo3 import OrderBook
from nautilus_trader.core.nautilus_pyo3 import OrderList
from nautilus_trader.core.nautilus_pyo3 import OrderListId
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderStatus
from nautilus_trader.core.nautilus_pyo3 import OwnOrderBook
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import SyntheticInstrument
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId
from decimal import Decimal
from typing import Any
from typing import Set

class CacheFacade:
    """
    Provides a read-only facade for the common `Cache`.
    """

    def get(self, key: str) -> bytes:
        """Abstract method (implement in subclass)."""
    def add(self, key: str, value: bytes) -> None:
        """Abstract method (implement in subclass)."""
    def quote_ticks(self, instrument_id: InstrumentId) -> list[QuoteTick]:
        """Abstract method (implement in subclass)."""
    def trade_ticks(self, instrument_id: InstrumentId) -> list[TradeTick]:
        """Abstract method (implement in subclass)."""
    def mark_prices(self, instrument_id: InstrumentId) -> list[MarkPriceUpdate]:
        """Abstract method (implement in subclass)."""
    def index_prices(self, instrument_id: InstrumentId) -> list[IndexPriceUpdate]:
        """Abstract method (implement in subclass)."""
    def bars(self, bar_type: BarType) -> list[Bar]:
        """Abstract method (implement in subclass)."""
    def price(self, instrument_id: InstrumentId, price_type: PriceType) -> Price:
        """Abstract method (implement in subclass)."""
    def prices(self, price_type: PriceType) -> dict[InstrumentId, Price]:
        """Abstract method (implement in subclass)."""
    def order_book(self, instrument_id: InstrumentId) -> OrderBook:
        """Abstract method (implement in subclass)."""
    def own_order_book(self, instrument_id: InstrumentId) -> OwnOrderBook:
        """Abstract method (implement in subclass)."""
    def own_bid_orders(self, instrument_id: InstrumentId, status: Set[OrderStatus] | None = None, accepted_buffer_ns: int = 0, ts_now: int = 0) -> dict[Decimal, list[Order]]:
        """Abstract method (implement in subclass)."""
    def own_ask_orders(self, instrument_id: InstrumentId, status: Set[OrderStatus] | None = None, accepted_buffer_ns: int = 0, ts_now: int = 0) -> dict[Decimal, list[Order]]:
        """Abstract method (implement in subclass)."""
    def quote_tick(self, instrument_id: InstrumentId, index: int = 0) -> QuoteTick:
        """Abstract method (implement in subclass)."""
    def trade_tick(self, instrument_id: InstrumentId, index: int = 0) -> TradeTick:
        """Abstract method (implement in subclass)."""
    def mark_price(self, instrument_id: InstrumentId, index: int = 0) -> MarkPriceUpdate:
        """Abstract method (implement in subclass)."""
    def index_price(self, instrument_id: InstrumentId, index: int = 0) -> IndexPriceUpdate:
        """Abstract method (implement in subclass)."""
    def bar(self, bar_type: BarType, index: int = 0) -> Bar:
        """Abstract method (implement in subclass)."""
    def book_update_count(self, instrument_id: InstrumentId) -> int:
        """Abstract method (implement in subclass)."""
    def quote_tick_count(self, instrument_id: InstrumentId) -> int:
        """Abstract method (implement in subclass)."""
    def trade_tick_count(self, instrument_id: InstrumentId) -> int:
        """Abstract method (implement in subclass)."""
    def mark_price_count(self, instrument_id: InstrumentId) -> int:
        """Abstract method (implement in subclass)."""
    def index_price_count(self, instrument_id: InstrumentId) -> int:
        """Abstract method (implement in subclass)."""
    def bar_count(self, bar_type: BarType) -> int:
        """Abstract method (implement in subclass)."""
    def has_order_book(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def has_quote_ticks(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def has_trade_ticks(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def has_mark_prices(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def has_index_prices(self, instrument_id: InstrumentId) -> bool:
        """Abstract method (implement in subclass)."""
    def has_bars(self, bar_type: BarType) -> bool:
        """Abstract method (implement in subclass)."""
    def get_xrate(self, venue: Venue, from_currency: Currency, to_currency: Currency, price_type: PriceType = PriceType.MID) -> Any:
        """Abstract method (implement in subclass)."""
    def get_mark_xrate(self, from_currency: Currency, to_currency: Currency) -> Any:
        """Abstract method (implement in subclass)."""
    def set_mark_xrate(self, from_currency: Currency, to_currency: Currency, xrate: float) -> None:
        """Abstract method (implement in subclass)."""
    def clear_mark_xrate(self, from_currency: Currency, to_currency: Currency) -> None:
        """Abstract method (implement in subclass)."""
    def clear_mark_xrates(self) -> None:
        """Abstract method (implement in subclass)."""
    def instrument(self, instrument_id: InstrumentId) -> Instrument:
        """Abstract method (implement in subclass)."""
    def instrument_ids(self, venue: Venue | None = None) -> list[InstrumentId]:
        """Abstract method (implement in subclass)."""
    def instruments(self, venue: Venue | None = None, underlying: str | None = None) -> list[Instrument]:
        """Abstract method (implement in subclass)."""
    def synthetic(self, instrument_id: InstrumentId) -> SyntheticInstrument:
        """Abstract method (implement in subclass)."""
    def synthetic_ids(self) -> list[InstrumentId]:
        """Abstract method (implement in subclass)."""
    def synthetics(self) -> list[SyntheticInstrument]:
        """Abstract method (implement in subclass)."""
    def account(self, account_id: AccountId) -> Account:
        """Abstract method (implement in subclass)."""
    def set_specific_venue(self, venue: Venue) -> None:
        """Abstract method (implement in subclass)."""
    def account_for_venue(self, venue: Venue) -> Account:
        """Abstract method (implement in subclass)."""
    def account_id(self, venue: Venue) -> AccountId:
        """Abstract method (implement in subclass)."""
    def accounts(self) -> list[Account]:
        """Abstract method (implement in subclass)."""
    def client_order_ids(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def client_order_ids_open(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def client_order_ids_closed(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def client_order_ids_emulated(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def client_order_ids_inflight(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def order_list_ids(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def position_ids(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def position_open_ids(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def position_closed_ids(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> Set:
        """Abstract method (implement in subclass)."""
    def actor_ids(self) -> Set:
        """Abstract method (implement in subclass)."""
    def strategy_ids(self) -> Set:
        """Abstract method (implement in subclass)."""
    def exec_algorithm_ids(self) -> Set:
        """Abstract method (implement in subclass)."""
    def order(self, client_order_id: ClientOrderId) -> Order:
        """Abstract method (implement in subclass)."""
    def client_order_id(self, venue_order_id: VenueOrderId) -> ClientOrderId:
        """Abstract method (implement in subclass)."""
    def venue_order_id(self, client_order_id: ClientOrderId) -> VenueOrderId:
        """Abstract method (implement in subclass)."""
    def client_id(self, client_order_id: ClientOrderId) -> ClientId:
        """Abstract method (implement in subclass)."""
    def orders(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def orders_open(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def orders_closed(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def orders_emulated(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def orders_inflight(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def orders_for_position(self, position_id: PositionId) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def order_exists(self, client_order_id: ClientOrderId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_order_open(self, client_order_id: ClientOrderId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_order_closed(self, client_order_id: ClientOrderId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_order_emulated(self, client_order_id: ClientOrderId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_order_inflight(self, client_order_id: ClientOrderId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_order_pending_cancel_local(self, client_order_id: ClientOrderId) -> bool:
        """Abstract method (implement in subclass)."""
    def orders_open_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> int:
        """Abstract method (implement in subclass)."""
    def orders_closed_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> int:
        """Abstract method (implement in subclass)."""
    def orders_emulated_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> int:
        """Abstract method (implement in subclass)."""
    def orders_inflight_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> int:
        """Abstract method (implement in subclass)."""
    def orders_total_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> int:
        """Abstract method (implement in subclass)."""
    def order_list(self, order_list_id: OrderListId) -> OrderList:
        """Abstract method (implement in subclass)."""
    def order_lists(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> list[OrderList]:
        """Abstract method (implement in subclass)."""
    def order_list_exists(self, order_list_id: OrderListId) -> bool:
        """Abstract method (implement in subclass)."""
    def orders_for_exec_algorithm(self, exec_algorithm_id: ExecAlgorithmId, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: OrderSide = OrderSide.NO_ORDER_SIDE) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def orders_for_exec_spawn(self, exec_spawn_id: ClientOrderId) -> list[Order]:
        """Abstract method (implement in subclass)."""
    def exec_spawn_total_quantity(self, exec_spawn_id: ClientOrderId, active_only: bool = False) -> Quantity:
        """Abstract method (implement in subclass)."""
    def exec_spawn_total_filled_qty(self, exec_spawn_id: ClientOrderId, active_only: bool = False) -> Quantity:
        """Abstract method (implement in subclass)."""
    def exec_spawn_total_leaves_qty(self, exec_spawn_id: ClientOrderId, active_only: bool = False) -> Quantity:
        """Abstract method (implement in subclass)."""
    def position(self, position_id: PositionId) -> Position:
        """Abstract method (implement in subclass)."""
    def position_for_order(self, client_order_id: ClientOrderId) -> Position:
        """Abstract method (implement in subclass)."""
    def position_id(self, client_order_id: ClientOrderId) -> PositionId:
        """Abstract method (implement in subclass)."""
    def position_snapshots(self, position_id: PositionId | None = None) -> list[Any]:
        """Abstract method (implement in subclass)."""
    def positions(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: PositionSide = PositionSide.NO_POSITION_SIDE) -> list[Position]:
        """Abstract method (implement in subclass)."""
    def position_exists(self, position_id: PositionId) -> bool:
        """Abstract method (implement in subclass)."""
    def positions_open(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: PositionSide = PositionSide.NO_POSITION_SIDE) -> list[Position]:
        """Abstract method (implement in subclass)."""
    def positions_closed(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> list[Position]:
        """Abstract method (implement in subclass)."""
    def is_position_open(self, position_id: PositionId) -> bool:
        """Abstract method (implement in subclass)."""
    def is_position_closed(self, position_id: PositionId) -> bool:
        """Abstract method (implement in subclass)."""
    def positions_open_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: PositionSide = PositionSide.NO_POSITION_SIDE) -> int:
        """Abstract method (implement in subclass)."""
    def positions_closed_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None) -> int:
        """Abstract method (implement in subclass)."""
    def positions_total_count(self, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: PositionSide = PositionSide.NO_POSITION_SIDE) -> int:
        """Abstract method (implement in subclass)."""
    def strategy_id_for_order(self, client_order_id: ClientOrderId) -> StrategyId:
        """Abstract method (implement in subclass)."""
    def strategy_id_for_position(self, position_id: PositionId) -> StrategyId:
        """Abstract method (implement in subclass)."""
    def add_greeks(self, greeks: Any) -> None:
        """Abstract method (implement in subclass)."""
    def add_yield_curve(self, yield_curve: Any) -> None:
        """Abstract method (implement in subclass)."""
    def greeks(self, instrument_id: InstrumentId) -> Any:
        """Abstract method (implement in subclass)."""
    def yield_curve(self, curve_name: str) -> Any:
        """Abstract method (implement in subclass)."""
