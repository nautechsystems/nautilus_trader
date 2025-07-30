from collections.abc import Callable
from collections.abc import Iterator
from datetime import datetime
from decimal import Decimal
from typing import Any

import pandas as pd

from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import AccountType
from nautilus_trader.core.nautilus_pyo3 import BookType
from nautilus_trader.core.nautilus_pyo3 import CacheFacade
from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import Data
from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithm
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Logger
from nautilus_trader.core.nautilus_pyo3 import LogGuard
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import NautilusKernel
from nautilus_trader.core.nautilus_pyo3 import OmsType
from nautilus_trader.core.nautilus_pyo3 import PortfolioFacade
from nautilus_trader.core.nautilus_pyo3 import Strategy
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.trading.trader import Trader
from stubs.backtest.exchange import SimulatedExchange
from stubs.backtest.models import FeeModel
from stubs.backtest.models import FillModel
from stubs.backtest.models import LatencyModel
from stubs.backtest.modules import SimulationModule
from stubs.common.actor import Actor
from stubs.data.engine import DataEngine
from stubs.data.messages import RequestData

class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies over historical
    data.

    Parameters
    ----------
    config : BacktestEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `BacktestEngineConfig`.
    """
    _config: BacktestEngineConfig
    _accumulator: Any
    _run_config_id: str | None
    _run_id: UUID4 | None
    _venues: dict[Venue, SimulatedExchange]
    _has_data: set[InstrumentId]
    _has_book_data: set[InstrumentId]
    _data: list[Data]
    _data_len: int
    _iteration: int
    _last_ns: int
    _end_ns: int
    _run_started: pd.Timestamp | None
    _run_finished: pd.Timestamp | None
    _backtest_start: pd.Timestamp | None
    _backtest_end: pd.Timestamp | None
    _kernel: NautilusKernel
    _instance_id: UUID4
    _log: Logger
    _data_engine: DataEngine # DataEngine
    _data_requests: dict[str, RequestData] # RequestData
    _backtest_subscription_names: set[Any]
    _data_iterator: Any # BacktestDataIterator

    def __init__(self, config: BacktestEngineConfig | None = None) -> None: ...
    def __del__(self) -> None: ...
    @property
    def trader_id(self) -> TraderId:
        """
        Return the engines trader ID.

        Returns
        -------
        TraderId

        """
        ...
    @property
    def machine_id(self) -> str:
        """
        Return the engines machine ID.

        Returns
        -------
        str

        """
        ...
    @property
    def instance_id(self) -> UUID4:
        """
        Return the engines instance ID.

        This is a unique identifier per initialized engine.

        Returns
        -------
        UUID4

        """
        ...
    @property
    def kernel(self) -> NautilusKernel:
        """
        Return the internal kernel for the engine.

        Returns
        -------
        NautilusKernel

        """
        ...
    @property
    def logger(self) -> Logger:
        """
        Return the internal logger for the engine.

        Returns
        -------
        Logger

        """
        ...
    @property
    def run_config_id(self) -> str:
        """
        Return the last backtest engine run config ID.

        Returns
        -------
        str or ``None``

        """
        ...
    @property
    def run_id(self) -> UUID4:
        """
        Return the last backtest engine run ID (if run).

        Returns
        -------
        UUID4 or ``None``

        """
        ...
    @property
    def iteration(self) -> int:
        """
        Return the backtest engine iteration count.

        Returns
        -------
        int

        """
        ...
    @property
    def run_started(self) -> pd.Timestamp | None:
        """
        Return when the last backtest run started (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        ...
    @property
    def run_finished(self) -> pd.Timestamp | None:
        """
        Return when the last backtest run finished (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        ...
    @property
    def backtest_start(self) -> pd.Timestamp | None:
        """
        Return the last backtest run time range start (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        ...
    @property
    def backtest_end(self) -> pd.Timestamp | None:
        """
        Return the last backtest run time range end (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        ...
    @property
    def trader(self) -> Trader:
        """
        Return the engines internal trader.

        Returns
        -------
        Trader

        """
        ...
    @property
    def cache(self) -> CacheFacade:
        """
        Return the engines internal read-only cache.

        Returns
        -------
        CacheFacade

        """
        ...
    @property
    def data(self) -> list[Data]:
        """
        Return the engines internal data stream.

        Returns
        -------
        list[Data]

        """
        ...
    @property
    def portfolio(self) -> PortfolioFacade:
        """
        Return the engines internal read-only portfolio.

        Returns
        -------
        PortfolioFacade

        """
        ...
    def get_log_guard(self) -> nautilus_pyo3.LogGuard | LogGuard | None:
        """
        Return the global logging subsystems log guard.

        May return ``None`` if the logging subsystem was already initialized.

        Returns
        -------
        nautilus_pyo3.LogGuard | LogGuard | None

        """
        ...
    def list_venues(self) -> list[Venue]:
        """
        Return the venues contained within the engine.

        Returns
        -------
        list[Venue]

        """
        ...
    def add_venue(
        self,
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        starting_balances: list[Money],
        base_currency: Currency | None = None,
        default_leverage: Decimal | None = None,
        leverages: dict[InstrumentId, Decimal] | None = None,
        modules: list[SimulationModule] | None = None,
        fill_model: FillModel | None = None,
        fee_model: FeeModel | None = None,
        latency_model: LatencyModel | None = None,
        book_type: BookType = BookType.L1_MBP,
        routing: bool = False,
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
    ) -> None:
        """
        Add a `SimulatedExchange` with the given parameters to the backtest engine.

        Parameters
        ----------
        venue : Venue
            The venue ID.
        oms_type : OmsType {``HEDGING``, ``NETTING``}
            The order management system type for the exchange. If ``HEDGING`` will
            generate new position IDs.
        account_type : AccountType
            The account type for the exchange.
        starting_balances : list[Money]
            The starting account balances (specify one for a single asset account).
        base_currency : Currency, optional
            The account base currency for the client. Use ``None`` for multi-currency accounts.
        default_leverage : Decimal, optional
            The account default leverage (for margin accounts).
        leverages : dict[InstrumentId, Decimal], optional
            The instrument specific leverage configuration (for margin accounts).
        modules : list[SimulationModule], optional
            The simulation modules to load into the exchange.
        fill_model : FillModel, optional
            The fill model for the exchange.
        fee_model : FeeModel, optional
            The fee model for the venue.
        latency_model : LatencyModel, optional
            The latency model for the exchange.
        book_type : BookType, default ``BookType.L1_MBP``
            The default order book type.
        routing : bool, default False
            If multi-venue routing should be enabled for the execution client.
        frozen_account : bool, default False
            If the account for this exchange is frozen (balances will not change).
        reject_stop_orders : bool, default True
            If stop orders are rejected on submission if trigger price is in the market.
        support_gtd_orders : bool, default True
            If orders with GTD time in force will be supported by the venue.
        support_contingent_orders : bool, default True
            If contingent orders will be supported/respected by the venue.
            If False, then it's expected the strategy will be managing any contingent orders.
        use_position_ids : bool, default True
            If venue position IDs will be generated on order fills.
        use_random_ids : bool, default False
            If all venue generated identifiers will be random UUID4's.
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
            If `venue` is already registered with the engine.

        """
        ...
    def change_fill_model(self, venue: Venue, model: FillModel) -> None:
        """
        Change the fill model for the exchange of the given venue.

        Parameters
        ----------
        venue : Venue
            The venue of the simulated exchange.
        model : FillModel
            The fill model to change to.

        """
        ...
    def add_instrument(self, instrument: Instrument) -> None:
        """
        Add the instrument to the backtest engine.

        The instrument must be valid for its associated venue. For instance,
        derivative instruments which would trade on margin cannot be added to
        a venue with a ``CASH`` account.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        Raises
        ------
        InvalidConfiguration
            If the venue for the `instrument` has not been added to the engine.
        InvalidConfiguration
            If `instrument` is not valid for its associated venue.

        """
        ...
    def add_data(
        self,
        data: list,
        client_id: ClientId = None,
        validate: bool = True,
        sort: bool = True,
    ) -> None:
        """
        Add the given `data` to the backtest engine.

        Parameters
        ----------
        data : list[Data]
            The data to add.
        client_id : ClientId, optional
            The client ID to associate with the data.
        validate : bool, default True
            If `data` should be validated
            (recommended when adding data directly to the engine).
        sort : bool, default True
            If `data` should be sorted by `ts_init` with the rest of the stream after adding
            (recommended when adding data directly to the engine).

        Raises
        ------
        ValueError
            If `data` is empty.
        ValueError
            If `data` contains objects which are not a type of `Data`.
        ValueError
            If `instrument_id` for the data is not found in the cache.
        ValueError
            If `data` elements do not have an `instrument_id` and `client_id` is ``None``.
        TypeError
            If `data` is a Rust PyO3 data type (cannot add directly to engine yet).

        Warnings
        --------
        Assumes all data elements are of the same type. Adding lists of varying
        data types could result in incorrect backtest logic.

        Caution if adding data without `sort` being True, as this could lead to running backtests
        on a stream which does not have monotonically increasing timestamps.

        """
        ...
    def dump_pickled_data(self) -> bytes:
        """
        Return the internal data stream pickled.

        Returns
        -------
        bytes

        """
        ...
    def load_pickled_data(self, data: bytes) -> None:
        """
        Load the given pickled data directly into the internal data stream.

        It is highly advised to only pass data to this method which was obtained
        through a call to `.dump_pickled_data()`.

        Warnings
        --------
        This low-level direct access method makes the following assumptions:
         - The data contains valid Nautilus objects only, which inherit from `Data`.
         - The data was successfully pickled from a call to `pickle.dumps()`.
         - The data was sorted prior to pickling.
         - All required instruments have been added to the engine.

        """
        ...
    def add_actor(self, actor: Actor) -> None: # Actor
        """
        Add the given actor to the backtest engine.

        Parameters
        ----------
        actor : Actor
            The actor to add.

        """
        ...
    def add_actors(self, actors: list[Actor]) -> None: # Actor
        """
        Add the given list of actors to the backtest engine.

        Parameters
        ----------
        actors : list[Actor]
            The actors to add.

        """
        ...
    def add_strategy(self, strategy: Strategy) -> None:
        """
        Add the given strategy to the backtest engine.

        Parameters
        ----------
        strategy : Strategy
            The strategy to add.

        """
        ...
    def add_strategies(self, strategies: list[Strategy]) -> None:
        """
        Add the given list of strategies to the backtest engine.

        Parameters
        ----------
        strategies : list[Strategy]
            The strategies to add.

        """
        ...
    def add_exec_algorithm(self, exec_algorithm: ExecAlgorithm) -> None:
        """
        Add the given execution algorithm to the backtest engine.

        Parameters
        ----------
        exec_algorithm : ExecAlgorithm
            The execution algorithm to add.

        """
        ...
    def add_exec_algorithms(self, exec_algorithms: list[ExecAlgorithm]) -> None:
        """
        Add the given list of execution algorithms to the backtest engine.

        Parameters
        ----------
        exec_algorithms : list[ExecAlgorithm]
            The execution algorithms to add.

        """
        ...
    def reset(self) -> None:
        """
        Reset the backtest engine.

        All stateful fields are reset to their initial value.

        Note: instruments and data are not dropped/reset, this can be done through a
        separate call to `.clear_data()` if desired.

        """
        ...
    def clear_data(self) -> None:
        """
        Clear the engines internal data stream.

        Does not clear added instruments.

        """
        ...
    def clear_actors(self) -> None:
        """
        Clear all actors from the engines internal trader.

        """
        ...
    def clear_strategies(self) -> None:
        """
        Clear all trading strategies from the engines internal trader.

        """
        ...
    def clear_exec_algorithms(self) -> None:
        """
        Clear all execution algorithms from the engines internal trader.

        """
        ...
    def dispose(self) -> None:
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.

        Calling this method multiple times has the same effect as calling it once (it is idempotent).
        Once called, it cannot be reversed, and no other methods should be called on this instance.

        """
        ...
    def run(
        self,
        start: datetime | str | int | None = None,
        end: datetime | str | int | None = None,
        run_config_id: str | None = None,
        streaming: bool = False,
    ) -> None:
        """
        Run a backtest.

        At the end of the run the trader and strategies will be stopped, then
        post-run analysis performed.

        For datasets larger than available memory, use `streaming` mode with the
        following sequence:
        - 1. Add initial data batch and strategies
        - 2. Call `run(streaming=True)`
        - 3. Call `clear_data()`
        - 4. Add next batch of data stream
        - 5. Call `run(streaming=False)` or `end()` when processing the final batch

        Parameters
        ----------
        start : datetime or str or int, optional
            The start datetime (UTC) for the backtest run.
            If ``None`` engine runs from the start of the data.
        end : datetime or str or int, optional
            The end datetime (UTC) for the backtest run.
            If ``None`` engine runs to the end of the data.
        run_config_id : str, optional
            The tokenized `BacktestRunConfig` ID.
        streaming : bool, default False
            Controls data loading and processing mode:
            - If False (default): Loads all data at once.
              This is currently the only supported mode for custom data (e.g., option Greeks).
            - If True, loads data in chunks for memory-efficient processing of large datasets.

        Raises
        ------
        ValueError
            If no data has been added to the engine.
        ValueError
            If the `start` is >= the `end` datetime.

        """
        ...
    def end(self) -> None:
        """
        Manually end the backtest.

        Notes
        -----
        Only required if you have previously been running with streaming.

        """
        ...
    def get_result(self) -> BacktestResult:
        """
        Return the backtest result from the last run.

        Returns
        -------
        BacktestResult

        """
        ...
    def set_default_market_data_client(self) -> None: ...

class BacktestDataIterator:
    _empty_data_callback: Callable[[str, int], None] | None
    _log: Logger
    _data: dict
    _data_name: dict
    _data_priority: dict
    _data_len: dict
    _data_index: dict
    _heap: list
    _next_data_priority: int
    _single_data: list
    _single_data_name: str
    _single_data_priority: int
    _single_data_len: int
    _single_data_index: int
    _is_single_data: bool

    def __init__(self, empty_data_callback: Callable[[str, int], None] | None = None) -> None: ...
    def add_data(self, data_name: str, data_list: list, append_data: bool = True) -> None: ...
    def remove_data(self, data_name: str) -> None: ...
    def next(self) -> Data: ...
    def reset(self) -> None: ...
    def set_index(self, data_name: str, index: int) -> None: ...
    def is_done(self) -> bool: ...
    def all_data(self) -> dict: ...
    def data(self, data_name: str) -> list: ...
    def __iter__(self) -> Iterator: ...
    def __next__(self) -> Any | None: ...
