import datetime as dt
from collections import deque
from decimal import Decimal
from typing import Any

from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.cache.facade import CacheDatabaseFacade, CacheFacade
from nautilus_trader.model.data import BarAggregation
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.synthetic import SyntheticInstrument
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.core.rust.model import OrderStatus
from nautilus_trader.core.rust.model import OmsType
from nautilus_trader.core.rust.model import PositionSide
from nautilus_trader.core.rust.model import PriceType
from nautilus_trader.common.component import Actor
from stubs.model.position import Position


class Cache(CacheFacade):
    """
    Provides a common object cache for market and execution related data.

    Parameters
    ----------
    database : CacheDatabaseFacade, optional
        The database adapter for the cache. If ``None`` then will bypass persistence.
    config : CacheConfig, optional
        The cache configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `CacheConfig`.
    """

    _database: CacheDatabaseFacade | None
    _log: Any
    _drop_instruments_on_reset: bool
    has_backing: bool
    tick_capacity: int
    bar_capacity: int
    _specific_venue: Venue | None
    _general: dict[str, bytes]
    _currencies: dict[str, Currency]
    _instruments: dict[InstrumentId, Instrument]
    _synthetics: dict[InstrumentId, SyntheticInstrument]
    _order_books: dict[InstrumentId, OrderBook]
    _own_order_books: dict[InstrumentId, nautilus_pyo3.OwnOrderBook]
    _quote_ticks: dict[InstrumentId, deque[QuoteTick]]
    _trade_ticks: dict[InstrumentId, deque[TradeTick]]
    _xrate_symbols: dict[InstrumentId, str]
    _mark_xrates: dict[tuple[Currency, Currency], float]
    _mark_prices: dict[InstrumentId, MarkPriceUpdate]
    _index_prices: dict[InstrumentId, IndexPriceUpdate]
    _bars: dict[BarType, deque[Bar]]
    _bars_bid: dict[InstrumentId, Bar]
    _bars_ask: dict[InstrumentId, Bar]
    _accounts: dict[AccountId, Account]
    _orders: dict[ClientOrderId, Order]
    _order_lists: dict[OrderListId, OrderList]
    _positions: dict[PositionId, Position]
    _position_snapshots: dict[PositionId, list[bytes]]
    _greeks: dict[InstrumentId, object]
    _yield_curves: dict[str, object]
    _index_venue_account: dict[Venue, AccountId]
    _index_venue_orders: dict[Venue, set[ClientOrderId]]
    _index_venue_positions: dict[Venue, set[PositionId]]
    _index_venue_order_ids: dict[VenueOrderId, ClientOrderId]
    _index_client_order_ids: dict[ClientOrderId, VenueOrderId]
    _index_order_position: dict[ClientOrderId, PositionId]
    _index_order_strategy: dict[ClientOrderId, StrategyId]
    _index_order_client: dict[ClientOrderId, ClientId]
    _index_position_strategy: dict[PositionId, StrategyId]
    _index_position_orders: dict[PositionId, set[ClientOrderId]]
    _index_instrument_orders: dict[InstrumentId, set[ClientOrderId]]
    _index_instrument_positions: dict[InstrumentId, set[PositionId]]
    _index_strategy_orders: dict[StrategyId, set[ClientOrderId]]
    _index_strategy_positions: dict[StrategyId, set[PositionId]]
    _index_exec_algorithm_orders: dict[ExecAlgorithmId, set[ClientOrderId]]
    _index_exec_spawn_orders: dict[ClientOrderId, set[ClientOrderId]]
    _index_orders: set[ClientOrderId]
    _index_orders_open: set[ClientOrderId]
    _index_orders_open_pyo3: set[nautilus_pyo3.ClientOrderId]
    _index_orders_closed: set[ClientOrderId]
    _index_orders_emulated: set[ClientOrderId]
    _index_orders_inflight: set[ClientOrderId]
    _index_orders_pending_cancel: set[ClientOrderId]
    _index_positions: set[PositionId]
    _index_positions_open: set[PositionId]
    _index_positions_closed: set[PositionId]
    _index_actors: set[ComponentId]
    _index_strategies: set[StrategyId]
    _index_exec_algorithms: set[ExecAlgorithmId]

    def __init__(self, database: CacheDatabaseFacade | None = None, config: CacheConfig | None = None) -> None: ...
    def set_specific_venue(self, venue: Venue) -> None:
        """
        Set a specific venue that the cache will use for subsequent `account_for_venue` calls.

        Primarily for Interactive Brokers, a multi-venue brokerage where account updates
        are not tied to a single venue.

        Parameters
        ----------
        venue : Venue
            The specific venue to set.

        """
        ...
    def cache_all(self) -> None:
        """
        Clears and loads the currencies, instruments, synthetics, accounts, orders, and positions.
        from the cache database.
        """
        ...
    def cache_general(self) -> None:
        """
        Clear the current general cache and load the general objects from the
        cache database.
        """
        ...
    def cache_currencies(self) -> None:
        """
        Clear the current currencies cache and load currencies from the cache
        database.
        """
        ...
    def cache_instruments(self) -> None:
        """
        Clear the current instruments cache and load instruments from the cache
        database.
        """
        ...
    def cache_synthetics(self) -> None:
        """
        Clear the current synthetic instruments cache and load synthetic instruments from the cache
        database.
        """
        ...
    def cache_accounts(self) -> None:
        """
        Clear the current accounts cache and load accounts from the cache
        database.
        """
        ...
    def cache_orders(self) -> None:
        """
        Clear the current orders cache and load orders from the cache database.
        """
        ...
    def cache_order_lists(self) -> None:
        """
        Clear the current order lists cache and load order lists using cached orders.
        """
        ...
    def cache_positions(self) -> None:
        """
        Clear the current positions cache and load positions from the cache
        database.
        """
        ...
    def build_index(self) -> None:
        """
        Build the cache index from objects currently held in memory.
        """
        ...
    def check_integrity(self) -> bool:
        """
        Check integrity of data within the cache.

        All data should be loaded from the database prior to this call. If an
        error is found then a log error message will also be produced.

        Returns
        -------
        bool
            True if checks pass, else False.

        """
        ...
    def check_residuals(self) -> bool:
        """
        Check for any residual open state and log warnings if any are found.

        'Open state' is considered to be open orders and open positions.

        Returns
        -------
        bool
            True if residuals exist, else False.

        """
        ...
    def purge_closed_orders(
        self,
        ts_now: int,
        buffer_secs: int = 0,
        purge_from_database: bool = False,
    ) -> None:
        """
        Purge all closed orders from the cache.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        buffer_secs : uint64_t, default 0
            The purge buffer (seconds) from when the order was closed.
            Only orders that have been closed for at least this amount of time will be purged.
            A value of 0 means purge all closed orders regardless of when they were closed.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        ...
    def purge_closed_positions(
        self,
        ts_now: int,
        buffer_secs: int = 0,
        purge_from_database: bool = False,
    ) -> None:
        """
        Purge all closed positions from the cache.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        buffer_secs : uint64_t, default 0
            The purge buffer (seconds) from when the position was closed.
            Only positions that have been closed for at least this amount of time will be purged.
            A value of 0 means purge all closed positions regardless of when they were closed.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        ...
    def purge_order(self, client_order_id: ClientOrderId, purge_from_database: bool = False) -> None:
        """
        Purge the order for the given client order ID from the cache (if found).

        All `OrderFilled` events for the order will also be purged from any associated position.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to purge.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        ...
    def purge_position(self, position_id: PositionId, purge_from_database: bool = False) -> None:
        """
        Purge the position for the given position ID from the cache (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID to purge.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        ...
    def purge_account_events(
        self,
        ts_now: int,
        lookback_secs: int = 0,
        purge_from_database: bool = False,
    ) -> None:
        """
        Purge all account state events which are outside the lookback window.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        lookback_secs : uint64_t, default 0
            The purge lookback window (seconds) from when the account state event occurred.
            Only events which are outside the lookback window will be purged.
            A value of 0 means purge all account state events.
        purge_from_database : bool, default False
            If purging operations will also delete from the backing database, in addition to the cache.

        """
        ...
    def clear_index(self) -> None: ...
    def reset(self) -> None:
        """
        Reset the cache.

        All stateful fields are reset to their initial value.
        """
        ...
    def dispose(self) -> None:
        """
        Dispose of the cache which will close any underlying database adapter.

        """
        ...
    def flush_db(self) -> None:
        """
        Flush the caches database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        ...
    def calculate_unrealized_pnl(self, position: Position) -> Money | None: ...
    def load_actor(self, actor: Actor) -> None:
        """
        Load the state dictionary into the given actor.

        Parameters
        ----------
        actor : Actor
            The actor to load.

        """
        ...
    def load_strategy(self, strategy: Strategy) -> None:
        """
        Load the state dictionary into the given strategy.

        Parameters
        ----------
        strategy : Strategy
            The strategy to load.

        """
        ...
    def load_instrument(self, instrument_id: InstrumentId) -> Instrument | None:
        """
        Load the instrument associated with the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.

        Returns
        -------
        Instrument or ``None``

        """
        ...
    def load_synthetic(self, instrument_id: InstrumentId) -> SyntheticInstrument | None:
        """
        Load the synthetic instrument associated with the given `instrument_id` (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The synthetic instrument ID to load.

        Returns
        -------
        SyntheticInstrument or ``None``

        Raises
        ------
        ValueError
            If `instrument_id` is not a synthetic instrument ID.

        """
        ...
    def load_account(self, account_id: AccountId) -> Account | None:
        """
        Load the account associated with the given account_id (if found).

        Parameters
        ----------
        account_id : AccountId
            The account ID to load.

        Returns
        -------
        Account or ``None``

        """
        ...
    def load_order(self, client_order_id: ClientOrderId) -> Order | None:
        """
        Load the order associated with the given ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to load.

        Returns
        -------
        Order or ``None``

        """
        ...
    def load_position(self, position_id: PositionId) -> Position | None:
        """
        Load the position associated with the given ID (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID to load.

        Returns
        -------
        Position or ``None``

        """
        ...
    def add(self, key: str, value: bytes) -> None:
        """
        Add the given general object `value` to the cache.

        The cache is agnostic to what the object actually is (and how it may
        be serialized), offering maximum flexibility.

        Parameters
        ----------
        key : str
            The cache key for the object.
        value : bytes
            The object value to write.

        """
        ...
    def add_order_book(self, order_book: OrderBook) -> None:
        """
        Add the given order book to the cache.

        Parameters
        ----------
        order_book : OrderBook
            The order book to add.

        """
        ...
    def add_own_order_book(self, own_order_book) -> None:
        """
        Add the given own order book to the cache.

        Parameters
        ----------
        own_order_book : nautilus_pyo3.OwnOrderBook
            The own order book to add.

        """
        ...
    def add_quote_tick(self, tick: QuoteTick) -> None:
        """
        Add the given quote tick to the cache.

        Parameters
        ----------
        tick : QuoteTick
            The tick to add.

        """
        ...
    def add_trade_tick(self, tick: TradeTick) -> None:
        """
        Add the given trade tick to the cache.

        Parameters
        ----------
        tick : TradeTick
            The tick to add.

        """
        ...
    def add_mark_price(self, mark_price: MarkPriceUpdate) -> None:
        """
        Add the given mark price update to the cache.

        Parameters
        ----------
        mark_price : MarkPriceUpdate
            The mark price update to add.

        """
        ...
    def add_index_price(self, index_price: IndexPriceUpdate) -> None:
        """
        Add the given index price update to the cache.

        Parameters
        ----------
        index_price : IndexPriceUpdate
            The index price update to add.

        """
        ...
    def add_bar(self, bar: Bar) -> None:
        """
        Add the given bar to the cache.

        Parameters
        ----------
        bar : Bar
            The bar to add.

        """
        ...
    def add_quote_ticks(self, ticks: list[QuoteTick]) -> None:
        """
        Add the given quotes to the cache.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The ticks to add.

        """
        ...
    def add_trade_ticks(self, ticks: list[TradeTick]) -> None:
        """
        Add the given trades to the cache.

        Parameters
        ----------
        ticks : list[TradeTick]
            The ticks to add.

        """
        ...
    def add_bars(self, bars: list[Bar]) -> None:
        """
        Add the given bars to the cache.

        Parameters
        ----------
        bars : list[Bar]
            The bars to add.

        """
        ...
    def add_currency(self, currency: Currency) -> None:
        """
        Add the given currency to the cache.

        Parameters
        ----------
        currency : Currency
            The currency to add.

        """
        ...
    def add_instrument(self, instrument: Instrument) -> None:
        """
        Add the given instrument to the cache.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        ...
    def add_synthetic(self, synthetic: SyntheticInstrument) -> None:
        """
        Add the given synthetic instrument to the cache.

        Parameters
        ----------
        synthetic : SyntheticInstrument
            The synthetic instrument to add.

        """
        ...
    def add_account(self, account: Account) -> None:
        """
        Add the given account to the cache.

        Parameters
        ----------
        account : Account
            The account to add.

        Raises
        ------
        ValueError
            If `account_id` is already contained in the cache.

        """
        ...
    def add_venue_order_id(
        self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        overwrite: bool = False,
    ) -> None:
        """
        Index the given client order ID with the given venue order ID.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to index.
        venue_order_id : VenueOrderId
            The venue order ID to index.
        overwrite : bool, default False
            If the venue order ID will 'overwrite' any existing indexing and replace
            it in the cache. This is currently used for updated orders where the venue
            order ID may change.

        Raises
        ------
        ValueError
            If `overwrite` is False and the `client_order_id` is already indexed with a different `venue_order_id`.

        """
        ...
    def add_order(
        self,
        order: Order,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        overwrite: bool = False,
    ) -> None:
        """
        Add the given order to the cache indexed with the given position
        ID.

        Parameters
        ----------
        order : Order
            The order to add.
        position_id : PositionId, optional
            The position ID to index for the order.
        client_id : ClientId, optional
            The execution client ID for order routing.
        overwrite : bool, default False
            If the added order should 'overwrite' any existing order and replace
            it in the cache. This is currently used for emulated orders which are
            being released and transformed into another type.

        Raises
        ------
        ValueError
            If `order.client_order_id` is already contained in the cache.

        """
        ...
    def add_order_list(self, order_list: OrderList) -> None:
        """
        Add the given order list to the cache.

        Parameters
        ----------
        order_list : OrderList
            The order_list to add.

        Raises
        ------
        ValueError
            If `order_list.id` is already contained in the cache.

        """
        ...
    def add_position_id(
        self,
        position_id: PositionId,
        venue: Venue,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
    ) -> None:
        """
        Index the given position ID with the other given IDs.

        Parameters
        ----------
        position_id : PositionId
            The position ID to index.
        venue : Venue
            The venue ID to index with the position ID.
        client_order_id : ClientOrderId
            The client order ID to index with the position ID.
        strategy_id : StrategyId
            The strategy ID to index with the position ID.

        """
        ...
    def add_position(self, position: Position, oms_type: OmsType) -> None:
        """
        Add the given position to the cache.

        Parameters
        ----------
        position : Position
            The position to add.
        oms_type : OmsType
            The order management system type for the position.

        Raises
        ------
        ValueError
            If `oms_type` is ``HEDGING`` and a virtual `position.id` is already contained in the cache.

        """
        ...
    def add_greeks(self, greeks: object) -> None:
        """
        Add greeks to the cache.

        Parameters
        ----------
        greeks : GreeksData
            The greeks to add.

        """
        ...
    def add_yield_curve(self, yield_curve: object) -> None:
        """
        Add a yield curve to the cache.

        Parameters
        ----------
        yield_curve : YieldCurveData
            The yield curve to add.

        """
        ...
    def greeks(self, instrument_id: InstrumentId) -> object | None:
        """
        Return the latest cached greeks for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to get the greeks for.

        Returns
        -------
        GreeksData
            The greeks for the given instrument ID.

        """
        ...
    def yield_curve(self, curve_name: str) -> object | None:
        """
        Return the latest cached yield curve for the given curve name.

        Parameters
        ----------
        curve_name : str
            The name of the yield curve to get.

        Returns
        -------
        YieldCurveData
            The interest rate curve for the given currency.

        """
        ...
    def snapshot_position(self, position: Position) -> None:
        """
        Snapshot the given position in its current state.

        The position ID will be appended with a UUID v4 string.

        Parameters
        ----------
        position : Position
            The position to snapshot.

        """
        ...
    def snapshot_position_state(
        self,
        position: Position,
        ts_snapshot: int,
        unrealized_pnl: Money | None = None,
        open_only: bool = True,
    ) -> None:
        """
        Snapshot the state dictionary for the given `position`.

        This method will persist to the backing cache database.

        Parameters
        ----------
        position : Position
            The position to snapshot the state for.
        ts_snapshot : uint64_t
            UNIX timestamp (nanoseconds) when the snapshot was taken.
        unrealized_pnl : Money, optional
            The current unrealized PnL for the position.
        open_only : bool, default True
            If only open positions should be snapshot, this flag helps to avoid race conditions
            where a position is snapshot when no longer open.

        """
        ...
    def snapshot_order_state(self, order: Order) -> None:
        """
        Snapshot the state dictionary for the given `order`.

        This method will persist to the backing cache database.

        Parameters
        ----------
        order : Order
            The order to snapshot the state for.

        """
        ...
    def update_account(self, account: Account) -> None:
        """
        Update the given account in the cache.

        Parameters
        ----------
        account : The account to update (from last event).

        """
        ...
    def update_order(self, order: Order) -> None:
        """
        Update the given order in the cache.

        Parameters
        ----------
        order : Order
            The order to update (from last event).

        """
        ...
    def update_order_pending_cancel_local(self, order: Order) -> None:
        """
        Update the given `order` as pending cancel locally.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        ...
    def update_own_order_book(self, order: Order) -> None:
        """
        Update the own order book for the given order.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        ...
    def update_position(self, position: Position) -> None:
        """
        Update the given position in the cache.

        Parameters
        ----------
        position : Position
            The position to update (from last event).

        """
        ...
    def update_actor(self, actor: Actor) -> None:
        """
        Update the given actor state in the cache.

        Parameters
        ----------
        actor : Actor
            The actor to update.
        """
        ...
    def update_strategy(self, strategy: Strategy) -> None:
        """
        Update the given strategy state in the cache.

        Parameters
        ----------
        strategy : Strategy
            The strategy to update.
        """
        ...
    def delete_actor(self, actor: Actor) -> None:
        """
        Delete the given actor from the cache.

        Parameters
        ----------
        actor : Actor
            The actor to deregister.

        Raises
        ------
        ValueError
            If `actor` is not contained in the actors index.

        """
        ...
    def delete_strategy(self, strategy: Strategy) -> None:
        """
        Delete the given strategy from the cache.

        Parameters
        ----------
        strategy : Strategy
            The strategy to deregister.

        Raises
        ------
        ValueError
            If `strategy` is not contained in the strategies index.

        """
        ...
    def get(self, key: str) -> bytes | None:
        """
        Return the general object for the given `key`.

        The cache is agnostic to what the object actually is (and how it may
        be serialized), offering maximum flexibility.

        Parameters
        ----------
        key : str
            The cache key for the object.

        Returns
        -------
        bytes or ``None``

        """
        ...
    def quote_ticks(self, instrument_id: InstrumentId) -> list[QuoteTick]:
        """
        Return the quotes for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks to get.

        Returns
        -------
        list[QuoteTick]

        """
        ...
    def trade_ticks(self, instrument_id: InstrumentId) -> list[TradeTick]:
        """
        Return trades for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks to get.

        Returns
        -------
        list[TradeTick]

        """
        ...
    def mark_prices(self, instrument_id: InstrumentId) -> list[MarkPriceUpdate]:
        """
        Return mark prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices to get.

        Returns
        -------
        list[MarkPriceUpdate]

        """
        ...
    def index_prices(self, instrument_id: InstrumentId) -> list[IndexPriceUpdate]:
        """
        Return index prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices to get.

        Returns
        -------
        list[IndexPriceUpdate]

        """
        ...
    def bars(self, bar_type: BarType) -> list[Bar]:
        """
        Return bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for bars to get.

        Returns
        -------
        list[Bar]

        """
        ...
    def price(self, instrument_id: InstrumentId, price_type: PriceType) -> Price | None:
        """
        Return the price for the given instrument ID and price type.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.
        price_type : PriceType
            The price type for the query.

        Returns
        -------
        Price or ``None``

        """
        ...
    def prices(self, price_type: PriceType) -> dict[InstrumentId, Price]:
        """
        Return a map of latest prices per instrument ID for the given price type.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.
        price_type : PriceType
            The price type for the query.

        Returns
        -------
        dict[InstrumentId, Price]
            Includes key value pairs for prices which exist.

        """
        ...
    def order_book(self, instrument_id: InstrumentId) -> OrderBook | None:
        """
        Return the order book for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the order book to get.

        Returns
        -------
        OrderBook or ``None``
            If book not found for the instrument ID then returns ``None``.

        """
        ...
    def own_order_book(self, instrument_id: InstrumentId) -> nautilus_pyo3.OwnOrderBook | None:
        """
        Return the own order book for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the own order book to get.
            Note this is the standard Cython `InstumentId`.

        Returns
        -------
        nautilus_pyo3.OwnOrderBook or ``None``
            If own book not found for the instrument ID then returns ``None``.

        """
        ...
    def own_bid_orders(
        self,
        instrument_id: InstrumentId,
        status: set[OrderStatus] | None = None,
        accepted_buffer_ns: int = 0,
        ts_now: int = 0,
    ) -> dict[Decimal, list[Order]] | None:
        """
        Return own bid orders for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the own orders to get.
            Note this is the standard Cython `InstumentId`.
        status : set[OrderStatus], optional
            The order status to filter for. Empty price levels after filtering are excluded from the result.
        accepted_buffer_ns : uint64_t, optional
            The minimum time in nanoseconds that must have elapsed since the order was accepted.
            Orders accepted less than this time ago will be filtered out.
        ts_now : uint64_t, optional
            The current time in nanoseconds. Required if accepted_buffer_ns > 0.

        Returns
        -------
        dict[Decimal, list[Order]] or ``None``
            If own book not found for the instrument ID then returns ``None``.

        Raises
        ------
        ValueError
            If `accepted_buffer_ns` > 0 and `ts_now` == 0.

        """
        ...
    def own_ask_orders(
        self,
        instrument_id: InstrumentId,
        status: set[OrderStatus] | None = None,
        accepted_buffer_ns: int = 0,
        ts_now: int = 0,
    ) -> dict[Decimal, list[Order]] | None:
        """
        Return own ask orders for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the own orders to get.
            Note this is the standard Cython `InstumentId`.
        status : set[OrderStatus], optional
            The order status to filter for. Empty price levels after filtering are excluded from the result.
        accepted_buffer_ns : uint64_t, optional
            The minimum time in nanoseconds that must have elapsed since the order was accepted.
            Orders accepted less than this time ago will be filtered out.
        ts_now : uint64_t, optional
            The current time in nanoseconds. Required if accepted_buffer_ns > 0.

        Returns
        -------
        dict[Decimal, list[Order]] or ``None``
            If own book not found for the instrument ID then returns ``None``.

        Raises
        ------
        ValueError
            If `accepted_buffer_ns` > 0 and `ts_now` == 0.

        """
        ...
    def quote_tick(self, instrument_id: InstrumentId, index: int = 0) -> QuoteTick | None:
        """
        Return the quote tick for the given instrument ID at the given index (if found).

        Last quote tick if no index specified.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the tick to get.
        index : int, optional
            The index for the tick to get.

        Returns
        -------
        QuoteTick or ``None``
            If no ticks or no tick at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent tick at index 0).

        """
        ...
    def trade_tick(self, instrument_id: InstrumentId, index: int = 0) -> TradeTick | None:
        """
        Return the trade tick for the given instrument ID at the given index (if found).

        Last trade tick if no index specified.

        Parameters
    
        ----------
        instrument_id : InstrumentId
            The instrument ID for the tick to get.
        index : int, optional
            The index for the tick to get.

        Returns
        -------
        TradeTick or ``None``
            If no ticks or no tick at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent tick at index 0).

        """
        ...
    def mark_price(self, instrument_id: InstrumentId, index: int = 0) -> MarkPriceUpdate | None:
        """
        Return the mark price for the given instrument ID at the given index (if found).

        Last mark price if no index specified.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark price to get.
        index : int, optional
            The index for the mark price to get.

        Returns
        -------
        MarkPriceUpdate or ``None``
            If no mark prices or no mark price at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent mark price at index 0).

        """
        ...
    def index_price(self, instrument_id: InstrumentId, index: int = 0) -> IndexPriceUpdate | None:
        """
        Return the index price for the given instrument ID at the given index (if found).

        Last index price if no index specified.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the index price to get.
        index : int, optional
            The index for the index price to get.

        Returns
        -------
        IndexPriceUpdate or ``None``
            If no index prices or no index price at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent index price at index 0).

        """
        ...
    def bar(self, bar_type: BarType, index: int = 0) -> Bar | None:
        """
        Return the bar for the given bar type at the given index (if found).

        Last bar if no index specified.

        Parameters
        ----------
        bar_type : BarType
            The bar type to get.
        index : int, optional
            The index for the bar to get.

        Returns
        -------
        Bar or ``None``
            If no bars or no bar at index then returns ``None``.

        Notes
        -----
        Reverse indexed (most recent bar at index 0).

        """
        ...
    def book_update_count(self, instrument_id: InstrumentId) -> int:
        """
        The count of order book updates for the given instrument ID.

        Will return zero if there is no book for the instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the book.

        Returns
        -------
        int

        """
        ...
    def quote_tick_count(self, instrument_id: InstrumentId) -> int:
        """
        The count of quotes for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        int

        """
        ...
    def trade_tick_count(self, instrument_id: InstrumentId) -> int:
        """
        The count of trades for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        int

        """
        ...
    def mark_price_count(self, instrument_id: InstrumentId) -> int:
        """
        The count of mark prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices.

        Returns
        -------
        int

        """
        ...
    def index_price_count(self, instrument_id: InstrumentId) -> int:
        """
        The count of index prices for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the index prices.

        Returns
        -------
        int

        """
        ...
    def bar_count(self, bar_type: BarType) -> int:
        """
        The count of bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to count.

        Returns
        -------
        int

        """
        ...
    def has_order_book(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the cache has an order book snapshot
        for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the order book snapshot.

        Returns
        -------
        bool

        """
        ...
    def has_quote_ticks(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the cache has quotes for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        bool

        """
        ...
    def has_trade_ticks(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the cache has trades for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the ticks.

        Returns
        -------
        bool

        """
        ...
    def has_mark_prices(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the cache has mark prices for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the mark prices.

        Returns
        -------
        bool

        """
        ...
    def has_index_prices(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the cache has index prices for the
        given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the index prices.

        Returns
        -------
        bool

        """
        ...
    def has_bars(self, bar_type: BarType) -> bool:
        """
        Return a value indicating whether the cache has bars for the given bar
        type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the bars.

        Returns
        -------
        bool

        """
        ...
    def get_xrate(
        self,
        venue: Venue,
        from_currency: Currency,
        to_currency: Currency,
        price_type: PriceType = PriceType.MID,
    ) -> float | None:
        """
        Return the calculated exchange rate.

        If the exchange rate cannot be calculated then returns ``None``.

        Parameters
        ----------
        venue : Venue
            The venue for the exchange rate.
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.
        price_type : PriceType
            The price type for the exchange rate.

        Returns
        -------
        float or ``None``

        Raises
        ------
        ValueError
            If `price_type` is ``LAST`` or ``MARK``.

        """
        ...
    def get_mark_xrate(self, from_currency: Currency, to_currency: Currency) -> float | None:
        """
        Return the exchange rate based on mark price.

        Will return ``None`` if an exchange rate has not been set.

        Parameters
        ----------
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.

        Returns
        -------
        float or ``None``

        """
        ...
    def set_mark_xrate(self, from_currency: Currency, to_currency: Currency, xrate: float) -> None:
        """
        Set the exchange rate based on mark price.

        Will also set the inverse xrate automatically.

        Parameters
        ----------
        from_currency : Currency
            The base currency for the exchange rate to set.
        to_currency : Currency
            The quote currency for the exchange rate to set.
        xrate : double
            The exchange rate based on mark price.

        Raises
        ------
        ValueError
            If `xrate` is zero.

        """
        ...
    def clear_mark_xrate(self, from_currency: Currency, to_currency: Currency) -> None:
        """
        Clear the exchange rate based on mark price.

        Parameters
        ----------
        from_currency : Currency
            The base currency for the exchange rate to clear.
        to_currency : Currency
            The quote currency for the exchange rate to clear.

        """
        ...
    def clear_mark_xrates(self) -> None:
        """
        Clear the exchange rates based on mark price.

        """
        ...
    def instrument(self, instrument_id: InstrumentId) -> Instrument | None:
        """
        Return the instrument corresponding to the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID of the instrument to return.

        Returns
        -------
        Instrument or ``None``

        """
        ...
    def instrument_ids(self, venue: Venue | None = None) -> list[InstrumentId]:
        """
        Return all instrument IDs held by the cache.

        Parameters
        ----------
        venue : Venue, optional
            The venue filter for the query.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def instruments(self, venue: Venue | None = None, underlying: str | None = None) -> list[Instrument]:
        """
        Return all instruments held by the cache.

        Parameters
        ----------
        venue : Venue, optional
            The venue filter for the query.
        underlying : str, optional
            The underlying root symbol for the query.

        Returns
        -------
        list[Instrument]

        """
        ...
    def bar_types(
        self,
        instrument_id: InstrumentId | None = None,
        price_type: PriceType | None = None,
        aggregation_source = None,
    ) -> list[BarType]:
        """
        Return all bar types with the given query filters.

        If a filter parameter is ``None``, then no filtering occurs for that parameter.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        price_type : PriceType, optional
            The price type query filter.
        aggregation_source : AggregationSource, optional
            The aggregation source query filter.

        Returns
        -------
        list[BarType]

        """
        ...
    def synthetic(self, instrument_id: InstrumentId) -> SyntheticInstrument | None:
        """
        Return the synthetic instrument corresponding to the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID of the synthetic instrument to return.

        Returns
        -------
        SyntheticInstrument or ``None``

        Raises
        ------
        ValueError
            If `instrument_id` is not a synthetic instrument ID.

        """
        ...
    def synthetic_ids(self) -> list[InstrumentId]:
        """
        Return all synthetic instrument IDs held by the cache.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def synthetics(self) -> list[SyntheticInstrument]:
        """
        Return all synthetic instruments held by the cache.

        Returns
        -------
        list[SyntheticInstrument]

        """
        ...
    def account(self, account_id: AccountId) -> Account | None:
        """
        Return the account matching the given ID (if found).

        Parameters
        ----------
        account_id : AccountId
            The account ID.

        Returns
        -------
        Account or ``None``

        """
        ...
    def account_for_venue(self, venue: Venue) -> Account | None:
        """
        Return the account matching the given client ID (if found).

        If unique_venue is set, it will be used instead of the provided venue.

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        Account or ``None``

        """
        ...
    def account_id(self, venue: Venue) -> AccountId | None:
        """
        Return the account ID for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account ID.

        Returns
        -------
        AccountId or ``None``

        """
        ...
    def accounts(self) -> list[Account]:
        """
        Return all accounts in the cache.

        Returns
        -------
        list[Account]

        """
        ...
    def client_order_ids(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[ClientOrderId]:
        """
        Return all client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        ...
    def client_order_ids_open(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[ClientOrderId]:
        """
        Return all open client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        ...
    def client_order_ids_closed(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[ClientOrderId]:
        """
        Return all closed client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        ...
    def client_order_ids_emulated(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[ClientOrderId]:
        """
        Return all emulated client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        ...
    def client_order_ids_inflight(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[ClientOrderId]:
        """
        Return all in-flight client order IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[ClientOrderId]

        """
        ...
    def order_list_ids(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[OrderListId]:
        """
        Return all order list IDs.

        Returns
        -------
        set[OrderListId]

        """
        ...
    def position_ids(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[PositionId]:
        """
        Return all position IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[PositionId]

        """
        ...
    def position_open_ids(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[PositionId]:
        """
        Return all open position IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[PositionId]

        """
        ...
    def position_closed_ids(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> set[PositionId]:
        """
        Return all closed position IDs with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        set[PositionId]

        """
        ...
    def actor_ids(self) -> set[ComponentId]:
        """
        Return all actor IDs.

        Returns
        -------
        set[ComponentId]

        """
        ...
    def strategy_ids(self) -> set[StrategyId]:
        """
        Return all strategy IDs.

        Returns
        -------
        set[StrategyId]

        """
        ...
    def exec_algorithm_ids(self) -> set[ExecAlgorithmId]:
        """
        Return all execution algorithm IDs.

        Returns
        -------
        set[ExecAlgorithmId]

        """
        ...
    def order(self, client_order_id: ClientOrderId) -> Order | None:
        """
        Return the order matching the given client order ID (if found).

        Returns
        -------
        Order or ``None``

        """
        ...
    def client_order_id(self, venue_order_id: VenueOrderId) -> ClientOrderId | None:
        """
        Return the client order ID matching the given venue order ID (if found).

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue assigned order ID.

        Returns
        -------
        ClientOrderId or ``None``

        """
        ...
    def venue_order_id(self, client_order_id: ClientOrderId) -> VenueOrderId | None:
        """
        Return the order ID matching the given client order ID (if found).

        Returns
        -------
        VenueOrderId or ``None``

        """
        ...
    def client_id(self, client_order_id: ClientOrderId) -> ClientId | None:
        """
        Return the specific execution client ID matching the given client order ID (if found).

        Returns
        -------
        ClientId or ``None``

        """
        ...
    def orders(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> list[Order]:
        """
        Return all orders matching the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def orders_open(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> list[Order]:
        """
        Return all open orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def orders_closed(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> list[Order]:
        """
        Return all closed orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def orders_emulated(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> list[Order]:
        """
        Return all emulated orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def orders_inflight(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> list[Order]:
        """
        Return all in-flight orders with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def orders_for_position(self, position_id: PositionId) -> list[Order]:
        """
        Return all orders for the given position ID.

        Parameters
        ----------
        position_id : PositionId
            The position ID for the orders.

        Returns
        -------
        list[Order]

        """
        ...
    def order_exists(self, client_order_id: ClientOrderId) -> bool:
        """
        Return a value indicating whether an order with the given ID exists.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        ...
    def is_order_open(self, client_order_id: ClientOrderId) -> bool:
        """
        Return a value indicating whether an order with the given ID is open.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        ...
    def is_order_closed(self, client_order_id: ClientOrderId) -> bool:
        """
        Return a value indicating whether an order with the given ID is closed.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        ...
    def is_order_emulated(self, client_order_id: ClientOrderId) -> bool:
        """
        Return a value indicating whether an order with the given ID is emulated.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        ...
    def is_order_inflight(self, client_order_id: ClientOrderId) -> bool:
        """
        Return a value indicating whether an order with the given ID is in-flight.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        ...
    def is_order_pending_cancel_local(self, client_order_id: ClientOrderId) -> bool:
        """
        Return a value indicating whether an order with the given ID is pending cancel locally.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID to check.

        Returns
        -------
        bool

        """
        ...
    def orders_open_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int:
        """
        Return the count of open orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        ...
    def orders_closed_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int:
        """
        Return the count of closed orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        ...
    def orders_emulated_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int:
        """
        Return the count of emulated orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        ...
    def orders_inflight_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int:
        """
        Return the count of in-flight orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        ...
    def orders_total_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> int:
        """
        Return the total count of orders with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        int

        """
        ...
    def order_list(self, order_list_id: OrderListId) -> OrderList | None:
        """
        Return the order list matching the given order list ID (if found).

        Returns
        -------
        OrderList or ``None``

        """
        ...
    def order_lists(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> list[OrderList]:
        """
        Return all order lists matching the given query filters.

        *No particular order of list elements is guaranteed.*

        Returns
        -------
        list[OrderList]

        """
        ...
    def order_list_exists(self, order_list_id: OrderListId) -> bool:
        """
        Return a value indicating whether an order list with the given ID exists.

        Parameters
        ----------
        order_list_id : OrderListId
            The order list ID to check.

        Returns
        -------
        bool

        """
        ...
    def orders_for_exec_algorithm(
        self,
        exec_algorithm_id: ExecAlgorithmId,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: OrderSide = OrderSide.NO_ORDER_SIDE,
    ) -> list[Order]:
        """
        Return all execution algorithm orders for the given query filters.

        Parameters
        ----------
        exec_algorithm_id : ExecAlgorithmId
            The execution algorithm ID.
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
            The order side query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def orders_for_exec_spawn(self, exec_spawn_id: ClientOrderId) -> list[Order]:
        """
        Return all orders for the given execution spawn ID (if found).

        Will also include the primary (original) order.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.

        Returns
        -------
        list[Order]

        """
        ...
    def exec_spawn_total_quantity(self, exec_spawn_id: ClientOrderId, active_only: bool = False) -> Quantity | None:
        """
        Return the total quantity for the given execution spawn ID (if found).

        If no execution spawn ID matches then returns ``None``.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.
        active_only : bool, default False
            The flag to filter for active execution spawn orders only.

        Returns
        -------
        Quantity or ``None``

        Notes
        -----
        An "active" order is defined as one which is *not closed*.

        """
        ...
    def exec_spawn_total_filled_qty(self, exec_spawn_id: ClientOrderId, active_only: bool = False) -> Quantity | None:
        """
        Return the total filled quantity for the given execution spawn ID (if found).

        If no execution spawn ID matches then returns ``None``.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.
        active_only : bool, default False
            The flag to filter for active execution spawn orders only.

        Returns
        -------
        Quantity or ``None``

        Notes
        -----
        An "active" order is defined as one which is *not closed*.

        """
        ...
    def exec_spawn_total_leaves_qty(self, exec_spawn_id: ClientOrderId, active_only: bool = False) -> Quantity | None:
        """
        Return the total leaves quantity for the given execution spawn ID (if found).

        If no execution spawn ID matches then returns ``None``.

        Parameters
        ----------
        exec_spawn_id : ClientOrderId
            The execution algorithm spawning primary (original) client order ID.
        active_only : bool, default False
            The flag to filter for active execution spawn orders only.

        Returns
        -------
        Quantity or ``None``

        Notes
        -----
        An "active" order is defined as one which is *not closed*.

        """
        ...
    def position(self, position_id: PositionId) -> Position | None:
        """
        Return the position associated with the given ID (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        Position or ``None``

        """
        ...
    def position_for_order(self, client_order_id: ClientOrderId) -> Position | None:
        """
        Return the position associated with the given client order ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID.

        Returns
        -------
        Position or ``None``

        """
        ...
    def position_id(self, client_order_id: ClientOrderId) -> PositionId | None:
        """
        Return the position ID associated with the given client order ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID associated with the position.

        Returns
        -------
        PositionId or ``None``

        """
        ...
    def position_snapshots(self, position_id: PositionId | None = None) -> list[Any]:
        """
        Return all position snapshots with the given optional identifier filter.

        Parameters
        ----------
        position_id : PositionId, optional
            The position ID query filter.

        Returns
        -------
        list[Position]

        """
        ...
    def positions(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> list[Any]:
        """
        Return all positions with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        list[Position]

        """
        ...
    def positions_open(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> list[Any]:
        """
        Return all open positions with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        list[Position]

        """
        ...
    def positions_closed(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> list[Any]:
        """
        Return all closed positions with the given query filters.

        *No particular order of list elements is guaranteed.*

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        list[Position]

        """
        ...
    def position_exists(self, position_id: PositionId) -> bool:
        """
        Return a value indicating whether a position with the given ID exists.

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        int

        """
        ...
    def is_position_open(self, position_id: PositionId) -> bool:
        """
        Return a value indicating whether a position with the given ID exists
        and is open.

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        bool

        """
        ...
    def is_position_closed(self, position_id: PositionId) -> bool:
        """
        Return a value indicating whether a position with the given ID exists
        and is closed.

        Parameters
        ----------
        position_id : PositionId
            The position ID.

        Returns
        -------
        bool

        """
        ...
    def positions_open_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> int:
        """
        Return the count of open positions with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        int

        """
        ...
    def positions_closed_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
    ) -> int:
        """
        Return the count of closed positions with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.

        Returns
        -------
        int

        """
        ...
    def positions_total_count(
        self,
        venue: Venue | None = None,
        instrument_id: InstrumentId | None = None,
        strategy_id: StrategyId | None = None,
        side: PositionSide = PositionSide.NO_POSITION_SIDE,
    ) -> int:
        """
        Return the total count of positions with the given query filters.

        Parameters
        ----------
        venue : Venue, optional
            The venue ID query filter.
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        strategy_id : StrategyId, optional
            The strategy ID query filter.
        side : PositionSide, default ``NO_POSITION_SIDE`` (no filter)
            The position side query filter.

        Returns
        -------
        int

        """
        ...
    def strategy_id_for_order(self, client_order_id: ClientOrderId) -> StrategyId | None:
        """
        Return the strategy ID associated with the given ID (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID associated with the strategy.

        Returns
        -------
        StrategyId or ``None``

        """
        ...
    def strategy_id_for_position(self, position_id: PositionId) -> StrategyId | None:
        """
        Return the strategy ID associated with the given ID (if found).

        Parameters
        ----------
        position_id : PositionId
            The position ID associated with the strategy.

        Returns
        -------
        StrategyId or ``None``

        """
        ...
    def heartbeat(self, timestamp: datetime) -> None:
        """
        Add a heartbeat at the given `timestamp`.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the heartbeat.

        """
        ...
    def audit_own_order_books(self) -> None:
        """
        Audit all own order books against public order books.

        Ensures:
         - Closed orders are removed from own order books.

        Logs all failures as errors.

        """
        ...

def process_own_order_map(
    own_order_map: dict[Decimal, list[nautilus_pyo3.OwnBookOrder]],
    order_cache: dict[ClientOrderId, Order],
) -> dict[Decimal, list[Order]]: ...
