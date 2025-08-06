from collections import deque
from decimal import Decimal
from typing import Any

from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from stubs.accounting.accounts.base import Account
from stubs.backtest.execution_client import BacktestExecClient
from stubs.backtest.matching_engine import OrderMatchingEngine
from stubs.backtest.models import FeeModel
from stubs.backtest.models import FillModel
from stubs.backtest.models import LatencyModel
from stubs.backtest.modules import SimulationModule
from stubs.cache.base import CacheFacade
from stubs.common.component import Logger
from stubs.common.component import MessageBus
from stubs.common.component import TestClock
from stubs.execution.messages import TradingCommand
from stubs.model.book import OrderBook
from stubs.model.data import Bar
from stubs.model.data import InstrumentClose
from stubs.model.data import InstrumentStatus
from stubs.model.data import OrderBookDelta
from stubs.model.data import OrderBookDeltas
from stubs.model.data import OrderBookDepth10
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Venue
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.orders.base import Order
from stubs.portfolio.base import PortfolioFacade

class SimulatedExchange:

    id: Venue
    oms_type: OmsType
    book_type: BookType
    msgbus: Any
    cache: Any
    exec_client: BacktestExecClient
    account_type: AccountType
    base_currency: Currency | None
    starting_balances: list[Money]
    default_leverage: Decimal
    leverages: dict[InstrumentId, Decimal]
    is_frozen_account: bool
    reject_stop_orders: bool
    support_gtd_orders: bool
    support_contingent_orders: bool
    use_position_ids: bool
    use_random_ids: bool
    use_reduce_only: bool
    use_message_queue: bool
    bar_execution: bool
    bar_adaptive_high_low_ordering: bool
    trade_execution: bool
    fill_model: FillModel
    fee_model: FeeModel
    latency_model: LatencyModel
    modules: list[SimulationModule]
    instruments: dict[InstrumentId, Instrument]

    _clock: TestClock
    _log: Logger
    _message_queue: deque
    _inflight_queue: list[tuple[(int, int), TradingCommand]]
    _inflight_counter: dict[int, int]
    _matching_engines: dict[InstrumentId, OrderMatchingEngine]

    def __init__(
        self,
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        starting_balances: list[Money],
        base_currency: Currency | None,
        default_leverage: Decimal,
        leverages: dict[InstrumentId, Decimal],
        modules: list[SimulationModule],
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: TestClock,
        fill_model: FillModel,
        fee_model: FeeModel,
        latency_model: LatencyModel | None = None,
        book_type: BookType = ...,
        frozen_account: bool = False,
        reject_stop_orders: bool = True,
        support_gtd_orders: bool = True,
        support_contingent_orders: bool = True,
        use_position_ids: bool = True,
        use_random_ids: bool = False,
        use_reduce_only: bool = True,
        use_message_queue: bool = True,
        bar_execution: bool = True,
        bar_adaptive_high_low_ordering: bool = False,
        trade_execution: bool = False,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def register_client(self, client: BacktestExecClient) -> None: ...
    def set_fill_model(self, fill_model: FillModel) -> None: ...
    def set_latency_model(self, latency_model: LatencyModel) -> None: ...
    def initialize_account(self) -> None: ...
    def add_instrument(self, instrument: Instrument) -> None: ...
    def best_bid_price(self, instrument_id: InstrumentId) -> Price | None: ...
    def best_ask_price(self, instrument_id: InstrumentId) -> Price | None: ...
    def get_book(self, instrument_id: InstrumentId) -> OrderBook | None: ...
    def get_matching_engine(self, instrument_id: InstrumentId) -> OrderMatchingEngine | None: ...
    def get_matching_engines(self) -> dict[InstrumentId, OrderMatchingEngine]: ...
    def get_books(self) -> dict[InstrumentId, OrderBook]: ...
    def get_open_orders(self, instrument_id: InstrumentId | None = None) -> list[Order]: ...
    def get_open_bid_orders(self, instrument_id: InstrumentId | None = None) -> list[Order]: ...
    def get_open_ask_orders(self, instrument_id: InstrumentId | None = None) -> list[Order]: ...
    def get_account(self) -> Account: ...
    def adjust_account(self, adjustment: Money) -> None: ...
    def update_instrument(self, instrument: Instrument) -> None: ...
    def send(self, command: TradingCommand) -> None: ...
    def process_order_book_delta(self, delta: OrderBookDelta) -> None: ...
    def process_order_book_deltas(self, deltas: OrderBookDeltas) -> None: ...
    def process_order_book_depth10(self, depth: OrderBookDepth10) -> None: ...
    def process_quote_tick(self, tick: QuoteTick) -> None: ...
    def process_trade_tick(self, tick: TradeTick) -> None: ...
    def process_bar(self, bar: Bar) -> None: ...
    def process_instrument_status(self, data: InstrumentStatus) -> None: ...
    def process_instrument_close(self, close: InstrumentClose) -> None: ...
    def process(self, ts_now: int) -> None: ...
    def reset(self) -> None: ...
