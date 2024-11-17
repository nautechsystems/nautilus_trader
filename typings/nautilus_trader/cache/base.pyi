from typing import List, Optional, Set

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.core.model import OrderSide, PositionSide, PriceType
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar, BarType, QuoteTick, TradeTick
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientId,
    ClientOrderId,
    ExecAlgorithmId,
    InstrumentId,
    OrderListId,
    PositionId,
    StrategyId,
    Venue,
    VenueOrderId,
)
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.synthetic import SyntheticInstrument
from nautilus_trader.model.objects import Currency, Price, Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.model.position import Position

class CacheFacade:
    """Provides a read-only facade for the common `Cache`."""

    # GENERAL
    def get(self, key: str) -> bytes: ...
    def add(self, key: str, value: bytes) -> None: ...

    # DATA QUERIES
    def quote_ticks(self, instrument_id: InstrumentId) -> List[QuoteTick]: ...
    def trade_ticks(self, instrument_id: InstrumentId) -> List[TradeTick]: ...
    def bars(self, bar_type: BarType) -> List[Bar]: ...
    def price(self, instrument_id: InstrumentId, price_type: PriceType) -> Price: ...
    def order_book(self, instrument_id: InstrumentId) -> OrderBook: ...
    def quote_tick(self, instrument_id: InstrumentId, index: int = 0) -> QuoteTick: ...
    def trade_tick(self, instrument_id: InstrumentId, index: int = 0) -> TradeTick: ...
    def bar(self, bar_type: BarType, index: int = 0) -> Bar: ...
    def book_update_count(self, instrument_id: InstrumentId) -> int: ...
    def quote_tick_count(self, instrument_id: InstrumentId) -> int: ...
    def trade_tick_count(self, instrument_id: InstrumentId) -> int: ...
    def bar_count(self, bar_type: BarType) -> int: ...
    def has_order_book(self, instrument_id: InstrumentId) -> bool: ...
    def has_quote_ticks(self, instrument_id: InstrumentId) -> bool: ...
    def has_trade_ticks(self, instrument_id: InstrumentId) -> bool: ...
    def has_bars(self, bar_type: BarType) -> bool: ...
    def get_xrate(
        self,
        venue: Venue,
        from_currency: Currency,
        to_currency: Currency,
        price_type: PriceType = PriceType.MID,
    ) -> float: ...

    # INSTRUMENT QUERIES
    def instrument(self, instrument_id: InstrumentId) -> Instrument: ...
    def instrument_ids(self, venue: Optional[Venue] = None) -> List[InstrumentId]: ...
    def instruments(
        self, venue: Optional[Venue] = None, underlying: Optional[str] = None
    ) -> List[Instrument]: ...

    # SYNTHETIC QUERIES
    def synthetic(self, instrument_id: InstrumentId) -> SyntheticInstrument: ...
    def synthetic_ids(self) -> List[InstrumentId]: ...
    def synthetics(self) -> List[SyntheticInstrument]: ...

    # ACCOUNT QUERIES
    def account(self, account_id: AccountId) -> Account: ...
    def account_for_venue(self, venue: Venue) -> Account: ...
    def account_id(self, venue: Venue) -> AccountId: ...
    def accounts(self) -> List[Account]: ...

    # IDENTIFIER QUERIES
    def client_order_ids(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[ClientOrderId]: ...
    def client_order_ids_open(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[ClientOrderId]: ...
    def client_order_ids_closed(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[ClientOrderId]: ...
    def client_order_ids_emulated(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[ClientOrderId]: ...
    def client_order_ids_inflight(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[ClientOrderId]: ...
    def order_list_ids(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[OrderListId]: ...
    def position_ids(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[PositionId]: ...
    def position_open_ids(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[PositionId]: ...
    def position_closed_ids(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> Set[PositionId]: ...
    def actor_ids(self) -> Set[str]: ...
    def strategy_ids(self) -> Set[StrategyId]: ...
    def exec_algorithm_ids(self) -> Set[ExecAlgorithmId]: ...

    # ORDER QUERIES
    def order(self, client_order_id: ClientOrderId) -> Order: ...
    def client_order_id(self, venue_order_id: VenueOrderId) -> ClientOrderId: ...
    def venue_order_id(self, client_order_id: ClientOrderId) -> VenueOrderId: ...
    def client_id(self, client_order_id: ClientOrderId) -> ClientId: ...
    def orders(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> List[Order]: ...
    def orders_open(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> List[Order]: ...
    def orders_closed(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> List[Order]: ...
    def orders_emulated(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> List[Order]: ...
    def orders_inflight(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> List[Order]: ...
    def orders_for_position(self, position_id: PositionId) -> List[Order]: ...
    def order_exists(self, client_order_id: ClientOrderId) -> bool: ...
    def is_order_open(self, client_order_id: ClientOrderId) -> bool: ...
    def is_order_closed(self, client_order_id: ClientOrderId) -> bool: ...
    def is_order_emulated(self, client_order_id: ClientOrderId) -> bool: ...
    def is_order_inflight(self, client_order_id: ClientOrderId) -> bool: ...
    def is_order_pending_cancel_local(self, client_order_id: ClientOrderId) -> bool: ...
    def orders_open_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int: ...
    def orders_closed_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int: ...
    def orders_emulated_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int: ...
    def orders_inflight_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int: ...
    def orders_total_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int: ...

    # ORDER LIST QUERIES
    def order_list(self, order_list_id: OrderListId) -> OrderList: ...
    def order_lists(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> List[OrderList]: ...
    def order_list_exists(self, order_list_id: OrderListId) -> bool: ...

    # EXEC ALGORITHM QUERIES
    def orders_for_exec_algorithm(
        self,
        exec_algorithm_id: ExecAlgorithmId,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> List[Order]: ...
    def orders_for_exec_spawn(self, exec_spawn_id: ClientOrderId) -> List[Order]: ...
    def exec_spawn_total_quantity(
        self, exec_spawn_id: ClientOrderId, active_only: bool = False
    ) -> Quantity: ...
    def exec_spawn_total_filled_qty(
        self, exec_spawn_id: ClientOrderId, active_only: bool = False
    ) -> Quantity: ...
    def exec_spawn_total_leaves_qty(
        self, exec_spawn_id: ClientOrderId, active_only: bool = False
    ) -> Quantity: ...

    # POSITION QUERIES
    def position(self, position_id: PositionId) -> Position: ...
    def position_for_order(self, client_order_id: ClientOrderId) -> Position: ...
    def position_id(self, client_order_id: ClientOrderId) -> PositionId: ...
    def position_snapshots(
        self, position_id: Optional[PositionId] = None
    ) -> List[Position]: ...
    def positions(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> List[Position]: ...
    def positions_open(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> List[Position]: ...
    def positions_closed(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> List[Position]: ...
    def position_exists(self, position_id: PositionId) -> bool: ...
    def is_position_open(self, position_id: PositionId) -> bool: ...
    def is_position_closed(self, position_id: PositionId) -> bool: ...
    def positions_open_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> int: ...
    def positions_closed_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
    ) -> int: ...
    def positions_total_count(
        self,
        venue: Optional[Venue] = None,
        instrument_id: Optional[InstrumentId] = None,
        strategy_id: Optional[StrategyId] = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> int: ...

    # STRATEGY QUERIES
    def strategy_id_for_order(self, client_order_id: ClientOrderId) -> StrategyId: ...
    def strategy_id_for_position(self, position_id: PositionId) -> StrategyId: ...
