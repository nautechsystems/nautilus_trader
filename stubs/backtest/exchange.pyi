from collections import deque
from decimal import Decimal
from typing import Any

from nautilus_trader.core.nautilus_pyo3 import Account
from nautilus_trader.core.nautilus_pyo3 import AccountType
from nautilus_trader.core.nautilus_pyo3 import BacktestExecClient
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BookType
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import FeeModel
from nautilus_trader.core.nautilus_pyo3 import FillModel
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import InstrumentClose
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import InstrumentStatus
from nautilus_trader.core.nautilus_pyo3 import LatencyModel
from nautilus_trader.core.nautilus_pyo3 import MessageBus
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OmsType
from nautilus_trader.core.nautilus_pyo3 import Order
from nautilus_trader.core.nautilus_pyo3 import OrderBook
from nautilus_trader.core.nautilus_pyo3 import OrderBookDelta
from nautilus_trader.core.nautilus_pyo3 import OrderBookDeltas
from nautilus_trader.core.nautilus_pyo3 import OrderBookDepth10
from nautilus_trader.core.nautilus_pyo3 import OrderMatchingEngine
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import SimulationModule
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.core.nautilus_pyo3 import TradingCommand
from nautilus_trader.core.nautilus_pyo3 import Venue
from stubs.cache.base import CacheFacade
from stubs.common.component import Logger, TestClock
from stubs.portfolio.base import PortfolioFacade

class SimulatedExchange:
    """
    Provides a simulated exchange venue.

    Parameters
    ----------
    venue : Venue
        The venue to simulate.
    oms_type : OmsType {``HEDGING``, ``NETTING``}
        The order management system type used by the exchange.
    account_type : AccountType
        The account type for the client.
    starting_balances : list[Money]
        The starting balances for the exchange.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    default_leverage : Decimal
        The account default leverage (for margin accounts).
    leverages : dict[InstrumentId, Decimal]
        The instrument specific leverage configuration (for margin accounts).
    modules : list[SimulationModule]
        The simulation modules for the exchange.
    portfolio : PortfolioFacade
        The read-only portfolio for the exchange.
    msgbus : MessageBus
        The message bus for the exchange.
    cache : CacheFacade
        The read-only cache for the exchange.
    clock : TestClock
        The clock for the exchange.
    fill_model : FillModel
        The fill model for the exchange.
    fee_model : FeeModel
        The fee model for the exchange.
    latency_model : LatencyModel, optional
        The latency model for the exchange.
    book_type : BookType
        The order book type for the exchange.
    frozen_account : bool, default False
        If the account for this exchange is frozen (balances will not change).
    reject_stop_orders : bool, default True
        If stop orders are rejected on submission if in the market.
    support_gtd_orders : bool, default True
        If orders with GTD time in force will be supported by the exchange.
    support_contingent_orders : bool, default True
        If contingent orders will be supported/respected by the exchange.
        If False, then its expected the strategy will be managing any contingent orders.
    use_position_ids : bool, default True
        If venue position IDs will be generated on order fills.
    use_random_ids : bool, default False
        If all exchange generated identifiers will be random UUID4's.
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders will be honored.
    use_message_queue : bool, default True
        If an internal message queue should be used to process trading commands in sequence after
        they have initially arrived. Setting this to False would be appropriate for real-time
        sandbox environments, where we don't want to introduce additional latency of waiting for
        the next data event before processing the trading command.
    bar_execution : bool, default True
        If bars should be processed by the matching engine(s) (and move the market).
    bar_adaptive_high_low_ordering : bool, default False
        Determines whether the processing order of bar prices is adaptive based on a heuristic.
        This setting is only relevant when `bar_execution` is True.
        If False, bar prices are always processed in the fixed order: Open, High, Low, Close.
        If True, the processing order adapts with the heuristic:
        - If High is closer to Open than Low then the processing order is Open, High, Low, Close.
        - If Low is closer to Open than High then the processing order is Open, Low, High, Close.
    trade_execution : bool, default False
        If trades should be processed by the matching engine(s) (and move the market).

    Raises
    ------
    ValueError
        If `instruments` is empty.
    ValueError
        If `instruments` contains a type other than `Instrument`.
    ValueError
        If `starting_balances` is empty.
    ValueError
        If `starting_balances` contains a type other than `Money`.
    ValueError
        If `base_currency` and multiple starting balances.
    ValueError
        If `modules` contains a type other than `SimulationModule`.

    """

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
    def register_client(self, client: BacktestExecClient) -> None:
        """
        Register the given execution client with the simulated exchange.

        Parameters
        ----------
        client : BacktestExecClient
            The client to register

        """
        ...
    def set_fill_model(self, fill_model: FillModel) -> None:
        """
        Set the fill model for all matching engines.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        ...
    def set_latency_model(self, latency_model: LatencyModel) -> None:
        """
        Change the latency model for this exchange.

        Parameters
        ----------
        latency_model : LatencyModel
            The latency model to set.

        """
        ...
    def initialize_account(self) -> None:
        """
        Initialize the account to the starting balances.

        """
        ...
    def add_instrument(self, instrument: Instrument) -> None:
        """
        Add the given instrument to the exchange.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        Raises
        ------
        ValueError
            If `instrument.id.venue` is not equal to the venue ID.
        InvalidConfiguration
            If `instrument` is invalid for this venue.

        """
        ...
    def best_bid_price(self, instrument_id: InstrumentId) -> Price | None:
        """
        Return the best bid price for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        Price or ``None``

        """
        ...
    def best_ask_price(self, instrument_id: InstrumentId) -> Price | None:
        """
        Return the best ask price for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        Price or ``None``

        """
        ...
    def get_book(self, instrument_id: InstrumentId) -> OrderBook | None:
        """
        Return the order book for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        OrderBook or ``None``

        """
        ...
    def get_matching_engine(self, instrument_id: InstrumentId) -> OrderMatchingEngine | None:
        """
        Return the matching engine for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the matching engine.

        Returns
        -------
        OrderMatchingEngine or ``None``

        """
        ...
    def get_matching_engines(self) -> dict[InstrumentId, OrderMatchingEngine]:
        """
        Return all matching engines for the exchange (for every instrument).

        Returns
        -------
        dict[InstrumentId, OrderMatchingEngine]

        """
        ...
    def get_books(self) -> dict[InstrumentId, OrderBook]:
        """
        Return all order books within the exchange.

        Returns
        -------
        dict[InstrumentId, OrderBook]

        """
        ...
    def get_open_orders(self, instrument_id: InstrumentId | None = None) -> list[Order]:
        """
        Return the open orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def get_open_bid_orders(self, instrument_id: InstrumentId | None = None) -> list[Order]:
        """
        Return the open bid orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def get_open_ask_orders(self, instrument_id: InstrumentId | None = None) -> list[Order]:
        """
        Return the open ask orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        ...
    def get_account(self) -> Account:
        """
        Return the account for the registered client (if registered).

        Returns
        -------
        Account or ``None``

        """
        ...
    def adjust_account(self, adjustment: Money) -> None:
        """
        Adjust the account at the exchange with the given adjustment.

        Parameters
        ----------
        adjustment : Money
            The adjustment for the account.

        """
        ...
    def update_instrument(self, instrument: Instrument) -> None:
        """
        Update the venues current instrument definition with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument definition to update.

        """
        ...
    def send(self, command: TradingCommand) -> None:
        """
        Send the given trading command into the exchange.

        Parameters
        ----------
        command : TradingCommand
            The command to send.

        """
        ...
    def process_order_book_delta(self, delta: OrderBookDelta) -> None:
        """
        Process the exchanges market for the given order book delta.

        Parameters
        ----------
        data : OrderBookDelta
            The order book delta to process.

        """
        ...
    def process_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Process the exchanges market for the given order book deltas.

        Parameters
        ----------
        data : OrderBookDeltas
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

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : QuoteTick
            The tick to process.

        """
        ...
    def process_trade_tick(self, tick: TradeTick) -> None:
        """
        Process the exchanges market for the given trade tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : TradeTick
            The tick to process.

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

        """
        ...
    def process_instrument_status(self, data: InstrumentStatus) -> None:
        """
        Process a specific instrument status.

        Parameters
        ----------
        data : InstrumentStatus
            The instrument status update to process.

        """
        ...
    def process_instrument_close(self, close: InstrumentClose) -> None:
        """
        Process the exchanges market for the given instrument close.

        Parameters
        ----------
        close : InstrumentClose
            The instrument close to process.

        """
        ...
    def process(self, ts_now: int) -> None:
        """
        Process the exchange to the given time.

        All pending commands will be processed along with all simulation modules.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).

        """
        ...
    def reset(self) -> None:
        """
        Reset the simulated exchange.

        All stateful fields are reset to their initial value.
        """
        ...
