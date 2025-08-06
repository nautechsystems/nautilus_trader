from datetime import timedelta

from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OmsType
from stubs.backtest.models import FeeModel
from stubs.backtest.models import FillModel
from stubs.cache.base import CacheFacade
from stubs.common.component import Logger
from stubs.common.component import MessageBus
from stubs.common.component import TestClock
from stubs.execution.matching_core import MatchingCore
from stubs.execution.messages import BatchCancelOrders
from stubs.execution.messages import CancelAllOrders
from stubs.execution.messages import CancelOrder
from stubs.execution.messages import ModifyOrder
from stubs.model.book import OrderBook
from stubs.model.data import Bar
from stubs.model.data import BarType
from stubs.model.data import InstrumentClose
from stubs.model.data import OrderBookDelta
from stubs.model.data import OrderBookDeltas
from stubs.model.data import OrderBookDepth10
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick
from stubs.model.identifiers import AccountId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import TraderId
from stubs.model.identifiers import Venue
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order
from stubs.model.position import Position

class OrderMatchingEngine:

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
    def set_fill_model(self, fill_model: FillModel) -> None: ...
    def update_instrument(self, instrument: Instrument) -> None: ...
    def best_bid_price(self) -> Price: ...
    def best_ask_price(self) -> Price: ...
    def get_book(self) -> OrderBook: ...
    def get_open_orders(self) -> list[Order]: ...
    def get_open_bid_orders(self) -> list[Order]: ...
    def get_open_ask_orders(self) -> list[Order]: ...
    def order_exists(self, client_order_id: ClientOrderId) -> bool: ...
    def process_order_book_delta(self, delta: OrderBookDelta) -> None: ...
    def process_order_book_deltas(self, deltas: OrderBookDeltas) -> None: ...
    def process_order_book_depth10(self, depth: OrderBookDepth10) -> None: ...
    def process_quote_tick(self, tick: QuoteTick) -> None: ...
    def process_trade_tick(self, tick: TradeTick) -> None: ...
    def process_bar(self, bar: Bar) -> None: ...
    def process_status(self, status: MarketStatusAction) -> None: ...
    def process_instrument_close(self, close: InstrumentClose) -> None: ...
    def process_auction_book(self, book: OrderBook) -> None: ...
    def process_order(self, order: Order, account_id: AccountId) -> None: ...
    def process_modify(self, command: ModifyOrder, account_id: AccountId) -> None: ...
    def process_cancel(self, command: CancelOrder, account_id: AccountId) -> None: ...
    def process_batch_cancel(self, command: BatchCancelOrders, account_id: AccountId) -> None: ...
    def process_cancel_all(self, command: CancelAllOrders, account_id: AccountId) -> None: ...
    def iterate(self, timestamp_ns: int, aggressor_side: AggressorSide = AggressorSide.NO_AGGRESSOR) -> None: ...
    def determine_limit_price_and_volume(self, order: Order) -> list[tuple[Price, Quantity]]: ...
    def determine_market_price_and_volume(self, order: Order) -> list[tuple[Price, Quantity]]: ...
    def fill_market_order(self, order: Order) -> None: ...
    def fill_limit_order(self, order: Order) -> None: ...
    def apply_fills(
        self,
        order: Order,
        fills: list,
        liquidity_side: LiquiditySide,
        venue_position_id: PositionId | None = None,
        position: Position | None = None,
    ) -> None: ...
    def fill_order(self, order: Order, last_px: Price, last_qty: Quantity, liquidity_side: LiquiditySide, venue_position_id: PositionId | None = None, position: Position | None = None) -> None: ...
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

