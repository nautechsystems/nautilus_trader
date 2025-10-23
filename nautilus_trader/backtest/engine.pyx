# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import heapq
import pickle
import uuid
from collections import deque
from decimal import Decimal
from heapq import heappush
from typing import Any
from typing import Callable
from typing import Generator

import cython
import pandas as pd

from nautilus_trader.accounting.error import AccountError
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.common import Environment
from nautilus_trader.common.component import is_logging_pyo3
from nautilus_trader.common.config import InvalidConfiguration
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model import BOOK_DATA_TYPES
from nautilus_trader.model import NAUTILUS_PYO3_DATA_TYPES
from nautilus_trader.system.kernel import NautilusKernel
from nautilus_trader.trading.trader import Trader

from cpython.datetime cimport timedelta
from cpython.object cimport PyObject
from libc.stdint cimport uint32_t
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.margin_models cimport LeveragedMarginModel
from nautilus_trader.accounting.margin_models cimport MarginModel
from nautilus_trader.backtest.data_client cimport BacktestDataClient
from nautilus_trader.backtest.data_client cimport BacktestMarketDataClient
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FeeModel
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.models cimport LatencyModel
from nautilus_trader.backtest.models cimport MakerTakerFeeModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport FORCE_STOP
from nautilus_trader.common.component cimport LOGGING_PYO3
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport LogGuard
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TestClock
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.common.component cimport flush_logger
from nautilus_trader.common.component cimport get_component_clocks
from nautilus_trader.common.component cimport is_logging_initialized
from nautilus_trader.common.component cimport log_sysinfo
from nautilus_trader.common.component cimport set_backtest_force_stop
from nautilus_trader.common.component cimport set_logging_clock_realtime_mode
from nautilus_trader.common.component cimport set_logging_clock_static_mode
from nautilus_trader.common.component cimport set_logging_clock_static_time
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.datetime cimport format_optional_iso8601
from nautilus_trader.core.datetime cimport maybe_dt_to_unix_nanos
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.backtest cimport TimeEventAccumulatorAPI
from nautilus_trader.core.rust.backtest cimport time_event_accumulator_advance_clock
from nautilus_trader.core.rust.backtest cimport time_event_accumulator_drain
from nautilus_trader.core.rust.backtest cimport time_event_accumulator_drop
from nautilus_trader.core.rust.backtest cimport time_event_accumulator_new
from nautilus_trader.core.rust.common cimport TimeEventHandler_t
from nautilus_trader.core.rust.common cimport logging_is_colored
from nautilus_trader.core.rust.common cimport vec_time_event_handlers_drop
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport InstrumentCloseType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport MarketStatus
from nautilus_trader.core.rust.model cimport MarketStatusAction
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport orderbook_best_ask_price
from nautilus_trader.core.rust.model cimport orderbook_best_bid_price
from nautilus_trader.core.rust.model cimport orderbook_has_ask
from nautilus_trader.core.rust.model cimport orderbook_has_bid
from nautilus_trader.core.rust.model cimport trade_id_new
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport SubscribeInstruments
from nautilus_trader.data.messages cimport UnsubscribeData
from nautilus_trader.data.messages cimport UnsubscribeInstruments
from nautilus_trader.execution.algorithm cimport ExecAlgorithm
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.execution.trailing cimport TrailingStopCalculator
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport BookOrder
from nautilus_trader.model.data cimport CustomData
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.functions cimport account_type_to_str
from nautilus_trader.model.functions cimport aggressor_side_to_str
from nautilus_trader.model.functions cimport book_type_to_str
from nautilus_trader.model.functions cimport oms_type_to_str
from nautilus_trader.model.functions cimport order_type_to_str
from nautilus_trader.model.functions cimport time_in_force_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport EXPIRING_INSTRUMENT_TYPES
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.crypto_future cimport CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair cimport CurrencyPair
from nautilus_trader.model.instruments.equity cimport Equity
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.limit_if_touched cimport LimitIfTouchedOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_if_touched cimport MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.portfolio.base cimport PortfolioFacade
from nautilus_trader.trading.strategy cimport Strategy


cdef class BacktestEngine:
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

    def __init__(self, config: BacktestEngineConfig | None = None) -> None:
        if config is None:
            config = BacktestEngineConfig()

        Condition.type(config, BacktestEngineConfig, "config")

        self._config: BacktestEngineConfig  = config

        # Set up components
        self._accumulator = <TimeEventAccumulatorAPI>time_event_accumulator_new()

        # Run IDs
        self._run_config_id: str | None = None
        self._run_id: UUID4 | None = None

        # Venues and data
        self._venues: dict[Venue, SimulatedExchange] = {}
        self._has_data: set[InstrumentId] = set()
        self._has_book_data: set[InstrumentId] = set()
        self._data: list[Data] = []
        self._data_len: uint64_t = 0
        self._iteration: uint64_t = 0
        self._last_ns : uint64_t = 0
        self._end_ns : uint64_t = 0

        # Timing
        self._run_started: pd.Timestamp | None = None
        self._run_finished: pd.Timestamp | None = None
        self._backtest_start: pd.Timestamp | None = None
        self._backtest_end: pd.Timestamp | None = None

        # Build core system kernel
        self._kernel = NautilusKernel(name=type(self).__name__, config=config)
        self._instance_id = self._kernel.instance_id
        self._log = Logger(type(self).__name__)

        self._data_engine: DataEngine = self._kernel.data_engine

        # Set up data iterator
        self._data_requests: dict[str, RequestData] = {}
        self._last_subscription_ts: dict[str, uint64_t] = {}
        self._backtest_subscription_names = set()
        self._response_data = []
        self._data_iterator = BacktestDataIterator()
        self._kernel.msgbus.register(endpoint="BacktestEngine.execute", handler=self._handle_data_command)

    def __del__(self) -> None:
        if self._accumulator._0 != NULL:
            time_event_accumulator_drop(self._accumulator)

    @property
    def trader_id(self) -> TraderId:
        """
        Return the engines trader ID.

        Returns
        -------
        TraderId

        """
        return self._kernel.trader_id

    @property
    def machine_id(self) -> str:
        """
        Return the engines machine ID.

        Returns
        -------
        str

        """
        return self._kernel.machine_id

    @property
    def instance_id(self) -> UUID4:
        """
        Return the engines instance ID.

        This is a unique identifier per initialized engine.

        Returns
        -------
        UUID4

        """
        return self._kernel.instance_id

    @property
    def kernel(self) -> NautilusKernel:
        """
        Return the internal kernel for the engine.

        Returns
        -------
        NautilusKernel

        """
        return self._kernel

    @property
    def logger(self) -> Logger:
        """
        Return the internal logger for the engine.

        Returns
        -------
        Logger

        """
        return self._log

    @property
    def run_config_id(self) -> str:
        """
        Return the last backtest engine run config ID.

        Returns
        -------
        str or ``None``

        """
        return self._run_config_id

    @property
    def run_id(self) -> UUID4:
        """
        Return the last backtest engine run ID (if run).

        Returns
        -------
        UUID4 or ``None``

        """
        return self._run_id

    @property
    def iteration(self) -> int:
        """
        Return the backtest engine iteration count.

        Returns
        -------
        int

        """
        return self._iteration

    @property
    def run_started(self) -> pd.Timestamp | None:
        """
        Return when the last backtest run started (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        return self._run_started

    @property
    def run_finished(self) -> pd.Timestamp | None:
        """
        Return when the last backtest run finished (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        return self._run_finished

    @property
    def backtest_start(self) -> pd.Timestamp | None:
        """
        Return the last backtest run time range start (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        return self._backtest_start

    @property
    def backtest_end(self) -> pd.Timestamp | None:
        """
        Return the last backtest run time range end (if run).

        Returns
        -------
        pd.Timestamp or ``None``

        """
        return self._backtest_end

    @property
    def trader(self) -> Trader:
        """
        Return the engines internal trader.

        Returns
        -------
        Trader

        """
        return self._kernel.trader

    @property
    def cache(self) -> CacheFacade:
        """
        Return the engines internal read-only cache.

        Returns
        -------
        CacheFacade

        """
        return self._kernel.cache

    @property
    def data(self) -> list[Data]:
        """
        Return the engines internal data stream.

        Returns
        -------
        list[Data]

        """
        return self._data.copy()

    @property
    def portfolio(self) -> PortfolioFacade:
        """
        Return the engines internal read-only portfolio.

        Returns
        -------
        PortfolioFacade

        """
        return self._kernel.portfolio

    def get_log_guard(self) -> nautilus_pyo3.LogGuard | LogGuard | None:
        """
        Return the global logging subsystems log guard.

        May return ``None`` if the logging subsystem was already initialized.

        Returns
        -------
        nautilus_pyo3.LogGuard | LogGuard | None

        """
        return self._kernel.get_log_guard()

    def list_venues(self) -> list[Venue]:
        """
        Return the venues contained within the engine.

        Returns
        -------
        list[Venue]

        """
        return list(self._venues)

    def add_venue(
        self,
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        starting_balances: list[Money],
        base_currency: Currency | None = None,
        default_leverage: Decimal | None = None,
        leverages: dict[InstrumentId, Decimal] | None = None,
        margin_model: MarginModel = None,
        modules: list[SimulationModule] | None = None,
        fill_model: FillModel | None = None,
        fee_model: FeeModel | None = None,
        latency_model: LatencyModel | None = None,
        book_type: BookType = BookType.L1_MBP,
        routing: bool = False,
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
        allow_cash_borrowing: bool = False,
        frozen_account: bool = False,
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
        margin_model : MarginModelConfig, optional
            The margin calculation model configuration. Default 'leveraged'.
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
        allow_cash_borrowing : bool, default False
            If cash accounts should allow borrowing (negative balances).
        frozen_account : bool, default False
            If the account for this exchange is frozen (balances will not change).

        Raises
        ------
        ValueError
            If `venue` is already registered with the engine.

        """
        if modules is None:
            modules = []

        if margin_model is None:
            margin_model = LeveragedMarginModel()

        if fill_model is None:
            fill_model = FillModel()

        if fee_model is None:
            fee_model = MakerTakerFeeModel()

        Condition.not_none(venue, "venue")
        Condition.not_in(venue, self._venues, "venue", "_venues")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules")
        Condition.type(fill_model, FillModel, "fill_model")
        Condition.type(fee_model, FeeModel, "fee_model")

        if default_leverage is None:
            if account_type == AccountType.MARGIN:
                default_leverage = Decimal(10)
            else:
                default_leverage = Decimal(1)

        # Create exchange
        exchange = SimulatedExchange(
            venue=venue,
            oms_type=oms_type,
            account_type=account_type,
            starting_balances=starting_balances,
            base_currency=base_currency,
            default_leverage=default_leverage,
            leverages=leverages or {},
            margin_model=margin_model,
            modules=modules,
            portfolio=self._kernel.portfolio,
            msgbus=self._kernel.msgbus,
            cache=self._kernel.cache,
            fill_model=fill_model,
            fee_model=fee_model,
            latency_model=latency_model,
            book_type=book_type,
            clock=self._kernel.clock,
            frozen_account=frozen_account,
            reject_stop_orders=reject_stop_orders,
            support_gtd_orders=support_gtd_orders,
            support_contingent_orders=support_contingent_orders,
            use_position_ids=use_position_ids,
            use_random_ids=use_random_ids,
            use_reduce_only=use_reduce_only,
            use_message_queue=use_message_queue,
            bar_execution=bar_execution,
            bar_adaptive_high_low_ordering=bar_adaptive_high_low_ordering,
            trade_execution=trade_execution,
        )

        self._venues[venue] = exchange

        # Create execution client for exchange
        exec_client = BacktestExecClient(
            exchange=exchange,
            msgbus=self._kernel.msgbus,
            cache=self._kernel.cache,
            clock=self._kernel.clock,
            routing=routing,
            frozen_account=frozen_account,
            allow_cash_borrowing=allow_cash_borrowing,
        )

        exchange.register_client(exec_client)
        self._kernel.exec_engine.register_client(exec_client)

        self._add_market_data_client_if_not_exists(venue)

        self._log.info(f"Added {exchange}")

    def change_fill_model(self, Venue venue, FillModel model) -> None:
        """
        Change the fill model for the exchange of the given venue.

        Parameters
        ----------
        venue : Venue
            The venue of the simulated exchange.
        model : FillModel
            The fill model to change to.

        """
        Condition.not_none(venue, "venue")
        Condition.not_none(model, "model")
        Condition.is_in(venue, self._venues, "venue", "self._venues")

        self._venues[venue].set_fill_model(model)

    def add_instrument(self, Instrument instrument) -> None:
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
        Condition.not_none(instrument, "instrument")

        if instrument.id.venue not in self._venues:
            raise InvalidConfiguration(
                "Cannot add an `Instrument` object without first adding its associated venue. "
                f"Add the {instrument.id.venue} venue using the `add_venue` method."
            )

        # Validate instrument is correct for the venue
        cdef SimulatedExchange venue = self._venues[instrument.id.venue]

        if (
            isinstance(instrument, CurrencyPair)
            and venue.account_type != AccountType.MARGIN
            and venue.base_currency is not None  # Single-currency account
        ):
            raise InvalidConfiguration(
                f"Cannot add `CurrencyPair` instrument {instrument} "
                "for a venue with a single-currency CASH account.",
            )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument.id.venue)

        # Add data
        self._kernel.data_engine.process(instrument)  # Adds to cache
        self._venues[instrument.id.venue].add_instrument(instrument)

        self._log.info(f"Added {instrument.id} Instrument")

    def add_data(
        self,
        list data,
        ClientId client_id = None,
        bint validate = True,
        bint sort = True,
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
        Condition.not_empty(data, "data")
        Condition.list_type(data, Data, "data")

        if isinstance(data[0], NAUTILUS_PYO3_DATA_TYPES):
            raise TypeError(
                f"Cannot add data of type `{type(data[0]).__name__}` from pyo3 directly to engine. "
                "This will be supported in a future release.",
            )

        cdef str data_added_str = "data"

        if validate:
            first = data[0]

            if hasattr(first, "instrument_id"):
                Condition.is_true(
                    first.instrument_id in self._kernel.cache.instrument_ids(),
                    f"`Instrument` {first.instrument_id} for the given data not found in the cache. "
                    "Add the instrument through `add_instrument()` prior to adding related data.",
                )
                # Check client has been registered
                self._add_market_data_client_if_not_exists(first.instrument_id.venue)
                self._has_data.add(first.instrument_id)
                data_added_str = f"{first.instrument_id} {type(first).__name__}"
            elif isinstance(first, Bar):
                Condition.is_true(
                    first.bar_type.instrument_id in self._kernel.cache.instrument_ids(),
                    f"`Instrument` {first.bar_type.instrument_id} for the given data not found in the cache. "
                    "Add the instrument through `add_instrument()` prior to adding related data.",
                )
                Condition.equal(
                    first.bar_type.aggregation_source,
                    AggregationSource.EXTERNAL,
                    "bar_type.aggregation_source",
                    "required source",
                )
                self._has_data.add(first.bar_type.instrument_id)
                data_added_str = f"{first.bar_type} {type(first).__name__}"
            else:
                Condition.not_none(client_id, "client_id")
                # Check client has been registered
                self._add_data_client_if_not_exists(client_id)

                if isinstance(first, CustomData):
                    data_added_str = f"{type(first.data).__name__} "

            if type(first) in BOOK_DATA_TYPES:
                self._has_book_data.add(first.instrument_id)

        # Add data
        self._data.extend(data)

        if sort:
            self._data = sorted(self._data, key=lambda x: x.ts_init)

        self._data_iterator.add_data("backtest_data", self._data)

        for data_point in data:
            data_type = type(data_point)

            if data_type is Bar:
                self._backtest_subscription_names.add(f"{data_point.bar_type}")
            elif data_type in (QuoteTick, TradeTick):
                self._backtest_subscription_names.add(f"{data_type.__name__}.{data_point.instrument_id}")
            elif data_type is CustomData:
                self._backtest_subscription_names.add(f"{type(data_point.data).__name__}.{getattr(data_point.data, 'instrument_id', None)}")

        self._log.info(
            f"Added {len(data):_} {data_added_str} element{'' if len(data) == 1 else 's'}",
        )

    def add_data_iterator(
        self,
        str data_name,
        generator: Generator[list[Data], None, None],
        ClientId client_id = None,
    ) -> None:
        """
        Add a single stream generator that yields ``list[Data]`` objects for the low-level streaming backtest API.

        Parameters
        ----------
        data_name : str
            The name identifier for the data stream.
        generator : Generator[list[Data], None, None]
            A Python generator that yields lists of ``Data`` objects.
        client_id : ClientId, optional
            The client ID to associate with the data.

        Notes
        -----
        This method enables streaming large datasets by loading data in chunks.
        The generator should yield ``list[Data]`` objects sorted by `ts_init` timestamp.

        """
        self._data_iterator.init_data(
            data_name,
            generator,
            append_data=True
        )

        self._log.info(f"Added {data_name} stream generator")

    cpdef void _handle_data_command(self, DataCommand command):
        if not(command.data_type.type in [Bar, QuoteTick, TradeTick, OrderBookDepth10]
               or type(command) not in [SubscribeData, UnsubscribeData, SubscribeInstruments, UnsubscribeInstruments]):
            return

        if isinstance(command, SubscribeData):
            self._handle_subscribe(<SubscribeData>command)
        elif isinstance(command, UnsubscribeData):
            self._handle_unsubscribe(<UnsubscribeData>command)

    cdef void _handle_subscribe(self, SubscribeData command):
        cdef RequestData request = command.to_request(None, None, self._handle_data_response)
        cdef str subscription_name = request.params["subscription_name"]

        if subscription_name in self._data_requests or subscription_name in self._backtest_subscription_names:
            return

        self._log.debug(f"Subscribing to {subscription_name}, {command.params.get('durations_seconds')=}")

        self._data_requests[subscription_name] = request
        request.params["end_ns"] = self._end_ns
        time_range_generator = TIME_RANGE_GENERATORS.get(
            request.params.get("time_range_generator", ""),
            BacktestEngine.default_time_range_generator
        )(self._last_ns, request.params)
        cdef bint append_data = request.params.get("append_data", True)
        self._data_iterator.init_data(subscription_name, self._subscription_generator(subscription_name, time_range_generator), append_data)

    def _subscription_generator(self, str subscription_name, time_range_generator):
        """
        Generator that yields data for subscription using a time generator.
        """
        def get_next_time_range(data_received):
            # Helper to get next time range with proper error handling
            try:
                return time_range_generator.send(data_received) if data_received else next(time_range_generator)
            except StopIteration:
                return None, None

        # Get initial time range
        start_time, end_time = get_next_time_range(False)

        try:
            while start_time is not None and start_time <= self._end_ns:
                # Clear and update response data
                self._response_data = []
                self._update_subscription_data(subscription_name, start_time, end_time)

                # Determine signal based on whether we got data
                data_received = len(self._response_data) > 0

                # Yield data if we have any
                if self._response_data:
                    yield self._response_data

                # Get next time range
                start_time, end_time = get_next_time_range(data_received)
        finally:
            # Ensure generator is properly closed
            try:
                time_range_generator.close()
            except (StopIteration, GeneratorExit):
                pass

    cpdef void _update_subscription_data(self, str subscription_name, uint64_t start_time, uint64_t end_time):
        cdef RequestData request = self._data_requests[subscription_name]
        cdef RequestData new_request = request.with_dates(
            unix_nanos_to_dt(start_time),
            unix_nanos_to_dt(end_time),
            self._last_ns
        )
        self._log.debug(f"Renewing {request.data_type.type.__name__} data from {unix_nanos_to_dt(start_time)} to {unix_nanos_to_dt(end_time)}")
        self._kernel._msgbus.request(endpoint="DataEngine.request", request=new_request)

    cpdef void _handle_data_response(self, DataResponse response):
        cdef list data = response.data
        cdef str subscription_name = response.params["subscription_name"]

        if not data:
            self._log.debug(f"Removing backtest data for {subscription_name}")
        else:
            self._log.debug(f"Received subscribe {subscription_name} data from {unix_nanos_to_dt(data[0].ts_init)} to {unix_nanos_to_dt(data[-1].ts_init)}")

        self._response_data = data

    cpdef void _handle_unsubscribe(self, UnsubscribeData command):
        cdef str subscription_name = ""

        if command.data_type.type is Bar:
            subscription_name = f"{command.bar_type}"
        elif type(command) is UnsubscribeInstruments:
            subscription_name = "subscribe_instruments"
        else:
            subscription_name = f"{command.data_type.type.__name__}.{command.instrument_id}"

        self._log.debug(f"Unsubscribing {subscription_name}")
        self._data_iterator.remove_data(subscription_name, complete_remove=True)
        self._data_requests.pop(subscription_name, None)

    @classmethod
    def default_time_range_generator(cls, uint64_t initial_time, dict params):
        """
        Generator that yields (start_time, end_time) tuples for data subscription.

        This generator handles the duration logic and can receive feedback via .send().
        """
        cdef uint64_t offset
        cdef uint64_t start_time
        cdef uint64_t end_time
        cdef uint64_t last_subscription_ts = initial_time

        cdef uint64_t end_ns = params.get("end_ns", 0)
        cdef bint point_data = params.get("point_data", False)
        durations_seconds = params.get("durations_seconds", [None])
        durations_ns = [duration_seconds * 1e9 if duration_seconds else None for duration_seconds in durations_seconds]
        cdef int iteration_index = 0

        while True:
            # Possibility to use durations of various lengths to take into account weekends or market breaks
            for duration_ns in durations_ns:
                # First iteration for [a, a + duration], then ]a + duration, a + 2 * duration]
                # When point_data we do a query for [start_time, start_time] only
                offset = 1 if iteration_index > 0 and not point_data else 0
                start_time = last_subscription_ts + offset

                if start_time > end_ns:
                    return

                if duration_ns:
                    end_time = min(start_time + duration_ns - offset, end_ns)
                else:
                    end_time = end_ns

                last_subscription_ts = end_time

                if point_data:
                    end_time = start_time

                # Yield the time range and wait for feedback
                data_received = yield (start_time, end_time)
                iteration_index += 1

                # If we received a success signal, break from the duration loop
                if data_received:
                    break
            else:
                # If we completed the for loop without breaking (no success), exit the while loop
                return

    def dump_pickled_data(self) -> bytes:
        """
        Return the internal data stream pickled.

        Returns
        -------
        bytes

        """
        return pickle.dumps(self._data)

    def load_pickled_data(self, bytes data) -> None:
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
        Condition.not_none(data, "data")
        self._data = pickle.loads(data)

        self._log.info(
            f"Loaded {len(self._data):_} data "
            f"element{'' if len(data) == 1 else 's'} from pickle",
        )

    def add_actor(self, actor: Actor) -> None:
        """
        Add the given actor to the backtest engine.

        Parameters
        ----------
        actor : Actor
            The actor to add.

        """
        # Checked inside trader
        self._kernel.trader.add_actor(actor)

    def add_actors(self, actors: list[Actor]) -> None:
        """
        Add the given list of actors to the backtest engine.

        Parameters
        ----------
        actors : list[Actor]
            The actors to add.

        """
        # Checked inside trader
        self._kernel.trader.add_actors(actors)

    def add_strategy(self, strategy: Strategy) -> None:
        """
        Add the given strategy to the backtest engine.

        Parameters
        ----------
        strategy : Strategy
            The strategy to add.

        """
        # Checked inside trader
        self._kernel.trader.add_strategy(strategy)

    def add_strategies(self, strategies: list[Strategy]) -> None:
        """
        Add the given list of strategies to the backtest engine.

        Parameters
        ----------
        strategies : list[Strategy]
            The strategies to add.

        """
        # Checked inside trader
        self._kernel.trader.add_strategies(strategies)

    def add_exec_algorithm(self, exec_algorithm: ExecAlgorithm) -> None:
        """
        Add the given execution algorithm to the backtest engine.

        Parameters
        ----------
        exec_algorithm : ExecAlgorithm
            The execution algorithm to add.

        """
        # Checked inside trader
        self._kernel.trader.add_exec_algorithm(exec_algorithm)

    def add_exec_algorithms(self, exec_algorithms: list[ExecAlgorithm]) -> None:
        """
        Add the given list of execution algorithms to the backtest engine.

        Parameters
        ----------
        exec_algorithms : list[ExecAlgorithm]
            The execution algorithms to add.

        """
        # Checked inside trader
        self._kernel.trader.add_exec_algorithms(exec_algorithms)

    def reset(self) -> None:
        """
        Reset the backtest engine.

        All stateful fields are reset to their initial value, except for data and instruments which persist.

        Notes
        -----
        Data and instruments are retained across resets by default to enable repeated runs
        with different strategies or parameters against the same dataset.

        See Also
        --------
        https://nautilustrader.io/docs/concepts/backtesting#repeated-runs

        """
        self._log.debug(f"Resetting")

        if self._kernel.trader.is_running:
            # End current backtest run
            self.end()

        # Reset DataEngine
        if self._kernel.data_engine.is_running:
            self._kernel.data_engine.stop()

        self._kernel.data_engine.reset()

        # Reset ExecEngine
        if self._kernel.exec_engine.is_running:
            self._kernel.exec_engine.stop()

        self._kernel.exec_engine.reset()

        # Reset RiskEngine
        if self._kernel.risk_engine.is_running:
            self._kernel.risk_engine.stop()

        self._kernel.risk_engine.reset()

        # Reset Emulator
        if self._kernel.emulator.is_running:
            self._kernel.emulator.stop()

        self._kernel.emulator.reset()

        self._kernel.trader.reset()

        for exchange in self._venues.values():
            exchange.reset()

        # Reset run IDs
        self._run_config_id = None
        self._run_id = None

        # Reset timing
        self._iteration = 0
        self._data_iterator = BacktestDataIterator()
        self._data_iterator.add_data("backtest_data", self._data)
        self._run_started = None
        self._run_finished = None
        self._backtest_start = None
        self._backtest_end = None

        self._log.info("Reset")

    def sort_data(self) -> None:
        """
        Sort the engines internal data stream.

        """
        self._data.sort()

    def clear_data(self) -> None:
        """
        Clear the engines internal data stream.

        Does not clear added instruments.

        """
        self._has_data.clear()
        self._has_book_data.clear()
        self._data.clear()
        self._data_len = 0
        self._data_iterator = BacktestDataIterator()

    def clear_actors(self) -> None:
        """
        Clear all actors from the engines internal trader.

        """
        self._kernel.trader.clear_actors()

    def clear_strategies(self) -> None:
        """
        Clear all trading strategies from the engines internal trader.

        """
        self._kernel.trader.clear_strategies()

    def clear_exec_algorithms(self) -> None:
        """
        Clear all execution algorithms from the engines internal trader.

        """
        self._kernel.trader.clear_exec_algorithms()

    def dispose(self) -> None:
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.

        Calling this method multiple times has the same effect as calling it once (it is idempotent).
        Once called, it cannot be reversed, and no other methods should be called on this instance.

        """
        self.clear_data()
        self._kernel.dispose()

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
        self._run(start, end, run_config_id, streaming)

        if not streaming:
            self.end()

    def end(self):
        """
        Manually end the backtest.

        Notes
        -----
        Only required if you have previously been running with streaming.

        """
        if self._kernel.trader.is_running:
            self._kernel.trader.stop()

        if self._kernel.data_engine.is_running:
            self._kernel.data_engine.stop()

        if self._kernel.risk_engine.is_running:
            self._kernel.risk_engine.stop()

        if self._kernel.exec_engine.is_running:
            self._kernel.exec_engine.stop()

        if self._kernel.emulator.is_running:
            self._kernel.emulator.stop()

        try:
            # Process remaining messages
            for exchange in self._venues.values():
                exchange.process(self._kernel.clock.timestamp_ns())
        except AccountError:
            pass

        self._run_finished = pd.Timestamp.utcnow()
        self._backtest_end = self._kernel.clock.utc_now()

        # Change logger clock back to real-time for consistent time stamping
        set_logging_clock_realtime_mode()

        if LOGGING_PYO3:
            nautilus_pyo3.logging_clock_set_realtime_mode()

        self._log_post_run()

        if LOGGING_PYO3:
            nautilus_pyo3.logger_flush()
        else:
            flush_logger()

    def get_result(self):
        """
        Return the backtest result from the last run.

        Returns
        -------
        BacktestResult

        """
        stats_pnls: dict[str, dict[str, float]] = {}

        for currency in self._kernel.portfolio.analyzer.currencies:
            stats_pnls[currency.code] = self._kernel.portfolio.analyzer.get_performance_stats_pnls(currency)

        if self._backtest_start is not None and self._backtest_end is not None:
            elapsed_time = (self._backtest_end - self._backtest_start).total_seconds()
        else:
            elapsed_time = 0

        return BacktestResult(
            trader_id=self._kernel.trader_id.value,
            machine_id=self._kernel.machine_id,
            run_config_id=self._run_config_id,
            instance_id=self._kernel.instance_id.value,
            run_id=self._run_id.to_str() if self._run_id is not None else None,
            run_started=maybe_dt_to_unix_nanos(self._run_started),
            run_finished=maybe_dt_to_unix_nanos(self.run_finished),
            backtest_start=maybe_dt_to_unix_nanos(self._backtest_start),
            backtest_end=maybe_dt_to_unix_nanos(self._backtest_end),
            elapsed_time=elapsed_time,
            iterations=self._iteration,
            total_events=self._kernel.exec_engine.event_count,
            total_orders=self._kernel.cache.orders_total_count(),
            total_positions=self._kernel.cache.positions_total_count(),
            stats_pnls=stats_pnls,
            stats_returns=self._kernel.portfolio.analyzer.get_performance_stats_returns(),
        )

    def _run(
        self,
        start: datetime | str | int | None = None,
        end: datetime | str | int | None = None,
        run_config_id: str | None = None,
        bint streaming = False,
    ):
        # Validate data
        cdef:
            SimulatedExchange exchange
            InstrumentId instrument_id
            bint has_data
            bint missing_book_data
            bint book_type_has_depth
        for exchange in self._venues.values():
            for instrument_id in exchange.instruments:
                has_data = instrument_id in self._has_data
                missing_book_data = instrument_id not in self._has_book_data
                book_type_has_depth = exchange.book_type > BookType.L1_MBP

                if book_type_has_depth and has_data and missing_book_data:
                    raise InvalidConfiguration(
                        f"No order book data found for instrument '{instrument_id }' when `book_type` is '{book_type_to_str(exchange.book_type)}'. "
                        "Set the venue `book_type` to 'L1_MBP' (for top-of-book data like quotes, trades, and bars) or provide order book data for this instrument."
                    )

        cdef uint64_t start_ns
        cdef uint64_t end_ns

        # Time range check and set
        if start is None:
            # Set `start` to start of data
            start_ns = self._data[0].ts_init if self._data else 0
            start = unix_nanos_to_dt(start_ns)
        else:
            start = pd.to_datetime(start, utc=True)
            start_ns = start.value

        if end is None:
            # Set `end` to end of data
            end_ns = self._data[-1].ts_init if self._data else 4102444800000000000  # Year 2100-01-01 00:00:00 UTC
            end = unix_nanos_to_dt(end_ns)
        else:
            end = pd.to_datetime(end, utc=True)
            end_ns = end.value

        Condition.is_true(start_ns <= end_ns, "start was > end")
        self._end_ns = end_ns

        # Set clocks
        self._last_ns = start_ns

        cdef TestClock clock
        for clock in get_component_clocks(self._instance_id):
            clock.set_time(start_ns)

        if self._iteration == 0:
            # Initialize run
            self._run_config_id = run_config_id  # Can be None
            self._run_id = UUID4()
            self._run_started = pd.Timestamp.utcnow()
            self._backtest_start = start

            for exchange in self._venues.values():
                exchange.initialize_account()
                open_orders = self._kernel.cache.orders_open(venue=exchange.id)

                for order in open_orders:
                    if order.is_emulated:
                        # Order should be loaded in the emulator already
                        continue

                    matching_engine = exchange.get_matching_engine(order.instrument_id)

                    if matching_engine is None:
                        self._log.error(
                            f"No matching engine for {order.instrument_id} to process {order}",
                        )
                        continue

                    matching_engine.process_order(order, order.account_id)

            # Reset any previously set FORCE_STOP
            set_backtest_force_stop(False)

            # Set start time of all components including logging
            for clock in get_component_clocks(self._instance_id):
                clock.set_time(start_ns)

            set_logging_clock_static_mode()
            set_logging_clock_static_time(start_ns)

            if LOGGING_PYO3:
                nautilus_pyo3.logging_clock_set_static_mode()
                nautilus_pyo3.logging_clock_set_static_time(start_ns)

            # Common kernel start-up sequence
            self._kernel.start()

            self._log_pre_run()

        self._log_run(start, end)

        # Set starting index
        cdef uint64_t i
        self._data_len = len(self._data)

        if self._data_len > 0:
            for i in range(self._data_len):
                if start_ns <= self._data[i].ts_init:
                    self._data_iterator.set_index("backtest_data", i)
                    break

        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        self._last_ns = 0
        cdef uint64_t raw_handlers_count = 0
        cdef Data data = self._data_iterator.next()
        cdef CVec raw_handlers
        try:
            while data is not None:
                if data.ts_init > end_ns:
                    # End of backtest
                    break

                if data.ts_init > self._last_ns:
                    # Advance clocks to the next data time
                    self._last_ns = data.ts_init
                    raw_handlers = self._advance_time(data.ts_init)
                    raw_handlers_count = raw_handlers.len

                # Process data through exchange
                if isinstance(data, Instrument):
                    exchange = self._venues[data.id.venue]
                    exchange.update_instrument(data)
                elif isinstance(data, OrderBookDelta):
                    exchange = self._venues[data.instrument_id.venue]
                    exchange.process_order_book_delta(data)
                elif isinstance(data, OrderBookDeltas):
                    exchange = self._venues[data.instrument_id.venue]
                    exchange.process_order_book_deltas(data)
                elif isinstance(data, OrderBookDepth10):
                    exchange = self._venues[data.instrument_id.venue]
                    exchange.process_order_book_depth10(data)
                elif isinstance(data, QuoteTick):
                    exchange = self._venues[data.instrument_id.venue]
                    exchange.process_quote_tick(data)
                elif isinstance(data, TradeTick):
                    exchange = self._venues[data.instrument_id.venue]
                    exchange.process_trade_tick(data)
                elif isinstance(data, Bar):
                    exchange = self._venues[data.bar_type.instrument_id.venue]
                    exchange.process_bar(data)
                elif isinstance(data, InstrumentClose):
                    exchange = self._venues[data.instrument_id.venue]
                    exchange.process_instrument_close(data)
                elif isinstance(data, InstrumentStatus):
                    exchange = self._venues[data.instrument_id.venue]
                    exchange.process_instrument_status(data)

                self._data_engine.process(data)

                # Process all exchange messages
                for exchange in self._venues.values():
                    exchange.process(data.ts_init)

                data = self._data_iterator.next()

                if data is None or data.ts_init > self._last_ns:
                    # Finally process the time events
                    self._process_raw_time_event_handlers(
                        raw_handlers,
                        self._last_ns,
                        only_now=True,
                    )

                    # Drop processed event handlers
                    vec_time_event_handlers_drop(raw_handlers)
                    raw_handlers_count = 0

                self._iteration += 1
        except AccountError as e:
            set_backtest_force_stop(True)
            self._log.error(f"Stopping backtest from {e}")
            if streaming:
                # Reraise exception to interrupt batch streaming
                raise

        # ---------------------------------------------------------------------#

        if FORCE_STOP:
            return

        # Process remaining messages
        for exchange in self._venues.values():
            exchange.process(self._kernel.clock.timestamp_ns())

        # Process remaining time events
        if raw_handlers_count > 0:
            self._process_raw_time_event_handlers(
                raw_handlers,
                self._last_ns,
                only_now=True,
                as_of_now=True,
            )
            vec_time_event_handlers_drop(raw_handlers)

    cdef CVec _advance_time(self, uint64_t ts_now):
        cdef list[TestClock] clocks = get_component_clocks(self._instance_id)
        cdef TestClock clock

        for clock in clocks:
            time_event_accumulator_advance_clock(
                &self._accumulator,
                &clock._mem,
                ts_now,
                False,
            )

        cdef CVec raw_handlers = time_event_accumulator_drain(&self._accumulator)

        # Handle all events prior to the `ts_now`
        self._process_raw_time_event_handlers(
            raw_handlers,
            ts_now,
            only_now=False,
        )

        # Set all clocks to now
        set_logging_clock_static_time(ts_now)

        if LOGGING_PYO3:
            nautilus_pyo3.logging_clock_set_static_time(ts_now)

        for clock in clocks:
            clock.set_time(ts_now)

        # Return all remaining events to be handled (at `ts_now`)
        return raw_handlers

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cdef void _process_raw_time_event_handlers(
        self,
        CVec raw_handler_vec,
        uint64_t ts_now,
        bint only_now,
        bint as_of_now = False,
    ):
        cdef TimeEventHandler_t* raw_handlers = <TimeEventHandler_t*>raw_handler_vec.ptr
        cdef:
            uint64_t i
            uint64_t ts_event_init
            uint64_t ts_last_init = 0
            TimeEventHandler_t raw_handler
            TimeEvent event
            TestClock clock
            PyObject *raw_callback
            object callback
            SimulatedExchange exchange
        for i in range(raw_handler_vec.len):
            if FORCE_STOP:
                # The FORCE_STOP flag has already been set,
                # no further time events should be processed.
                return

            raw_handler = <TimeEventHandler_t>raw_handlers[i]
            ts_event_init = raw_handler.event.ts_init

            if should_skip_time_event(ts_event_init, ts_now, only_now, as_of_now):
                continue  # Do not process event

            # Set all clocks to event timestamp
            set_logging_clock_static_time(ts_event_init)

            if LOGGING_PYO3:
                nautilus_pyo3.logging_clock_set_static_time(ts_event_init)

            for clock in get_component_clocks(self._instance_id):
                clock.set_time(ts_event_init)

            event = TimeEvent.from_mem_c(raw_handler.event)

            # Cast raw `PyObject *` to a `PyObject`
            raw_callback = <PyObject *>raw_handler.callback_ptr
            callback = <object>raw_callback
            callback(event)

            if ts_event_init != ts_last_init:
                # Process exchange messages
                ts_last_init = ts_event_init

                for exchange in self._venues.values():
                    exchange.process(ts_event_init)

    def _get_log_color_code(self):
        return "\033[36m" if logging_is_colored() else ""

    def _log_pre_run(self):
        if is_logging_pyo3():
            nautilus_pyo3.log_sysinfo(component=type(self).__name__)
        else:
            log_sysinfo(component=type(self).__name__)

        cdef str color = self._get_log_color_code()

        for exchange in self._venues.values():
            account = exchange.exec_client.get_account()
            self._log.info(f"{color}=================================================================")
            self._log.info(f"{color} SimulatedVenue {exchange.id}")
            self._log.info(f"{color}=================================================================")
            self._log.info(f"{repr(account)}")
            self._log.info(f"{color}-----------------------------------------------------------------")
            self._log.info(f"Balances starting:")

            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                for b in account.starting_balances().values():
                    self._log.info(b.to_formatted_str())

    def _log_run(self, start: pd.Timestamp, end: pd.Timestamp):
        cdef str color = self._get_log_color_code()

        self._log.info(f"{color}=================================================================")
        self._log.info(f"{color} BACKTEST RUN")
        self._log.info(f"{color}=================================================================")
        self._log.info(f"Run config ID:  {self._run_config_id}")
        self._log.info(f"Run ID:         {self._run_id}")
        self._log.info(f"Run started:    {format_optional_iso8601(self._run_started)}")
        self._log.info(f"Backtest start: {format_optional_iso8601(self._backtest_start)}")
        self._log.info(f"Batch start:    {format_optional_iso8601(start)}")
        self._log.info(f"Batch end:      {format_optional_iso8601(end)}")
        self._log.info(f"{color}-----------------------------------------------------------------")

    def _log_post_run(self):
        if self._run_finished and self._run_started:
            elapsed_time = self._run_finished - self._run_started
        else:
            elapsed_time = None

        if self._backtest_end and self._backtest_start:
            backtest_range = self._backtest_end - self._backtest_start
        else:
            backtest_range = None

        cdef str color = self._get_log_color_code()

        self._log.info(f"{color}=================================================================")
        self._log.info(f"{color} BACKTEST POST-RUN")
        self._log.info(f"{color}=================================================================")
        self._log.info(f"Run config ID:  {self._run_config_id}")
        self._log.info(f"Run ID:         {self._run_id}")
        self._log.info(f"Run started:    {format_optional_iso8601(self._run_started)}")
        self._log.info(f"Run finished:   {format_optional_iso8601(self._run_finished)}")
        self._log.info(f"Elapsed time:   {elapsed_time}")
        self._log.info(f"Backtest start: {format_optional_iso8601(self._backtest_start)}")
        self._log.info(f"Backtest end:   {format_optional_iso8601(self._backtest_end)}")
        self._log.info(f"Backtest range: {backtest_range}")
        self._log.info(f"Iterations: {self._iteration:_}")
        self._log.info(f"Total events: {self._kernel.exec_engine.event_count:_}")
        self._log.info(f"Total orders: {self._kernel.cache.orders_total_count():_}")

        # Get all positions for venue
        cdef list positions = []

        for position in self._kernel.cache.positions() + self._kernel.cache.position_snapshots():
            positions.append(position)

        self._log.info(f"Total positions: {len(positions):_}")

        if not self._config.run_analysis:
            return

        cdef:
            list venue_positions
            set venue_currencies
        for venue in self._venues.values():
            account = venue.exec_client.get_account()
            self._log.info(f"{color}=================================================================")
            self._log.info(f"{color} SimulatedVenue {venue.id}")
            self._log.info(f"{color}=================================================================")
            self._log.info(f"{repr(account)}")
            self._log.info(f"{color}-----------------------------------------------------------------")
            unrealized_pnls: dict[Currency, Money] | None = None

            if venue.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                if account is None:
                    continue

                self._log.info(f"Balances starting:")

                for b in account.starting_balances().values():
                    self._log.info(b.to_formatted_str())

                self._log.info(f"{color}-----------------------------------------------------------------")
                self._log.info(f"Balances ending:")

                for b in account.balances_total().values():
                    self._log.info(b.to_formatted_str())

                self._log.info(f"{color}-----------------------------------------------------------------")
                self._log.info(f"Commissions:")

                for c in account.commissions().values():
                    self._log.info(Money(-c.as_double(), c.currency).to_formatted_str())  # Display commission as negative

                self._log.info(f"{color}-----------------------------------------------------------------")
                self._log.info(f"Unrealized PnLs (included in totals):")
                unrealized_pnls = self.portfolio.unrealized_pnls(Venue(venue.id.value))

                if not unrealized_pnls:
                    self._log.info("None")
                else:
                    for b in unrealized_pnls.values():
                        self._log.info(b.to_formatted_str())

            # Log output diagnostics for all simulation modules
            for module in venue.modules:
                module.log_diagnostics(self._log)

            self._log.info(f"{color}=================================================================")
            self._log.info(f"{color} PORTFOLIO PERFORMANCE")
            self._log.info(f"{color}=================================================================")

            # Collect all positions and currencies for venue
            venue_positions = []
            venue_currencies = set()

            for position in positions:
                if position.instrument_id.venue == venue.id:
                    venue_positions.append(position)
                    venue_currencies.add(position.quote_currency)

                    if position.base_currency is not None:
                        venue_currencies.add(position.base_currency)

            # Calculate statistics
            self._kernel.portfolio.analyzer.calculate_statistics(account, venue_positions)

            # Present PnL performance stats per asset
            for currency in sorted(list(venue_currencies), key=lambda x: x.code):
                self._log.info(f" PnL Statistics ({str(currency)})")
                self._log.info(f"{color}-----------------------------------------------------------------")
                unrealized_pnl = unrealized_pnls.get(currency) if unrealized_pnls else None

                for stat in self._kernel.portfolio.analyzer.get_stats_pnls_formatted(currency, unrealized_pnl):
                    self._log.info(stat)

                self._log.info(f"{color}-----------------------------------------------------------------")

            self._log.info(" Returns Statistics")
            self._log.info(f"{color}-----------------------------------------------------------------")

            for stat in self._kernel.portfolio.analyzer.get_stats_returns_formatted():
                self._log.info(stat)

            self._log.info(f"{color}-----------------------------------------------------------------")

            self._log.info(" General Statistics")
            self._log.info(f"{color}-----------------------------------------------------------------")

            for stat in self._kernel.portfolio.analyzer.get_stats_general_formatted():
                self._log.info(stat)

            self._log.info(f"{color}-----------------------------------------------------------------")

    def _add_data_client_if_not_exists(self, ClientId client_id) -> None:
        if client_id not in self._kernel.data_engine.registered_clients:
            client = BacktestDataClient(
                client_id=client_id,
                msgbus=self._kernel.msgbus,
                cache=self._kernel.cache,
                clock=self._kernel.clock,
            )
            self._kernel.data_engine.register_client(client)

    def _add_market_data_client_if_not_exists(self, Venue venue) -> None:
        cdef ClientId client_id = ClientId(venue.value)

        if client_id not in self._kernel.data_engine.registered_clients:
            client = BacktestMarketDataClient(
                client_id=client_id,
                msgbus=self._kernel.msgbus,
                cache=self._kernel.cache,
                clock=self._kernel.clock,
            )
            self._kernel.data_engine.register_client(client)

    def set_default_market_data_client(self) -> None:
        cdef ClientId client_id = ClientId("backtest_default_client")
        client = BacktestMarketDataClient(
            client_id=client_id,
            msgbus=self._kernel.msgbus,
            cache=self._kernel.cache,
            clock=self._kernel.clock,
        )
        self._kernel.data_engine.register_client(client)


cdef class BacktestDataIterator:
    """
    Time-ordered multiplexer for historical ``Data`` streams in backtesting.

    The iterator efficiently manages multiple data streams and yields ``Data`` objects
    in strict chronological order based on their ``ts_init`` timestamps. It supports
    both static data lists and dynamic data generators for streaming large datasets.

    **Architecture:**

    - **Single-stream optimization**: When exactly one stream is loaded, uses a fast
      array walk for optimal performance.
    - **Multi-stream merging**: With two or more streams, employs a binary min-heap
      to perform efficient k-way merge sorting.
    - **Dynamic streaming**: Supports Python generators that yield data chunks on-demand,
      enabling processing of datasets larger than available memory.

    **Stream Priority:**

    Streams can be assigned different priorities using the ``append_data`` parameter:

    - ``append_data=True`` (default): Lower priority, processed after existing streams
    - ``append_data=False``: Higher priority, processed before existing streams

    When multiple data points have identical timestamps, higher priority streams
    are yielded first.

    **Performance Characteristics:**

    - **Memory efficient**: Dynamic generators load data incrementally
    - **Time complexity**: O(log n) per item for n streams (heap operations)
    - **Space complexity**: O(k) where k is the total number of active data points
      across all streams at any given time

    Parameters
    ----------
    empty_data_callback : Callable[[str, int], None], optional
        Called once per stream when it is exhausted. Arguments are the stream
        name and the final ``ts_init`` timestamp observed.

    Notes
    -----
    All data within each stream must be pre-sorted by ``ts_init`` in ascending order.
    The iterator assumes this invariant and does not perform additional sorting.

    See Also
    --------
    BacktestEngine.add_data : Add static data to the backtest engine
    BacktestEngine.add_data_iterator : Add streaming data generators

    """
    def __init__(self) -> None:
        self._log = Logger(type(self).__name__)

        self._data = {} # key=data_priority, value=data_list
        self._data_name = {} # key=data_priority, value=data_name
        self._data_priority = {} # key=data_name, value=data_priority
        self._data_len = {} # key=data_priority, value=len(data_list)
        self._data_index = {} # key=data_priority, value=current index of data_list
        self._data_update_function = {} # key=data_priority, value=data_update_function, Callable[[], list] | None

        self._heap = []
        # Counter for assigning priorities to data streams.
        # Incremented before use so that a priority of zero is never assigned.
        self._next_data_priority = 0
        self._reset_single_data()

    cpdef void _reset_single_data(self):
        self._single_data = []
        self._single_data_name = ""
        self._single_data_priority = 0
        self._single_data_len = 0
        self._single_data_index = 0
        self._is_single_data = False

    def add_data(self, data_name, list data, bint append_data=True):
        """
        Add (or replace) a named, pre-sorted data list for static data loading.

        If a stream with the same ``data_name`` already exists, it will be replaced
        with the new data.

        Parameters
        ----------
        data_name : str
            Unique identifier for the data stream.
        data : list[Data]
            Data instances sorted ascending by `ts_init`.
        append_data : bool, default ``True``
            Controls stream priority for timestamp ties:
            ``True`` – lower priority (appended).
            ``False`` – higher priority (prepended).

        Raises
        ------
        ValueError
            If `data_name` is not a valid string.

        """
        Condition.valid_string(data_name, "data_name")

        if not data:
            return

        def data_generator():
            yield data
            # Generator ends after yielding once

        self.init_data(data_name, data_generator(), append_data)

    def init_data(self, str data_name, data_generator, bint append_data=True):
        """
        Add (or replace) a named data generator for streaming large datasets.

        This method enables memory-efficient processing of large datasets by using
        Python generators that yield data chunks on-demand. The generator is called
        incrementally as data is consumed, allowing datasets larger than available
        memory to be processed.

        The generator should yield lists of ``Data`` objects, where each list represents
        a chunk of data. When a chunk is exhausted, the iterator automatically calls
        ``next()`` on the generator to fetch the next chunk.

        Parameters
        ----------
        data_name : str
            Unique identifier for the data stream.
        data_generator : Generator[list[Data], None, None]
            A Python generator that yields lists of ``Data`` instances sorted ascending by `ts_init`.
        append_data : bool, default ``True``
            Controls stream priority for timestamp ties:
            ``True`` – lower priority (appended).
            ``False`` – higher priority (prepended).

        Raises
        ------
        ValueError
            If `data_name` is not a valid string.

        """
        Condition.valid_string(data_name, "data_name")

        cdef list[Data] data

        try:
            data = next(data_generator)

            if data:
                self._data_update_function[data_name] = data_generator
                self._add_data(data_name, data, append_data)
                self._log.debug(f"Added {len(data):_} data elements from iterator '{data_name}'")
        except StopIteration:
            # Generator is already exhausted, nothing to add
            pass

    cdef void _add_data(self, str data_name, list data_list, bint append_data=True):
        if len(data_list) == 0:
            return

        cdef int data_priority

        if data_name in self._data_priority:
            data_priority = self._data_priority[data_name]
            self.remove_data(data_name)
        else:
            # heapq is a min priority queue so smaller values are popped first.
            # Increment the counter *before* applying the sign so that priority
            # zero is never produced (zero would undermine prepend/append
            # semantics when ordering streams).
            self._next_data_priority += 1
            data_priority = (1 if append_data else -1) * self._next_data_priority

        if self._is_single_data:
            self._deactivate_single_data()

        self._data[data_priority] = sorted(data_list, key=lambda data: data.ts_init)
        self._data_name[data_priority] = data_name
        self._data_priority[data_name] = data_priority
        self._data_len[data_priority] = len(data_list)
        self._data_index[data_priority] = 0

        if len(self._data) == 1:
            self._activate_single_data()
            return

        self._push_data(data_priority, 0)

    cpdef void remove_data(self, str data_name, bint complete_remove=False):
        """
        Remove the data stream identified by ``data_name``. The operation is silently
        ignored if the specified stream does not exist.

        Parameters
        ----------
        data_name : str
            The unique identifier of the data stream to remove.
        complete_remove : bool, default False
            Controls the level of cleanup performed:
            - ``False``: Remove stream data but preserve generator function for potential
              re-initialization (useful for temporary stream removal)
            - ``True``: Complete removal including any associated generator function
              (recommended for permanent stream removal)

        Raises
        ------
        ValueError
            If `data_name` is not a valid string.

        """
        Condition.valid_string(data_name, "data_name")

        if data_name not in self._data_priority:
            return

        cdef int data_priority = self._data_priority[data_name]
        del self._data[data_priority]
        del self._data_name[data_priority]
        del self._data_priority[data_name]
        del self._data_len[data_priority]
        del self._data_index[data_priority]

        if complete_remove:
            del self._data_update_function[data_name]

        if len(self._data) == 1:
            self._activate_single_data()
            return

        if len(self._data) == 0:
            self._reset_single_data()
            return

        # rebuild heap excluding data_priority
        self._heap = [item for item in self._heap if item[1] != data_priority]
        heapq.heapify(self._heap)

    cpdef void _activate_single_data(self):
        assert len(self._data) == 1

        cdef str single_data_name = list(self._data_name.values())[0]
        self._single_data_name = single_data_name
        self._single_data_priority = self._data_priority[self._single_data_name]
        self._single_data = self._data[self._single_data_priority]
        self._single_data_len = self._data_len[self._single_data_priority]
        self._single_data_index = self._data_index[self._single_data_priority]
        self._heap = []
        self._is_single_data = True

    cpdef void _deactivate_single_data(self):
        assert len(self._heap) == 0

        if self._single_data_index < self._single_data_len:
            self._data_index[self._single_data_priority] = self._single_data_index
            self._push_data(self._single_data_priority, self._single_data_index)

        self._reset_single_data()

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef Data next(self):
        """
        Return the next ``Data`` object in chronological order.

        This method implements the core iteration logic, yielding data points from
        all streams in strict chronological order based on ``ts_init`` timestamps.
        When multiple data points have identical timestamps, stream priority
        determines the order.

        The method automatically handles:
        - Single-stream optimization for performance
        - Multi-stream heap-based merging
        - Dynamic data loading from generators
        - Stream exhaustion and cleanup

        Returns
        -------
        Data or None
            The next ``Data`` object in chronological order, or ``None`` when
            all streams are exhausted.

        Notes
        -----
        - Returns ``None`` when all streams are exhausted
        - Automatically triggers generator calls for streaming data
        - Performance is optimized for single-stream scenarios
        - Thread-safe only when called from a single thread

        """
        cdef:
            uint64_t ts_init
            int data_priority
            int cursor
            Data object_to_return

        if not self._is_single_data:
            if not self._heap:
                return None

            ts_init, data_priority, cursor = heapq.heappop(self._heap)
            object_to_return = self._data[data_priority][cursor]

            self._data_index[data_priority] += 1
            self._push_data(data_priority, self._data_index[data_priority])

            return object_to_return

        if self._single_data_index >= self._single_data_len:
            return None

        object_to_return = self._single_data[self._single_data_index]
        self._single_data_index += 1

        if self._single_data_index >= self._single_data_len:
            self._update_data(self._single_data_priority)

        return object_to_return

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void _push_data(self, int data_priority, int data_index):
        cdef uint64_t ts_init

        if data_index < self._data_len[data_priority]:
            ts_init = self._data[data_priority][data_index].ts_init
            heapq.heappush(self._heap, (ts_init, data_priority, data_index))
        else:
            self._update_data(data_priority)

    cpdef void _update_data(self, int data_priority):
        cdef str data_name = self._data_name[data_priority]

        if data_name not in self._data_update_function:
            return

        cdef list[Data] data

        try:
            data = next(self._data_update_function[data_name])

            if data:
                # No need for append_data bool as it's an update
                self._add_data(data_name, data)
                self._log.debug(f"Adding {len(data):_} data elements from iterator '{data_name}'")
            else:
                self.remove_data(data_name, complete_remove=True)
        except StopIteration:
            # Generator is exhausted, remove the stream
            self.remove_data(data_name, complete_remove=True)

    cpdef void set_index(self, str data_name, int index):
        """
        Move the cursor of `data_name` to `index` and rebuild ordering.

        Raises
        ------
        ValueError
            If `data_name` is not a valid string.

        """
        Condition.valid_string(data_name, "data_name")

        if data_name not in self._data_priority:
            return

        cdef int data_priority = self._data_priority[data_name]
        self._data_index[data_priority] = index
        self._reset_heap()

    cpdef void _reset_heap(self):
        if len(self._data) == 1:
            self._activate_single_data()
            return

        self._heap = []

        for data_priority, index in self._data_index.items():
            self._push_data(data_priority, index)

    cpdef bint is_done(self):
        """
        Return ``True`` when every stream has been fully consumed.
        """
        if self._is_single_data:
            return self._single_data_index >= self._single_data_len
        else:
            return not self._heap

    cpdef dict all_data(self):
        """
        Return a *shallow* mapping of ``{stream_name: list[Data]}``.
        """
        # we assume dicts are ordered by order of insertion
        return {data_name:self._data[data_priority] for data_priority, data_name in self._data_name.items()}

    cpdef list[Data] data(self, str data_name):
        """
        Return the underlying data list for `data_name`.

        Returns
        -------
        list[Data]

        Raises
        ------
        ValueError
            If `data_name` is not a valid string.
        KeyError
            If the stream is unknown.

        """
        Condition.valid_string(data_name, "data_name")

        return self._data[self._data_priority[data_name]]

    def __iter__(self):
        return self

    def __next__(self):
        cdef Data element
        element = self.next()

        if element is None:
            raise StopIteration

        return element


cdef class SimulatedExchange:
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

    def __init__(
        self,
        Venue venue not None,
        OmsType oms_type,
        AccountType account_type,
        list starting_balances not None,
        Currency base_currency: Currency | None,
        default_leverage not None: Decimal,
        leverages not None: dict[InstrumentId, Decimal],
        list modules not None,
        PortfolioFacade portfolio not None,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        TestClock clock not None,
        FillModel fill_model not None,
        FeeModel fee_model not None,
        LatencyModel latency_model = None,
        MarginModel margin_model = None,
        BookType book_type = BookType.L1_MBP,
        bint frozen_account = False,
        bint reject_stop_orders = True,
        bint support_gtd_orders = True,
        bint support_contingent_orders = True,
        bint use_position_ids = True,
        bint use_random_ids = False,
        bint use_reduce_only = True,
        bint use_message_queue = True,
        bint bar_execution = True,
        bint bar_adaptive_high_low_ordering = False,
        bint trade_execution = False,
    ) -> None:
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(starting_balances, Money, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules", "SimulationModule")
        if base_currency:
            Condition.is_true(len(starting_balances) == 1, "single-currency account has multiple starting currencies")
        if default_leverage and default_leverage > 1 or leverages:
            Condition.is_true(account_type == AccountType.MARGIN, "leverages defined when account type is not `MARGIN`")

        self._clock = clock
        self._log = Logger(name=f"{type(self).__name__}({venue})")

        self.id = venue
        self.oms_type = oms_type
        self._log.info(f"OmsType={oms_type_to_str(oms_type)}")
        self.book_type = book_type

        self.msgbus = msgbus
        self.cache = cache
        self.exec_client = None  # Initialized when execution client registered

        # Accounting
        self.account_type = account_type
        self.base_currency = base_currency
        self.starting_balances = starting_balances
        self.default_leverage = default_leverage
        self.leverages = leverages
        self.margin_model = margin_model
        self.is_frozen_account = frozen_account

        # Execution config
        self.reject_stop_orders = reject_stop_orders
        self.support_gtd_orders = support_gtd_orders
        self.support_contingent_orders = support_contingent_orders
        self.use_position_ids = use_position_ids
        self.use_random_ids = use_random_ids
        self.use_reduce_only = use_reduce_only
        self.use_message_queue = use_message_queue
        self.bar_execution = bar_execution
        self.bar_adaptive_high_low_ordering = bar_adaptive_high_low_ordering
        self.trade_execution = trade_execution

        # Execution models
        self.fill_model = fill_model
        self.fee_model = fee_model
        self.latency_model = latency_model

        # Load modules
        self.modules = []
        for module in modules:
            Condition.not_in(module, self.modules, "module", "modules")
            module.register_venue(self)
            module.register_base(
                portfolio=portfolio,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
            )
            self.modules.append(module)
            self._log.info(f"Loaded {module}")

        # Markets
        self.instruments: dict[InstrumentId, Instrument] = {}
        self._matching_engines: dict[InstrumentId, OrderMatchingEngine] = {}

        self._message_queue = deque()
        self._inflight_queue: list[tuple[(uint64_t, uint64_t), TradingCommand]] = []
        self._inflight_counter: dict[uint64_t, uint64_t] = {}

        # For direct communication from SpreadQuoteAggregator
        spread_quote_endpoint = f"SimulatedExchange.spread_quote.{venue}"
        if spread_quote_endpoint not in self.msgbus._endpoints:
            self.msgbus.register(endpoint=spread_quote_endpoint, handler=self.process_quote_tick)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"id={self.id}, "
            f"oms_type={oms_type_to_str(self.oms_type)}, "
            f"account_type={account_type_to_str(self.account_type)})"
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, BacktestExecClient client):
        """
        Register the given execution client with the simulated exchange.

        Parameters
        ----------
        client : BacktestExecClient
            The client to register

        """
        Condition.not_none(client, "client")

        self.exec_client = client

        self._log.info(f"Registered ExecutionClient-{client}")

    cpdef void set_fill_model(self, FillModel fill_model):
        """
        Set the fill model for all matching engines.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        Condition.not_none(fill_model, "fill_model")

        self.fill_model = fill_model

        cdef OrderMatchingEngine matching_engine
        for matching_engine in self._matching_engines.values():
            matching_engine.set_fill_model(fill_model)
            self._log.info(
                f"Changed `FillModel` for {matching_engine.venue} "
                f"to {self.fill_model}",
            )

    cpdef void set_latency_model(self, LatencyModel latency_model):
        """
        Change the latency model for this exchange.

        Parameters
        ----------
        latency_model : LatencyModel
            The latency model to set.

        """
        Condition.not_none(latency_model, "latency_model")

        self.latency_model = latency_model

        self._log.info("Changed latency model")

    cpdef void initialize_account(self):
        """
        Initialize the account to the starting balances.

        """
        self._generate_fresh_account_state()

    cpdef void add_instrument(self, Instrument instrument):
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
        Condition.not_none(instrument, "instrument")
        Condition.equal(instrument.id.venue, self.id, "instrument.id.venue", "self.id")

        # Validate instrument
        if isinstance(instrument, (CryptoPerpetual, CryptoFuture)):
            if self.account_type == AccountType.CASH:
                raise InvalidConfiguration(
                    f"Cannot add a `{type(instrument).__name__}` type instrument "
                    f"to a venue with a `CASH` account type. Add to a "
                    f"venue with a `MARGIN` account type.",
                )

        self.instruments[instrument.id] = instrument

        cdef OrderMatchingEngine matching_engine = OrderMatchingEngine(
            instrument=instrument,
            raw_id=len(self.instruments),
            fill_model=self.fill_model,
            fee_model=self.fee_model,
            book_type=self.book_type,
            oms_type=self.oms_type,
            account_type=self.account_type,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self._clock,
            reject_stop_orders=self.reject_stop_orders,
            support_gtd_orders=self.support_gtd_orders,
            support_contingent_orders=self.support_contingent_orders,
            use_position_ids=self.use_position_ids,
            use_random_ids=self.use_random_ids,
            use_reduce_only=self.use_reduce_only,
            bar_execution=self.bar_execution,
            bar_adaptive_high_low_ordering=self.bar_adaptive_high_low_ordering,
            trade_execution=self.trade_execution,
        )

        self._matching_engines[instrument.id] = matching_engine

        self._log.info(f"Added instrument {instrument.id} and created matching engine")

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self, InstrumentId instrument_id):
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
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(instrument_id)
        if matching_engine is None:
            return None

        return matching_engine.best_bid_price()

    cpdef Price best_ask_price(self, InstrumentId instrument_id):
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
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(instrument_id)
        if matching_engine is None:
            return None

        return matching_engine.best_ask_price()

    cpdef OrderBook get_book(self, InstrumentId instrument_id):
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
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(instrument_id)
        if matching_engine is None:
            return None

        return matching_engine.get_book()

    cpdef OrderMatchingEngine get_matching_engine(self, InstrumentId instrument_id):
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
        return self._matching_engines.get(instrument_id)

    cpdef dict get_matching_engines(self):
        """
        Return all matching engines for the exchange (for every instrument).

        Returns
        -------
        dict[InstrumentId, OrderMatchingEngine]

        """
        return self._matching_engines.copy()

    cpdef dict get_books(self):
        """
        Return all order books within the exchange.

        Returns
        -------
        dict[InstrumentId, OrderBook]

        """
        cdef dict books = {}

        cdef OrderMatchingEngine matching_engine
        for matching_engine in self._matching_engines.values():
            books[matching_engine.instrument.id] = matching_engine.get_book()

        return books

    cpdef list get_open_orders(self, InstrumentId instrument_id = None):
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
        cdef OrderMatchingEngine matching_engine
        if instrument_id is not None:
            matching_engine = self._matching_engines.get(instrument_id)
            if matching_engine is None:
                return []
            else:
                return matching_engine.get_open_orders()

        cdef list open_orders = []
        for matching_engine in self._matching_engines.values():
            open_orders += matching_engine.get_open_orders()

        return open_orders

    cpdef list get_open_bid_orders(self, InstrumentId instrument_id = None):
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
        cdef OrderMatchingEngine matching_engine
        if instrument_id is not None:
            matching_engine = self._matching_engines.get(instrument_id)
            if matching_engine is None:
                return []
            else:
                return matching_engine.get_open_bid_orders()

        cdef list open_bid_orders = []
        for matching_engine in self._matching_engines.values():
            open_bid_orders += matching_engine.get_open_bid_orders()

        return open_bid_orders

    cpdef list get_open_ask_orders(self, InstrumentId instrument_id = None):
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
        cdef OrderMatchingEngine matching_engine
        if instrument_id is not None:
            matching_engine = self._matching_engines.get(instrument_id)
            if matching_engine is None:
                return []
            else:
                return matching_engine.get_open_ask_orders()

        cdef list open_ask_orders = []
        for matching_engine in self._matching_engines.values():
            open_ask_orders += matching_engine.get_open_ask_orders()

        return open_ask_orders

    cpdef Account get_account(self):
        """
        Return the account for the registered client (if registered).

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(self.exec_client, "self.exec_client")

        return self.exec_client.get_account()

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment):
        """
        Adjust the account at the exchange with the given adjustment.

        Parameters
        ----------
        adjustment : Money
            The adjustment for the account.

        """
        Condition.not_none(adjustment, "adjustment")

        if self.is_frozen_account:
            return  # Nothing to adjust

        cdef Account account = self.cache.account_for_venue(self.exec_client.venue)
        if account is None:
            self._log.error(
                f"Cannot adjust account: no account found for {self.exec_client.venue}"
            )
            return

        cdef AccountBalance balance = account.balance(adjustment.currency)
        if balance is None:
            self._log.error(
                f"Cannot adjust account: no balance found for {adjustment.currency}"
            )
            return

        balance.total = Money(balance.total + adjustment, adjustment.currency)
        balance.free = Money(balance.free + adjustment, adjustment.currency)

        cdef list margins = []
        if account.is_margin_account:
            margins = list(account.margins().values())

        # Generate and handle event
        self.exec_client.generate_account_state(
            balances=[balance],
            margins=margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    cpdef void update_instrument(self, Instrument instrument):
        """
        Update the venues current instrument definition with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument definition to update.

        """
        Condition.not_none(instrument, "instrument")

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(instrument.id)
        if matching_engine is None:
            self.add_instrument(instrument)
            return

        matching_engine.update_instrument(instrument)

    cpdef void send(self, TradingCommand command):
        """
        Send the given trading command into the exchange.

        Parameters
        ----------
        command : TradingCommand
            The command to send.

        """
        Condition.not_none(command, "command")

        if not self.use_message_queue:
            self._process_trading_command(command)
        elif self.latency_model is None:
            self._message_queue.appendleft(command)
        else:
            heappush(self._inflight_queue, self.generate_inflight_command(command))

    cdef tuple generate_inflight_command(self, TradingCommand command):
        cdef uint64_t ts
        if isinstance(command, (SubmitOrder, SubmitOrderList)):
            ts = command.ts_init + self.latency_model.insert_latency_nanos
        elif isinstance(command, ModifyOrder):
            ts = command.ts_init + self.latency_model.update_latency_nanos
        elif isinstance(command, (CancelOrder, CancelAllOrders, BatchCancelOrders)):
            ts = command.ts_init + self.latency_model.cancel_latency_nanos
        else:
            raise ValueError(f"invalid `TradingCommand`, was {command}")  # pragma: no cover (design-time error)

        if ts not in self._inflight_counter:
            self._inflight_counter[ts] = 0

        self._inflight_counter[ts] += 1
        cdef (uint64_t, uint64_t) key = (ts, self._inflight_counter[ts])

        return key, command

    cpdef void process_order_book_delta(self, OrderBookDelta delta):
        """
        Process the exchanges market for the given order book delta.

        Parameters
        ----------
        data : OrderBookDelta
            The order book delta to process.

        """
        Condition.not_none(delta, "delta")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(delta)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(delta.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(delta.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {delta.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[delta.instrument_id]

        matching_engine.process_order_book_delta(delta)

    cpdef void process_order_book_deltas(self, OrderBookDeltas deltas):
        """
        Process the exchanges market for the given order book deltas.

        Parameters
        ----------
        data : OrderBookDeltas
            The order book deltas to process.

        """
        Condition.not_none(deltas, "deltas")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(deltas)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(deltas.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(deltas.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {deltas.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[deltas.instrument_id]

        matching_engine.process_order_book_deltas(deltas)

    cpdef void process_order_book_depth10(self, OrderBookDepth10 depth):
        """
        Process the exchanges market for the given order book depth.

        Parameters
        ----------
        depth : OrderBookDepth10
            The order book depth to process.

        """
        Condition.not_none(depth, "depth")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(depth)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(depth.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(depth.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {depth.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[depth.instrument_id]

        matching_engine.process_order_book_depth10(depth)

    cpdef void process_quote_tick(self, QuoteTick tick):
        """
        Process the exchanges market for the given quote tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : QuoteTick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(tick)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(tick.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(tick.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {tick.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[tick.instrument_id]

        matching_engine.process_quote_tick(tick)

    cpdef void process_trade_tick(self, TradeTick tick):
        """
        Process the exchanges market for the given trade tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : TradeTick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(tick)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(tick.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(tick.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {tick.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[tick.instrument_id]

        matching_engine.process_trade_tick(tick)

    cpdef void process_bar(self, Bar bar):
        """
        Process the exchanges market for the given bar.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        """
        Condition.not_none(bar, "bar")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(bar)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(bar.bar_type.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(bar.bar_type.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {bar.bar_type.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[bar.bar_type.instrument_id]

        matching_engine.process_bar(bar)

    cpdef void process_instrument_status(self, InstrumentStatus data):
        """
        Process a specific instrument status.

        Parameters
        ----------
        data : InstrumentStatus
            The instrument status update to process.

        """
        Condition.not_none(data, "data")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(data)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(data.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(data.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {data.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[data.instrument_id]

        matching_engine.process_status(data.action)

    cpdef void process_instrument_close(self, InstrumentClose close):
        """
        Process the exchanges market for the given instrument close.

        Parameters
        ----------
        close : InstrumentClose
            The instrument close to process.

        """
        Condition.not_none(close, "close")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(close)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(close.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(close.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {close.instrument_id}")

            self.add_instrument(instrument)
            matching_engine = self._matching_engines[close.instrument_id]

        matching_engine.process_instrument_close(close)

    cpdef void process(self, uint64_t ts_now):
        """
        Process the exchange to the given time.

        All pending commands will be processed along with all simulation modules.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).

        """
        self._clock.set_time(ts_now)

        cdef:
            uint64_t ts
        while self._inflight_queue:
            # Peek at timestamp of next in-flight message
            ts = self._inflight_queue[0][0][0]
            if ts <= ts_now:
                # Place message on queue to be processed
                self._message_queue.appendleft(self._inflight_queue.pop(0)[1])
                self._inflight_counter.pop(ts, None)
            else:
                break

        cdef TradingCommand command
        while self._message_queue:
            command = self._message_queue.pop()
            self._process_trading_command(command)

        # Iterate over modules
        cdef SimulationModule module
        for module in self.modules:
            module.process(ts_now)

    cpdef void reset(self):
        """
        Reset the simulated exchange.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting")

        for module in self.modules:
            module.reset()

        self._generate_fresh_account_state()

        for matching_engine in self._matching_engines.values():
            matching_engine.reset()

        self._message_queue = deque()
        self._inflight_queue.clear()
        self._inflight_counter.clear()

        self._log.info("Reset")

    cdef void _process_trading_command(self, TradingCommand command):

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(command.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"Cannot process command: no matching engine for {command.instrument_id}")

        cdef:
            Order order
            list[Order] orders
        if isinstance(command, SubmitOrder):
            matching_engine.process_order(command.order, self.exec_client.account_id)
        elif isinstance(command, SubmitOrderList):
            for order in command.order_list.orders:
                matching_engine.process_order(order, self.exec_client.account_id)
        elif isinstance(command, ModifyOrder):
            # Check if order is in SUBMITTED status or PENDING_UPDATE with previous SUBMITTED status
            # (bracket orders not yet at matching engine)
            order = self.cache.order(command.client_order_id)
            if (order is not None and
                (order.status_c() == OrderStatus.SUBMITTED or
                 (order.status_c() == OrderStatus.PENDING_UPDATE and order._previous_status == OrderStatus.SUBMITTED))):
                # Handle modification locally for bracket orders not yet sent to matching engine
                self._process_modify_submitted_order(command)
            else:
                matching_engine.process_modify(command, self.exec_client.account_id)
        elif isinstance(command, CancelOrder):
            matching_engine.process_cancel(command, self.exec_client.account_id)
        elif isinstance(command, CancelAllOrders):
            matching_engine.process_cancel_all(command, self.exec_client.account_id)
        elif isinstance(command, BatchCancelOrders):
            matching_engine.process_batch_cancel(command, self.exec_client.account_id)

    cdef void _process_modify_submitted_order(self, ModifyOrder command):
        """
        Process modification of an order that is in SUBMITTED status.

        This handles bracket orders (TP/SL) that haven't been sent to the matching engine yet.
        """
        cdef Order order = self.cache.order(command.client_order_id)
        if order is None:
            self._generate_order_modify_rejected(
                command.trader_id,
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                None,
                f"{command.client_order_id!r} not found",
                self.exec_client.account_id,
            )
            return

        # Apply the modification directly to the order
        cdef:
            Quantity new_quantity = command.quantity if command.quantity is not None else order.quantity
            Price new_price = command.price if command.price is not None else (order.price if hasattr(order, 'price') else None)
            Price new_trigger_price = command.trigger_price if command.trigger_price is not None else (order.trigger_price if hasattr(order, 'trigger_price') else None)

        # Generate OrderUpdated event
        self._generate_order_updated(
            order,
            new_quantity,
            new_price,
            new_trigger_price,
        )

    cdef void _generate_order_modify_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
        AccountId account_id,
    ):
        """Generate an OrderModifyRejected event."""
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderModifyRejected event = OrderModifyRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_updated(
        self,
        Order order,
        Quantity quantity,
        Price price,
        Price trigger_price,
    ):
        """Generate an OrderUpdated event."""
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_fresh_account_state(self):
        cdef list balances = [
            AccountBalance(
                total=money,
                locked=Money(0, money.currency),
                free=money,
            )
            for money in self.starting_balances
        ]

        self.exec_client.generate_account_state(
            balances=balances,
            margins=[],
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        # Set leverages and margin model
        cdef Account account = self.get_account()
        if account.is_margin_account:
            account.set_default_leverage(self.default_leverage)

            # Set instrument specific leverages
            for instrument_id, leverage in self.leverages.items():
                account.set_leverage(instrument_id, leverage)

            # Set margin model if provided
            if self.margin_model is not None:
                account.set_margin_model(self.margin_model)


cdef class OrderMatchingEngine:
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

    def __init__(
        self,
        Instrument instrument not None,
        uint32_t raw_id,
        FillModel fill_model not None,
        FeeModel fee_model not None,
        BookType book_type,
        OmsType oms_type,
        AccountType account_type,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        TestClock clock not None,
        bint reject_stop_orders = True,
        bint support_gtd_orders = True,
        bint support_contingent_orders = True,
        bint use_position_ids = True,
        bint use_random_ids = False,
        bint use_reduce_only = True,
        bint bar_execution = True,
        bint bar_adaptive_high_low_ordering = False,
        bint trade_execution = False,
        # auction_match_algo = default_auction_match
    ) -> None:
        self._clock = clock
        self._log = Logger(name=f"{type(self).__name__}({instrument.id.venue})")
        self.msgbus = msgbus
        self.cache = cache

        self.venue = instrument.id.venue
        self.instrument = instrument
        self.raw_id = raw_id
        self.book_type = book_type
        self.oms_type = oms_type
        self.account_type = account_type
        self.market_status = MarketStatus.OPEN

        self._instrument_has_expiration = instrument.instrument_class in EXPIRING_INSTRUMENT_TYPES
        self._instrument_close = None
        self._reject_stop_orders = reject_stop_orders
        self._support_gtd_orders = support_gtd_orders
        self._support_contingent_orders = support_contingent_orders
        self._use_position_ids = use_position_ids
        self._use_random_ids = use_random_ids
        self._use_reduce_only = use_reduce_only
        self._bar_execution = bar_execution
        self._bar_adaptive_high_low_ordering = bar_adaptive_high_low_ordering
        self._trade_execution = trade_execution

        # self._auction_match_algo = auction_match_algo
        self._fill_model = fill_model
        self._fee_model = fee_model
        self._book = OrderBook(
            instrument_id=instrument.id,
            book_type=book_type,
        )
        self._opening_auction_book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L3_MBO,
        )
        self._closing_auction_book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L3_MBO,
        )

        self._account_ids: dict[TraderId, AccountId]  = {}
        self._execution_bar_types: dict[InstrumentId, BarType]  =  {}
        self._execution_bar_deltas: dict[BarType, timedelta]  =  {}
        self._cached_filled_qty: dict[ClientOrderId, Quantity] = {}

        # Market
        self._core = MatchingCore(
            instrument_id=instrument.id,
            price_increment=instrument.price_increment,
            trigger_stop_order=self.trigger_stop_order,
            fill_market_order=self.fill_market_order,
            fill_limit_order=self.fill_limit_order,
        )

        self._target_bid = 0
        self._target_ask = 0
        self._target_last = 0
        self._has_targets = False
        self._last_bid_bar: Bar | None = None
        self._last_ask_bar: Bar | None = None

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"venue={self.venue.value}, "
            f"instrument_id={self.instrument.id.value}, "
            f"raw_id={self.raw_id})"
        )

    cpdef void reset(self):
        self._log.debug(f"Resetting OrderMatchingEngine {self.instrument.id}")

        self._book.clear(0, 0)
        self._account_ids.clear()
        self._execution_bar_types.clear()
        self._execution_bar_deltas.clear()
        self._cached_filled_qty.clear()
        self._core.reset()
        self._target_bid = 0
        self._target_ask = 0
        self._target_last = 0
        self._has_targets = False
        self._last_bid_bar = None
        self._last_ask_bar = None

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

        self._log.info(f"Reset OrderMatchingEngine {self.instrument.id}")

    cpdef void set_fill_model(self, FillModel fill_model):
        """
        Set the fill model to the given model.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        Condition.not_none(fill_model, "fill_model")

        self._fill_model = fill_model

        self._log.debug(f"Changed `FillModel` to {self._fill_model}")

    cpdef void update_instrument(self, Instrument instrument):
        """
        Update the matching engines current instrument definition with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument definition to update.

        """
        Condition.not_none(instrument, "instrument")
        Condition.equal(instrument.id, self.instrument.id, "instrument.id", "self.instrument.id")

        self.instrument = instrument

        self._log.debug(f"Updated instrument definition for {instrument.id}")

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self):
        """
        Return the best bid price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        return self._book.best_bid_price()

    cpdef Price best_ask_price(self):
        """
        Return the best ask price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        return self._book.best_ask_price()

    cpdef OrderBook get_book(self):
        """
        Return the internal order book.

        Returns
        -------
        OrderBook

        """
        return self._book

    cpdef list get_open_orders(self):
        """
        Return the open orders in the matching engine.

        Returns
        -------
        list[Order]

        """
        return self.get_open_bid_orders() + self.get_open_ask_orders()

    cpdef list get_open_bid_orders(self):
        """
        Return the open bid orders in the matching engine.

        Returns
        -------
        list[Order]

        """
        return self._core.get_orders_bid()

    cpdef list get_open_ask_orders(self):
        """
        Return the open ask orders at the exchange.

        Returns
        -------
        list[Order]

        """
        return self._core.get_orders_ask()

    cpdef bint order_exists(self, ClientOrderId client_order_id):
        return self._core.order_exists(client_order_id)

# -- DATA PROCESSING ------------------------------------------------------------------------------

    cpdef void process_order_book_delta(self, OrderBookDelta delta):
        """
        Process the exchanges market for the given order book delta.

        Parameters
        ----------
        delta : OrderBookDelta
            The order book delta to process.

        """
        Condition.not_none(delta, "delta")

        if is_logging_initialized():
            self._log.debug(f"Processing {delta!r}")

        self._book.apply_delta(delta)

        # TODO: WIP to introduce flags
        # if data.flags == TimeInForce.GTC:
        #     self._book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_OPEN:
        #     self._opening_auction_book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_CLOSE:
        #     self._closing_auction_book.apply(data)
        # else:
        #     raise RuntimeError(data.time_in_force)

        self.iterate(delta.ts_init)

    cpdef void process_order_book_deltas(self, OrderBookDeltas deltas):
        """
        Process the exchanges market for the given order book deltas.

        Parameters
        ----------
        delta : OrderBookDeltas
            The order book deltas to process.

        """
        Condition.not_none(deltas, "deltas")

        if is_logging_initialized():
            self._log.debug(f"Processing {deltas!r}")

        self._book.apply_deltas(deltas)

        # TODO: WIP to introduce flags
        # if data.flags == TimeInForce.GTC:
        #     self._book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_OPEN:
        #     self._opening_auction_book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_CLOSE:
        #     self._closing_auction_book.apply(data)
        # else:
        #     raise RuntimeError(data.time_in_force)

        self.iterate(deltas.ts_init)

    cpdef void process_order_book_depth10(self, OrderBookDepth10 depth):
        """
        Process the exchanges market for the given order book depth.

        Parameters
        ----------
        depth : OrderBookDepth10
            The order book depth to process.

        """
        Condition.not_none(depth, "depth")

        if is_logging_initialized():
            self._log.debug(f"Processing {depth!r}")

        self._book.apply_depth(depth)

        self.iterate(depth.ts_init)


    cpdef void process_quote_tick(self, QuoteTick tick) :
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
        Condition.not_none(tick, "tick")

        if is_logging_initialized():
            self._log.debug(f"Processing {tick!r}")

        # Validate precisions
        if tick._mem.bid_price.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {tick.bid_price.precision=} did not match {self.instrument.price_precision=}",
            )
        if tick._mem.ask_price.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {tick.ask_price.precision=} did not match {self.instrument.price_precision=}",
            )
        if tick._mem.bid_size.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {tick.bid_size.precision=} did not match {self.instrument.size_precision=}",
            )
        if tick._mem.ask_size.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {tick.ask_size.precision=} did not match {self.instrument.size_precision=}",
            )

        if self.book_type == BookType.L1_MBP:
            self._book.update_quote_tick(tick)

        self.iterate(tick.ts_init)

    cpdef void process_trade_tick(self, TradeTick tick):
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
        Condition.not_none(tick, "tick")

        if is_logging_initialized():
            self._log.debug(f"Processing {tick!r}")

        # Validate precisions
        if tick._mem.price.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {tick.price.precision=} did not match {self.instrument.price_precision=}",
            )
        if tick._mem.size.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {tick.size.precision=} did not match {self.instrument.size_precision=}",
            )

        if self.book_type == BookType.L1_MBP:
            self._book.update_trade_tick(tick)

        cdef AggressorSide aggressor_side = AggressorSide.NO_AGGRESSOR
        cdef PriceRaw price_raw = tick._mem.price.raw

        self._core.set_last_raw(price_raw)

        if self._trade_execution:
            aggressor_side = tick.aggressor_side

            if aggressor_side == AggressorSide.BUYER:
                self._core.set_ask_raw(price_raw)

                if price_raw < self._core.bid_raw:
                    self._core.set_bid_raw(price_raw)
            elif aggressor_side == AggressorSide.SELLER:
                self._core.set_bid_raw(price_raw)

                if price_raw > self._core.ask_raw:
                    self._core.set_ask_raw(price_raw)
            elif aggressor_side == AggressorSide.NO_AGGRESSOR:
                # Update both bid and ask when no specific aggressor
                if price_raw <= self._core.bid_raw:
                    self._core.set_bid_raw(price_raw)

                if price_raw >= self._core.ask_raw:
                    self._core.set_ask_raw(price_raw)
            else:
                aggressor_side_str = aggressor_side_to_str(aggressor_side)
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"invalid `AggressorSide` for trade execution, was {aggressor_side_str}",  # pragma: no cover
                )

        self.iterate(tick.ts_init, aggressor_side)

    cpdef void process_bar(self, Bar bar):
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
        Condition.not_none(bar, "bar")

        if not self._bar_execution:
            return

        if self.book_type != BookType.L1_MBP:
            return  # Can only process an L1 book with bars

        cdef BarType bar_type = bar.bar_type
        if bar_type.aggregation_source == AggregationSource.INTERNAL:
            return  # Do not process internally aggregated bars

        if bar_type.spec.aggregation == BarAggregation.MONTH:
            return  # Do not process monthly bars (there is no available `timedelta`)

        # Validate precisions
        if bar._mem.open.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.open.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.high.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.high.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.low.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.low.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.close.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.close.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.volume.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {bar.volume.precision=} did not match {self.instrument.size_precision=}",
            )

        cdef InstrumentId instrument_id = bar_type.instrument_id
        cdef BarType execution_bar_type = self._execution_bar_types.get(instrument_id)

        if execution_bar_type is None:
            execution_bar_type = bar_type
            self._execution_bar_types[instrument_id] = bar_type
            self._execution_bar_deltas[bar_type] = bar_type.spec.timedelta

        if execution_bar_type != bar_type:
            bar_type_timedelta = self._execution_bar_deltas.get(bar_type)

            if bar_type_timedelta is None:
                bar_type_timedelta = bar_type.spec.timedelta
                self._execution_bar_deltas[bar_type] = bar_type_timedelta

            if self._execution_bar_deltas[execution_bar_type] >= bar_type_timedelta:
                self._execution_bar_types[instrument_id] = bar_type
            else:
                return

        if is_logging_initialized():
            self._log.debug(f"Processing {bar!r}")

        cdef PriceType price_type = bar_type.spec.price_type
        if price_type == PriceType.LAST or price_type == PriceType.MID:
            self._process_trade_ticks_from_bar(bar)
        elif price_type == PriceType.BID:
            self._last_bid_bar = bar
            self._process_quote_ticks_from_bar()
        elif price_type == PriceType.ASK:
            self._last_ask_bar = bar
            self._process_quote_ticks_from_bar()
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `PriceType`, was {price_type}",  # pragma: no cover
            )

    cpdef void process_status(self, MarketStatusAction status):
        """
        Process the exchange status.

        Parameters
        ----------
        status : MarketStatusAction
            The status action to process.

        """
        # # TODO: Reimplement
        if (self.market_status, status) == (MarketStatus.CLOSED, MarketStatusAction.TRADING):
            self.market_status = MarketStatus.OPEN
        elif (self.market_status, status) == (MarketStatus.CLOSED, MarketStatusAction.PRE_OPEN):
            # Do nothing on pre-market open.
            self.market_status = MarketStatus.OPEN
        # elif (self.market_status, status) == (MarketStatus.PRE_OPEN, MarketStatusAction.PAUSE):
        #     # Opening auction period, run auction match on pre-open auction orderbook
        #     self.process_auction_book(self._opening_auction_book)
        #     self.market_status = status
        # elif (self.market_status, status) == (MarketStatus.PAUSE, MarketStatusAction.OPEN):
        #     # Normal market open
        #     self.market_status = status
        # elif (self.market_status, status) == (MarketStatus.OPEN, MarketStatusAction.PAUSE):
        #     # Closing auction period, run auction match on closing auction orderbook
        #     self.process_auction_book(self._closing_auction_book)
        #     self.market_status = status
        # elif (self.market_status, status) == (MarketStatus.PAUSE, MarketStatusAction.CLOSED):
        #     # Market closed - nothing to do for now
        #     # TODO - should we implement some sort of closing price message here?
        #     self.market_status = status

    cpdef void process_instrument_close(self, InstrumentClose close):
        """
        Process the instrument close.

        Parameters
        ----------
        close : InstrumentClose
            The close price to process.

        """
        if close.instrument_id != self.instrument.id:
            self._log.warning(f"Received instrument close for unknown instrument_id: {close.instrument_id}")
            return

        if close.close_type == InstrumentCloseType.CONTRACT_EXPIRED:
            self._instrument_close = close
            self.iterate(close.ts_init)

    cpdef void process_auction_book(self, OrderBook book):
        Condition.not_none(book, "book")

        cdef:
            list traded_bids
            list traded_asks
        # Perform an auction match on this auction order book
        # traded_bids, traded_asks = self._auction_match_algo(book.bids, book.asks)

        cdef set client_order_ids = {c.value for c in self.cache.client_order_ids()}

        # cdef:
        #     BookOrder order
        #     Order real_order
        #     PositionId venue_position_id
        # # Check filled orders from auction for any client orders and emit fills
        # for order in traded_bids + traded_asks:
        #     if order.order_id in client_order_ids:
        #         real_order = self.cache.order(ClientOrderId(order.order_id))
        #         venue_position_id = self._get_position_id(real_order)
        #         self._generate_order_filled(
        #             real_order,
        #             self._get_venue_order_id(real_order),
        #             venue_position_id,
        #             Quantity(order.size, self.instrument.size_precision),
        #             Price(order.price, self.instrument.price_precision),
        #             self.instrument.quote_currency,
        #             Money(0.0, self.instrument.quote_currency),
        #             LiquiditySide.NO_LIQUIDITY_SIDE,
        #         )

    cdef void _process_trade_ticks_from_bar(self, Bar bar):
        cdef double size_value = max(bar.volume.as_double() / 4.0, self.instrument.size_increment.as_double())
        cdef Quantity size = Quantity(size_value, bar._mem.volume.precision)

        # Create base tick template
        cdef TradeTick tick = self._create_base_trade_tick(bar, size)

        # Process each price point
        cdef bint process_high_first = (
            not self._bar_adaptive_high_low_ordering
            or abs(bar._mem.high.raw - bar._mem.open.raw) < abs(bar._mem.low.raw - bar._mem.open.raw)
        )
        self._process_trade_bar_open(bar, tick)

        if process_high_first:
            self._process_trade_bar_high(bar, tick)
            self._process_trade_bar_low(bar, tick)
        else:
            self._process_trade_bar_low(bar, tick)
            self._process_trade_bar_high(bar, tick)

        self._process_trade_bar_close(bar, tick)

    cdef TradeTick _create_base_trade_tick(self, Bar bar, Quantity size):
        return TradeTick(
            bar.bar_type.instrument_id,
            bar.open,
            size,
            AggressorSide.BUYER if not self._core.is_last_initialized or bar._mem.open.raw > self._core.last_raw else AggressorSide.SELLER,
            self._generate_trade_id(),
            bar.ts_init,
            bar.ts_init,
        )

    cdef void _process_trade_bar_open(self, Bar bar, TradeTick tick):
        if not self._core.is_last_initialized or bar._mem.open.raw != self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with open {bar.open}")

            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.open.raw)

    cdef void _process_trade_bar_high(self, Bar bar, TradeTick tick):
        if bar._mem.high.raw > self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with high {bar.high}")

            tick._mem.price = bar._mem.high
            tick._mem.aggressor_side = AggressorSide.BUYER
            tick._mem.trade_id = trade_id_new(pystr_to_cstr(self._generate_trade_id_str()))
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.high.raw)

    cdef void _process_trade_bar_low(self, Bar bar, TradeTick tick):
        if bar._mem.low.raw < self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with low {bar.low}")

            tick._mem.price = bar._mem.low
            tick._mem.aggressor_side = AggressorSide.SELLER
            tick._mem.trade_id = trade_id_new(pystr_to_cstr(self._generate_trade_id_str()))
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.low.raw)

    cdef void _process_trade_bar_close(self, Bar bar, TradeTick tick):
        if bar._mem.close.raw != self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with close {bar.close}")

            tick._mem.price = bar._mem.close
            tick._mem.aggressor_side = AggressorSide.BUYER if bar._mem.close.raw > self._core.last_raw else AggressorSide.SELLER
            tick._mem.trade_id = trade_id_new(pystr_to_cstr(self._generate_trade_id_str()))
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.close.raw)

    cdef void _process_quote_ticks_from_bar(self):
        if self._last_bid_bar is None or self._last_ask_bar is None:
            return  # Wait for next bar

        if self._last_bid_bar.ts_init != self._last_ask_bar.ts_init:
            return  # Wait for next bar

        cdef double size_increment_f64 = self.instrument.size_increment.as_double()
        cdef double bid_size_value = max(self._last_bid_bar.volume.as_double() / 4.0, size_increment_f64)
        cdef double ask_size_value = max(self._last_ask_bar.volume.as_double() / 4.0, size_increment_f64)
        cdef Quantity bid_size = Quantity(bid_size_value, self._last_bid_bar._mem.volume.precision)
        cdef Quantity ask_size = Quantity(ask_size_value, self._last_ask_bar._mem.volume.precision)

        # Create base tick template
        cdef QuoteTick tick = self._create_base_quote_tick(bid_size, ask_size)

        # Process each price point
        cdef bint process_high_first = (
            not self._bar_adaptive_high_low_ordering
            or abs(self._last_bid_bar._mem.high.raw - self._last_bid_bar._mem.open.raw) < abs(self._last_bid_bar._mem.low.raw - self._last_bid_bar._mem.open.raw)
        )
        self._process_quote_bar_open(tick)

        if process_high_first:
            self._process_quote_bar_high(tick)
            self._process_quote_bar_low(tick)
        else:
            self._process_quote_bar_low(tick)
            self._process_quote_bar_high(tick)

        self._process_quote_bar_close(tick)

        self._last_bid_bar = None
        self._last_ask_bar = None

    cdef QuoteTick _create_base_quote_tick(self, Quantity bid_size, Quantity ask_size):
        return QuoteTick(
            self._book.instrument_id,
            self._last_bid_bar.open,
            self._last_ask_bar.open,
            bid_size,
            ask_size,
            self._last_bid_bar.ts_init,
            self._last_ask_bar.ts_init,
        )

    cdef void _process_quote_bar_open(self, QuoteTick tick):
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    cdef void _process_quote_bar_high(self, QuoteTick tick):
        tick._mem.bid_price = self._last_bid_bar._mem.high
        tick._mem.ask_price = self._last_ask_bar._mem.high
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    cdef void _process_quote_bar_low(self, QuoteTick tick):
        tick._mem.bid_price = self._last_bid_bar._mem.low
        tick._mem.ask_price = self._last_ask_bar._mem.low
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    cdef void _process_quote_bar_close(self, QuoteTick tick):
        tick._mem.bid_price = self._last_bid_bar._mem.close
        tick._mem.ask_price = self._last_ask_bar._mem.close
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    # -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef void process_order(self, Order order, AccountId account_id):
        if self._core.order_exists(order.client_order_id):
            return  # Already processed

        # Index identifiers
        self._account_ids[order.trader_id] = account_id

        cdef uint64_t now_ns
        if self._instrument_has_expiration:
            now_ns = self._clock.timestamp_ns()

            if now_ns < self.instrument.activation_ns:
                self._generate_order_rejected(
                    order,
                    f"Contract {self.instrument.id} not yet active, "
                    f"activation {format_iso8601(unix_nanos_to_dt(self.instrument.activation_ns))}"
                )
                return
            elif now_ns > self.instrument.expiration_ns:
                self._generate_order_rejected(
                    order,
                    f"Contract {self.instrument.id} has expired, "
                    f"expiration {format_iso8601(unix_nanos_to_dt(self.instrument.expiration_ns))}"
                )
                return

        cdef:
            Order parent
            Order contingenct_order
            ClientOrderId client_order_id
        if self._support_contingent_orders and order.parent_order_id is not None:
            parent = self.cache.order(order.parent_order_id)
            assert parent is not None and parent.contingency_type == ContingencyType.OTO, "OTO parent not found"

            if parent.status_c() == OrderStatus.REJECTED and order.is_open_c():
                self._generate_order_rejected(order, f"REJECT OTO from {parent.client_order_id}")
                return  # Order rejected
            elif parent.status_c() == OrderStatus.ACCEPTED or parent.status_c() == OrderStatus.TRIGGERED:
                self._log.info(f"Pending OTO {order.client_order_id} triggers from {parent.client_order_id}")
                return  # Pending trigger

            if order.linked_order_ids is not None:
                # Check contingent orders are still open
                for client_order_id in order.linked_order_ids or []:
                    contingent_order = self.cache.order(client_order_id)

                    if contingent_order is None:
                        raise RuntimeError(f"Cannot find contingent order for {client_order_id!r}")  # pragma: no cover

                    if order.contingency_type == ContingencyType.OCO or order.contingency_type == ContingencyType.OUO:
                        if not order.is_closed_c() and contingent_order.is_closed_c():
                            self._generate_order_rejected(order, f"Contingent order {client_order_id} already closed")
                            return  # Order rejected

        # Check order quantity precision
        if order.quantity._mem.precision != self.instrument.size_precision:
            self._generate_order_rejected(
                order,
                f"Invalid size precision for order {order.client_order_id}, "
                f"was {order.quantity.precision} "
                f"when {self.instrument.id} size precision is {self.instrument.size_precision}"
            )
            return  # Invalid order

        cdef Price price
        if order.has_price_c():
            # Check order price precision
            price = order.price

            if price._mem.precision != self.instrument.price_precision:
                self._generate_order_rejected(
                    order,
                    f"Invalid price precision for order {order.client_order_id}, "
                    f"was {price.precision} "
                    f"when {self.instrument.id} price precision is {self.instrument.price_precision}"
                )
                return  # Invalid order

        cdef Price trigger_price
        if order.has_trigger_price_c():
            # Check order trigger price precision
            trigger_price = order.trigger_price

            if trigger_price._mem.precision != self.instrument.price_precision:
                self._generate_order_rejected(
                    order,
                    f"Invalid trigger price precision for order {order.client_order_id}, "
                    f"was {trigger_price.precision} "
                    f"when {self.instrument.id} price precision is {self.instrument.price_precision}"
                )
                return  # Invalid order

        cdef Price activation_price
        if order.has_activation_price_c():
            # Check order activation price precision
            activation_price = order.activation_price

            if activation_price._mem.precision != self.instrument.price_precision:
                self._generate_order_rejected(
                    order,
                    f"Invalid activation price precision for order {order.client_order_id}, "
                    f"was {activation_price.precision} "
                    f"when {self.instrument.id} price precision is {self.instrument.price_precision}"
                )
                return  # Invalid order

        cdef Position position = self.cache.position_for_order(order.client_order_id)

        cdef PositionId position_id
        if position is None and self.oms_type == OmsType.NETTING:
            position_id = PositionId(f"{order.instrument_id}-{order.strategy_id}")
            position = self.cache.position(position_id)

        # Check not shorting an equity without a MARGIN account
        if (
            order.side == OrderSide.SELL
            and self.account_type != AccountType.MARGIN
            and isinstance(self.instrument, Equity)
            and (position is None or not order.would_reduce_only(position.side, position.quantity))
        ):
            self._generate_order_rejected(
                order,
                f"SHORT SELLING not permitted on a CASH account with position {position} and order {order!r}"
            )
            return  # Cannot short sell

        # Check reduce-only instruction
        if self._use_reduce_only and order.is_reduce_only and not order.is_closed_c():
            if (
                not position
                or position.is_closed_c()
                or (order.is_buy_c() and position.is_long_c())
                or (order.is_sell_c() and position.is_short_c())
            ):
                self._generate_order_rejected(
                    order,
                    f"REDUCE_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"would have increased position",
                )
                return  # Reduce only

        if order.order_type == OrderType.MARKET:
            self._process_market_order(order)
        elif order.order_type == OrderType.MARKET_TO_LIMIT:
            self._process_market_to_limit_order(order)
        elif order.order_type == OrderType.LIMIT:
            self._process_limit_order(order)
        elif order.order_type == OrderType.STOP_MARKET:
            self._process_stop_market_order(order)
        elif order.order_type == OrderType.STOP_LIMIT:
            self._process_stop_limit_order(order)
        elif order.order_type == OrderType.MARKET_IF_TOUCHED:
            self._process_market_if_touched_order(order)
        elif order.order_type == OrderType.LIMIT_IF_TOUCHED:
            self._process_limit_if_touched_order(order)
        elif (
            order.order_type == OrderType.TRAILING_STOP_MARKET
            or order.order_type == OrderType.TRAILING_STOP_LIMIT
        ):
            self._process_trailing_stop_order(order)
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"{order_type_to_str(order.order_type)} "  # pragma: no cover
                f"orders are not supported for backtesting in this version",  # pragma: no cover
            )

    cpdef void process_modify(self, ModifyOrder command, AccountId account_id):
        cdef Order order = self._core.get_order(command.client_order_id)
        if order is None:
            self._generate_order_modify_rejected(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                account_id=account_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"{command.client_order_id!r} not found",
            )
        else:
            self.update_order(
                order,
                command.quantity,
                command.price,
                command.trigger_price,
            )

    cpdef void process_cancel(self, CancelOrder command, AccountId account_id):
        cdef Order order = self._core.get_order(command.client_order_id)
        if order is None:
            self._generate_order_cancel_rejected(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                account_id=account_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"{command.client_order_id!r} not found",
            )
        else:
            if order.is_inflight_c() or order.is_open_c():
                self.cancel_order(order)

    cpdef void process_batch_cancel(self, BatchCancelOrders command, AccountId account_id):
        cdef CancelOrder cancel
        for cancel in command.cancels:
            self.process_cancel(cancel, account_id)

    cpdef void process_cancel_all(self, CancelAllOrders command, AccountId account_id):
        cdef Order order
        for order in self.cache.orders_open(venue=None, instrument_id=command.instrument_id):
            if command.order_side != OrderSide.NO_ORDER_SIDE and command.order_side != order.side:
                continue

            if order.is_inflight_c() or order.is_open_c():
                self.cancel_order(order)

    cdef void _process_market_order(self, MarketOrder order):
        # Check AT_THE_OPEN/AT_THE_CLOSE time in force
        if order.time_in_force == TimeInForce.AT_THE_OPEN or order.time_in_force == TimeInForce.AT_THE_CLOSE:
            self._log.error(
                f"Market auction for time in force {time_in_force_to_str(order.time_in_force)} "
                "is not currently supported",
            )
            # TODO: This functionality needs reimplementing
            # self._process_auction_market_order(order)
            return

        # Check market exists
        if order.side == OrderSide.BUY and not self._core.is_ask_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self._core.is_bid_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Immediately fill marketable order
        self.fill_market_order(order)

    cdef void _process_market_to_limit_order(self, MarketToLimitOrder order):
        # Check market exists
        if order.side == OrderSide.BUY and not self._core.is_ask_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self._core.is_bid_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Immediately fill marketable order
        self.fill_market_order(order)

        if order.is_open_c():
            self.accept_order(order)

    cdef void _process_limit_order(self, LimitOrder order):
        # Check AT_THE_OPEN/AT_THE_CLOSE time in force
        if order.time_in_force == TimeInForce.AT_THE_OPEN or order.time_in_force == TimeInForce.AT_THE_CLOSE:
            self._process_auction_limit_order(order)
            return

        if order.is_post_only and self._core.is_limit_matched(order.side, order.price):
            self._generate_order_rejected(
                order,
                f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                f"limit px of {order.price} would have been a TAKER: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
                True,  # due_post_only
            )
            return  # Invalid price

        # Order is valid and accepted
        self.accept_order(order)

        # Check for immediate fill
        if self._core.is_limit_matched(order.side, order.price):
            # Filling as liquidity taker
            if order.liquidity_side == LiquiditySide.NO_LIQUIDITY_SIDE:
                order.liquidity_side = LiquiditySide.TAKER

            self.fill_limit_order(order)
        elif order.time_in_force == TimeInForce.FOK or order.time_in_force == TimeInForce.IOC:
            self.cancel_order(order)

    cdef void _process_stop_market_order(self, StopMarketOrder order):
        if self._core.is_stop_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price

            self.fill_market_order(order)
            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_stop_limit_order(self, StopLimitOrder order):
        if self._core.is_stop_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"trigger stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price

            self.accept_order(order)
            self._generate_order_triggered(order)

            # Check if immediately marketable
            if self._core.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.TAKER
                self.fill_limit_order(order)

            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_market_if_touched_order(self, MarketIfTouchedOrder order):
        if self._core.is_touch_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price

            self.fill_market_order(order)
            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_limit_if_touched_order(self, LimitIfTouchedOrder order):
        if self._core.is_touch_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"trigger stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price

            self.accept_order(order)
            self._generate_order_triggered(order)

            # Check if immediately marketable
            if self._core.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.TAKER
                self.fill_limit_order(order)

            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_trailing_stop_order(self, Order order):
        assert order.order_type == OrderType.TRAILING_STOP_MARKET \
            or order.order_type == OrderType.TRAILING_STOP_LIMIT

        cdef Price market_price = None
        if order.activation_price is None:
            # If activation price is not given,
            # set the activation price to the last price, and activate order
            market_price = self._core.ask if order.side == OrderSide.BUY else self._core.bid

            if market_price is None:
                # If there is no market price, we cannot process the order
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"cannot process trailing stop, "
                    f"no BID or ASK price for {order.instrument_id} "
                    f"(add quotes or use bars)",
                )

            order.set_activated_c(market_price)
        else:
            # If activation price is given,
            # the activation price should not be in the market, like if_touched orders.
            if self._core.is_touch_triggered(order.side, order.activation_price):
                # NOTE: need to apply 'reject_stop_orders' to activation price?
                if self._reject_stop_orders:
                    self._generate_order_rejected(
                        order,
                        f"{order.type_string_c()} {order.side_string_c()} order "
                        f"activation px of {order.activation_price} was in the market: "
                        f"bid={self._core.bid}, "
                        f"ask={self._core.ask}",
                    )
                    return  # Invalid price

                # if we cannot reject the order, we activate it
                order.set_activated_c(None)

        if order.is_activated:
            if order.has_trigger_price_c() and self._core.is_stop_triggered(order.side, order.trigger_price):
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"trigger stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_auction_market_order(self, MarketOrder order):
        cdef:
            Instrument instrument = self.instrument
            BookOrder book_order = BookOrder(
                side=order.side,
                price=instrument.max_price if order.is_buy_c() else instrument.min_price,
                size=order.quantity,
                order_id=self._clock.timestamp_ns(),
            )
        self._process_auction_book_order(book_order, time_in_force=order.time_in_force)

    cdef void _process_auction_limit_order(self, LimitOrder order):
        cdef:
            Instrument instrument = self.instrument
            BookOrder book_order = BookOrder(
                price=order.price,
                size=order.quantity,
                side=order.side,
                order_id=self._clock.timestamp_ns(),
            )
        self._process_auction_book_order(book_order, time_in_force=order.time_in_force)

    cdef void _process_auction_book_order(self, BookOrder order, TimeInForce time_in_force):
        if time_in_force == TimeInForce.AT_THE_OPEN:
            self._opening_auction_book.add(order, 0, 0, 0)
        elif time_in_force == TimeInForce.AT_THE_CLOSE:
            self._closing_auction_book.add(order, 0, 0, 0)
        else:
            raise RuntimeError(time_in_force)

    cdef void _update_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
    ):
        if self._core.is_limit_matched(order.side, price):
            if order.is_post_only:
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"new limit px of {price} would have been a TAKER: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Cannot update order

            self._generate_order_updated(order, qty, price, None)
            order.liquidity_side = LiquiditySide.TAKER
            self.fill_limit_order(order)  # Immediate fill as TAKER
            return  # Filled

        self._generate_order_updated(order, qty, price, None)

    cdef void _update_stop_market_order(
        self,
        Order order,
        Quantity qty,
        Price trigger_price,
    ):
        if self._core.is_stop_triggered(order.side, trigger_price):
            self._generate_order_modify_rejected(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                account_id=order.account_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=f"{order.type_string_c()} {order.side_string_c()} order "
                f"new stop px of {trigger_price} was in the market: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
            )
            return  # Cannot update order

        self._generate_order_updated(order, qty, None, trigger_price)

    cdef void _update_stop_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ):
        if not order.is_triggered:
            # Updating stop price
            if self._core.is_stop_triggered(order.side, trigger_price):
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"{order.type_string_c()} {order.side_string_c()} order "
                    f"new trigger stop px of {trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Cannot update order
        else:
            # Updating limit price
            if self._core.is_limit_matched(order.side, price):
                if order.is_post_only:
                    self._generate_order_modify_rejected(
                        trader_id=order.trader_id,
                        strategy_id=order.strategy_id,
                        account_id=order.account_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        reason=f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order  "
                        f"new limit px of {price} would have been a TAKER: "
                        f"bid={self._core.bid}, "
                        f"ask={self._core.ask}",
                    )
                    return  # Cannot update order
                else:
                    self._generate_order_updated(order, qty, price, None)
                    order.liquidity_side = LiquiditySide.TAKER
                    self.fill_limit_order(order)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price, trigger_price or order.trigger_price)

    cdef void _update_market_if_touched_order(
        self,
        Order order,
        Quantity qty,
        Price trigger_price,
    ):
        if self._core.is_touch_triggered(order.side, trigger_price):
            self._generate_order_modify_rejected(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                account_id=order.account_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=f"{order.type_string_c()} {order.side_string_c()} order "
                       f"new stop px of {trigger_price} was in the market: "
                       f"bid={self._core.bid}, "
                       f"ask={self._core.ask}",
            )
            return  # Cannot update order

        self._generate_order_updated(order, qty, None, trigger_price)

    cdef void _update_limit_if_touched_order(
        self,
        Order order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ):
        if not order.is_triggered:
            # Updating stop price
            if self._core.is_touch_triggered(order.side, trigger_price):
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"{order.type_string_c()} {order.side_string_c()} order "
                           f"new trigger stop px of {trigger_price} was in the market: "
                           f"bid={self._core.bid}, "
                           f"ask={self._core.ask}",
                )
                return  # Cannot update order
        else:
            # Updating limit price
            if self._core.is_limit_matched(order.side, price):
                if order.is_post_only:
                    self._generate_order_modify_rejected(
                        trader_id=order.trader_id,
                        strategy_id=order.strategy_id,
                        account_id=order.account_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        reason=f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order  "
                               f"new limit px of {price} would have been a TAKER: "
                               f"bid={self._core.bid}, "
                               f"ask={self._core.ask}",
                    )
                    return  # Cannot update order
                else:
                    self._generate_order_updated(order, qty, price, None)
                    order.liquidity_side = LiquiditySide.TAKER
                    self.fill_limit_order(order)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price, trigger_price or order.trigger_price)

    cdef void _update_trailing_stop_market_order(
        self,
        Order order,
        Quantity qty,
        Price trigger_price,
    ):
        if order.is_activated:
            # Activated trailing-stop may not yet have a trigger_price;
            # await next market update to calculate it
            if trigger_price is None:
                return

            self._update_stop_market_order(order, qty, trigger_price)
        elif qty or trigger_price:
            self._generate_order_updated(order, qty, None, trigger_price)

    cdef void _update_trailing_stop_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ):
        if order.is_activated:
            # Activated trailing-stop may not yet have a trigger_price;
            # await next market update to calculate it
            if trigger_price is None:
                return

            self._update_stop_limit_order(order, qty, price, trigger_price)
        elif qty or trigger_price:
            self._generate_order_updated(order, qty, price, trigger_price)

    cdef void _trail_stop_order(self, Order order):
        cdef Price market_price = None

        if not order.is_activated:
            if order.activation_price is None:
                # NOTE
                # The activation price should have been set in OrderMatchingEngine._process_trailing_stop_order()
                # However, the implementation of the emulator bypass this step, and directly call this method through match_order().
                market_price = self.ask if order.side == OrderSide.BUY else self.bid

                if market_price is None:
                    # If there is no market price, we cannot process the order
                    raise RuntimeError(  # pragma: no cover (design-time error)
                        f"cannot process trailing stop, "
                        f"no BID or ASK price for {order.instrument_id} "
                        f"(add quotes or use bars)",
                    )

                order.set_activated_c(market_price)
            elif self._core.is_touch_triggered(order.side, order.activation_price):
                order.set_activated_c(None)
            else:
                return  # Do nothing

        cdef tuple output = TrailingStopCalculator.calculate(
            price_increment=self.instrument.price_increment,
            order=order,
            bid=self._core.bid,
            ask=self._core.ask,
            last=self._core.last,
        )

        cdef Price new_trigger_price = output[0]
        cdef Price new_price = output[1]
        if new_trigger_price is None and new_price is None:
            return  # No updates

        self._generate_order_updated(
            order=order,
            quantity=order.quantity,
            price=new_price,
            trigger_price=new_trigger_price,
        )

# -- ORDER PROCESSING -----------------------------------------------------------------------------

    cpdef void iterate(self, uint64_t timestamp_ns, AggressorSide aggressor_side = AggressorSide.NO_AGGRESSOR):
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
        self._clock.set_time(timestamp_ns)

        cdef Price_t bid
        cdef Price_t ask

        if orderbook_has_bid(&self._book._mem) and aggressor_side == AggressorSide.NO_AGGRESSOR:
            bid = orderbook_best_bid_price(&self._book._mem)
            self._core.set_bid_raw(bid.raw)

        if orderbook_has_ask(&self._book._mem) and aggressor_side == AggressorSide.NO_AGGRESSOR:
            ask = orderbook_best_ask_price(&self._book._mem)
            self._core.set_ask_raw(ask.raw)

        self._core.iterate(timestamp_ns)

        cdef list orders = self._core.get_orders()
        cdef Order order
        for order in orders:
            if order.is_closed_c():
                self._cached_filled_qty.pop(order.client_order_id, None)
                continue

            # Check expiry
            if self._support_gtd_orders:
                if order.expire_time_ns > 0 and timestamp_ns >= order.expire_time_ns:
                    self._core.delete_order(order)
                    self._cached_filled_qty.pop(order.client_order_id, None)
                    self.expire_order(order)
                    continue

            # Manage trailing stop
            if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
                self._trail_stop_order(order)

            # Move market back to targets
            if self._has_targets:
                self._core.set_bid_raw(self._target_bid)
                self._core.set_ask_raw(self._target_ask)
                self._core.set_last_raw(self._target_last)
                self._has_targets = False

        # Reset any targets after iteration
        self._target_bid = 0
        self._target_ask = 0
        self._target_last = 0
        self._has_targets = False

        # Instrument expiration
        if (self._instrument_has_expiration and timestamp_ns >= self.instrument.expiration_ns) or self._instrument_close is not None:
            self._log.info(f"{self.instrument.id} reached expiration")

            # Cancel all open orders
            for order in self.get_open_orders():
                self.cancel_order(order)

            # Close all open positions
            for position in self.cache.positions_open(None, self.instrument.id):
                order = MarketOrder(
                    trader_id=position.trader_id,
                    strategy_id=position.strategy_id,
                    instrument_id=position.instrument_id,
                    client_order_id=ClientOrderId(str(uuid.uuid4())),
                    order_side=Order.closing_side_c(position.side),
                    quantity=position.quantity,
                    init_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                    reduce_only=True,
                    tags=[f"EXPIRATION_{self.venue}_CLOSE"],
                )
                self.cache.add_order(order, position_id=position.id)
                self.fill_market_order(order)

    cpdef void fill_market_order(self, Order order):
        """
        Fill the given *marketable* order.

        Parameters
        ----------
        order : Order
            The order to fill.

        """
        cdef Quantity cached_filled_qty = self._cached_filled_qty.get(order.client_order_id)
        if cached_filled_qty is not None and cached_filled_qty._mem.raw >= order.quantity._mem.raw:
            self._log.debug(
                f"Ignoring fill as already filled pending application of events: "
                f"{cached_filled_qty=}, {order.quantity=}, {order.filled_qty=}, {order.leaves_qty=}",
            )
            return

        cdef PositionId venue_position_id = self._get_position_id(order)
        cdef Position position = None
        if venue_position_id is not None:
            position = self.cache.position(venue_position_id)

        if self._use_reduce_only and order.is_reduce_only and position is None:
            self._log.warning(
                f"Canceling REDUCE_ONLY {order.type_string_c()} "
                f"as would increase position",
            )
            self.cancel_order(order)
            return  # Order canceled

        order.liquidity_side = LiquiditySide.TAKER
        cdef list fills = self.determine_market_fills_with_simulation(order)

        self.apply_fills(
            order=order,
            fills=fills,
            liquidity_side=order.liquidity_side,
            venue_position_id=venue_position_id,
            position=position,
        )

    cdef list determine_market_fills_with_simulation(self, Order order):
        """
        Determine market order fills using FillModel simulation if available.

        This method first checks if the FillModel provides a simulated OrderBook
        for fill simulation. If so, it uses that for fill determination. Otherwise,
        it falls back to the standard market fill logic.
        """
        if self._fill_model is None:
            return self.determine_market_price_and_volume(order)

        # Get current best bid/ask for simulation
        cdef Price best_bid = self._core.bid
        cdef Price best_ask = self._core.ask

        if best_bid is None or best_ask is None:
            return []  # No market available

        # Try to get simulated OrderBook from FillModel
        cdef OrderBook simulated_book = self._fill_model.get_orderbook_for_fill_simulation(
            self.instrument, order, best_bid, best_ask
        )

        if simulated_book is not None:
            # Use simulated OrderBook for fill determination
            fills = simulated_book.simulate_fills(
                order,
                price_prec=self.instrument.price_precision,
                size_prec=self.instrument.size_precision,
                is_aggressive=True,
            )
            # If simulation produced no fills (e.g., custom model removed best levels),
            # fall back to standard market logic to preserve expected behavior.
            if not fills:
                return self.determine_market_price_and_volume(order)
            return fills
        else:
            # Fall back to standard logic
            return self.determine_market_price_and_volume(order)

    cpdef list determine_market_price_and_volume(self, Order order):
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
        cdef list fills = self._book.simulate_fills(
            order,
            price_prec=self.instrument.price_precision,
            size_prec=self.instrument.size_precision,
            is_aggressive=True,
        )

        cdef Price price
        cdef Price triggered_price
        if self._book.book_type == BookType.L1_MBP and fills:
            triggered_price = order.get_triggered_price_c()

            if order.order_type == OrderType.MARKET or order.order_type == OrderType.MARKET_TO_LIMIT or order.order_type == OrderType.MARKET_IF_TOUCHED:
                if order.side == OrderSide.BUY:
                    if self._core.is_ask_initialized:
                        price = self._core.ask
                    else:
                        price = self.best_ask_price()

                    if triggered_price:
                        price = triggered_price

                    if price is not None:
                        self._core.set_last_raw(price._mem.raw)
                        fills[0] = (price, fills[0][1])
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best ASK price was None when filling MARKET order",  # pragma: no cover
                        )
                elif order.side == OrderSide.SELL:
                    if self._core.is_bid_initialized:
                        price = self._core.bid
                    else:
                        price = self.best_bid_price()

                    if triggered_price:
                        price = triggered_price

                    if price is not None:
                        self._core.set_last_raw(price._mem.raw)
                        fills[0] = (price, fills[0][1])
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best BID price was None when filling MARKET order",  # pragma: no cover
                        )
            else:
                price = order.price if (order.order_type == OrderType.LIMIT or order.order_type == OrderType.LIMIT_IF_TOUCHED) else order.trigger_price

                if triggered_price:
                    price = triggered_price

                if order.side == OrderSide.BUY:
                    self._core.set_ask_raw(price._mem.raw)
                elif order.side == OrderSide.SELL:
                    self._core.set_bid_raw(price._mem.raw)
                else:
                    raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

                self._core.set_last_raw(price._mem.raw)
                fills[0] = (price, fills[0][1])

        return fills

    cpdef void fill_limit_order(self, Order order):
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
        Condition.is_true(order.has_price_c(), "order has no limit `price`")

        cdef Quantity cached_filled_qty = self._cached_filled_qty.get(order.client_order_id)
        if cached_filled_qty is not None and cached_filled_qty._mem.raw >= order.quantity._mem.raw:
            self._log.debug(
                f"Ignoring fill as already filled pending application of events: "
                f"{cached_filled_qty=}, {order.quantity=}, {order.filled_qty=}, {order.leaves_qty=}",
            )
            return

        cdef Price price = order.price
        if order.liquidity_side == LiquiditySide.MAKER and self._fill_model:
            if order.side == OrderSide.BUY and self._core.bid_raw == price._mem.raw and not self._fill_model.is_limit_filled():
                return  # Not filled
            elif order.side == OrderSide.SELL and self._core.ask_raw == price._mem.raw and not self._fill_model.is_limit_filled():
                return  # Not filled

        cdef PositionId venue_position_id = self._get_position_id(order)
        cdef Position position = None
        if venue_position_id is not None:
            position = self.cache.position(venue_position_id)

        if self._use_reduce_only and order.is_reduce_only and position is None:
            self._log.warning(
                f"Canceling REDUCE_ONLY {order.type_string_c()} "
                f"as would increase position",
            )
            self.cancel_order(order)
            return  # Order canceled

        cdef list fills = self.determine_limit_fills_with_simulation(order)

        self.apply_fills(
            order=order,
            fills=fills,
            liquidity_side=order.liquidity_side,
            venue_position_id=venue_position_id,
            position=position,
        )

    cdef list determine_limit_fills_with_simulation(self, Order order):
        """
        Determine limit order fills using FillModel simulation if available.

        This method first checks if the FillModel provides a simulated OrderBook
        for fill simulation. If so, it uses that for fill determination. Otherwise,
        it falls back to the standard limit fill logic.
        """
        if self._fill_model is None:
            return self.determine_limit_price_and_volume(order)

        # Get current best bid/ask for simulation
        cdef Price best_bid = self._core.bid
        cdef Price best_ask = self._core.ask

        if best_bid is None or best_ask is None:
            return []  # No market available

        # Try to get simulated OrderBook from FillModel
        cdef OrderBook simulated_book = self._fill_model.get_orderbook_for_fill_simulation(
            self.instrument, order, best_bid, best_ask
        )

        if simulated_book is not None:
            # Use simulated OrderBook for fill determination
            return simulated_book.simulate_fills(
                order,
                price_prec=self.instrument.price_precision,
                size_prec=self.instrument.size_precision,
                is_aggressive=False,
            )
        else:
            # Fall back to standard logic
            return self.determine_limit_price_and_volume(order)

    cpdef list determine_limit_price_and_volume(self, Order order):
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
        Condition.is_true(order.has_price_c(), "order has no limit `price`")

        cdef list fills = self._book.simulate_fills(
            order,
            price_prec=self.instrument.price_precision,
            size_prec=self.instrument.size_precision,
            is_aggressive=False,
        )

        cdef Price triggered_price = order.get_triggered_price_c()
        cdef Price price = order.price

        if (
                fills
                and triggered_price is not None
                and order.liquidity_side == LiquiditySide.TAKER
        ):
            ########################################################################
            # Filling as TAKER from a trigger
            ########################################################################
            if order.side == OrderSide.BUY and price._mem.raw > triggered_price._mem.raw:
                fills[0] = (triggered_price, fills[0][1])
                self._has_targets = True
                self._target_bid = self._core.bid_raw
                self._target_ask = self._core.ask_raw
                self._target_last = self._core.last_raw
                self._core.set_ask_raw(price._mem.raw)
                self._core.set_last_raw(price._mem.raw)
            elif order.side == OrderSide.SELL and price._mem.raw < triggered_price._mem.raw:
                fills[0] = (triggered_price, fills[0][1])
                self._has_targets = True
                self._target_bid = self._core.bid_raw
                self._target_ask = self._core.ask_raw
                self._target_last = self._core.last_raw
                self._core.set_bid_raw(price._mem.raw)
                self._core.set_last_raw(price._mem.raw)

        cdef tuple[Price, Quantity] fill
        cdef Price last_px
        if (
                fills
                and order.liquidity_side == LiquiditySide.MAKER
        ):
            ########################################################################
            # Filling as MAKER
            ########################################################################
            price = order.price

            if order.side == OrderSide.BUY:
                if triggered_price and price > triggered_price:
                    price = triggered_price

                for fill in fills:
                    last_px = fill[0]

                    if last_px._mem.raw < price._mem.raw:
                        # Marketable BUY would have filled at limit
                        self._has_targets = True
                        self._target_bid = self._core.bid_raw
                        self._target_ask = self._core.ask_raw
                        self._target_last = self._core.last_raw
                        self._core.set_ask_raw(price._mem.raw)
                        self._core.set_last_raw(price._mem.raw)
                        last_px._mem.raw = price._mem.raw
            elif order.side == OrderSide.SELL:
                if triggered_price and price < triggered_price:
                    price = triggered_price
                for fill in fills:
                    last_px = fill[0]

                    if last_px._mem.raw > price._mem.raw:
                        # Marketable SELL would have filled at limit
                        self._has_targets = True
                        self._target_bid = self._core.bid_raw
                        self._target_ask = self._core.ask_raw
                        self._target_last = self._core.last_raw
                        self._core.set_bid_raw(price._mem.raw)
                        self._core.set_last_raw(price._mem.raw)
                        last_px._mem.raw = price._mem.raw
            else:
                raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

        return fills

    cpdef void apply_fills(
        self,
        Order order,
        list fills,
        LiquiditySide liquidity_side,
        PositionId venue_position_id: PositionId | None = None,
        Position position: Position | None = None,
    ):
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
        Condition.not_none(order, "order")
        Condition.not_none(fills, "fills")
        Condition.not_equal(liquidity_side, LiquiditySide.NO_LIQUIDITY_SIDE, "liquidity_side", "NO_LIQUIDITY_SIDE")

        order.liquidity_side = liquidity_side

        cdef:
            Price fill_px
            Quantity fill_qty
            QuantityRaw total_size_raw = 0
        if order.time_in_force == TimeInForce.FOK:
            # Check FOK requirement
            for fill in fills:
                fill_px, fill_qty = fill
                total_size_raw += fill_qty._mem.raw

            if order.leaves_qty._mem.raw > total_size_raw:
                self.cancel_order(order)
                return  # Cannot fill full size - so kill/cancel

        if not fills:
            if order.status_c() == OrderStatus.SUBMITTED:
                self._generate_order_rejected(order, f"no market with no fills for {order.instrument_id}")
            else:
                self._log.error(
                    "Cannot fill order: no fills from book when fills were expected (check data)",
                )
            return  # No fills

        if self.oms_type == OmsType.NETTING:
            venue_position_id = None  # No position IDs generated by the venue

        if is_logging_initialized():
            self._log.debug(
                "Market: "
                f"bid={self._book.best_bid_size()} @ {self._book.best_bid_price()}, "
                f"ask={self._book.best_ask_size()} @ {self._book.best_ask_price()}, "
                f"last={self._core.last}",
            )
            self._log.debug(
                f"Applying fills to {order}, "
                f"venue_position_id={venue_position_id}, "
                f"position={position}, "
                f"fills={fills}",
            )

        cdef:
            bint initial_market_to_limit_fill = False
            Price last_fill_px = None
        for fill_px, fill_qty in fills:
            # Validate price precision
            if fill_px._mem.precision != self.instrument.price_precision:
                raise RuntimeError(
                    f"Invalid price precision for fill {fill_px.precision} "
                    f"when instrument price precision is {self.instrument.price_precision}. "
                    f"Check that the data price precision matches the {self.instrument.id} instrument"
                )

            # Validate size precision
            if fill_qty._mem.precision != self.instrument.size_precision:
                raise RuntimeError(
                    f"Invalid size precision for fill {fill_qty.precision} "
                    f"when instrument size precision is {self.instrument.size_precision}. "
                    f"Check that the data size precision matches the {self.instrument.id} instrument"
                )

            if order.filled_qty._mem.raw == 0:
                if order.order_type == OrderType.MARKET_TO_LIMIT:
                    self._generate_order_updated(
                        order,
                        qty=order.quantity,
                        price=fill_px,
                        trigger_price=None,
                    )
                    initial_market_to_limit_fill = True

            if self.book_type == BookType.L1_MBP and self._fill_model.is_slipped():
                if order.side == OrderSide.BUY:
                    fill_px = fill_px.add(self.instrument.price_increment)
                elif order.side == OrderSide.SELL:
                    fill_px = fill_px.sub(self.instrument.price_increment)
                else:
                    raise ValueError(  # pragma: no cover (design-time error)
                        f"invalid `OrderSide`, was {order.side}",  # pragma: no cover (design-time error)
                    )

            # Check reduce only order
            if self._use_reduce_only and order.is_reduce_only and fill_qty._mem.raw > position.quantity._mem.raw:
                if position.quantity._mem.raw == 0:
                    return  # Done

                # Adjust fill to honor reduce only execution (fill remaining position size only)
                fill_qty = Quantity.from_raw_c(position.quantity._mem.raw, fill_qty._mem.precision)

                self._generate_order_updated(
                    order=order,
                    qty=fill_qty,
                    price=None,
                    trigger_price=None,
                )

            if fill_qty._mem.raw == 0:
                if len(fills) == 1 and order.status_c() == OrderStatus.SUBMITTED:
                    self._generate_order_rejected(order, f"no market for {order.instrument_id}")

                return  # Done

            self.fill_order(
                order=order,
                last_px=fill_px,
                last_qty=fill_qty,
                liquidity_side=order.liquidity_side,
                venue_position_id=venue_position_id,
                position=position,
            )
            if order.order_type == OrderType.MARKET_TO_LIMIT and initial_market_to_limit_fill:
                return  # Filled initial level

            last_fill_px = fill_px

        if order.time_in_force == TimeInForce.IOC and order.is_open_c():
            # IOC order has filled all available size
            self.cancel_order(order)
            return

        # Check MARKET order on exhausted book volume
        if (
            order.is_open_c()
            and self.book_type == BookType.L1_MBP
            and (
            order.order_type == OrderType.MARKET
            or order.order_type == OrderType.MARKET_IF_TOUCHED
            or order.order_type == OrderType.STOP_MARKET
            or order.order_type == OrderType.TRAILING_STOP_MARKET
        )
        ):
            # Exhausted simulated book volume (continue aggressive filling into next level)
            # This is a very basic implementation of slipping by a single tick, in the future
            # we will implement more detailed fill modeling.
            if order.side == OrderSide.BUY:
                fill_px = last_fill_px.add(self.instrument.price_increment)
            elif order.side == OrderSide.SELL:
                fill_px = last_fill_px.sub(self.instrument.price_increment)
            else:
                raise ValueError(  # pragma: no cover (design-time error)
                    f"invalid `OrderSide`, was {order.side}",  # pragma: no cover (design-time error)
                )

            self.fill_order(
                order=order,
                last_px=fill_px,
                last_qty=order.leaves_qty,
                liquidity_side=order.liquidity_side,
                venue_position_id=venue_position_id,
                position=position,
            )

        # Check LIMIT order on exhausted book volume
        if (
            order.is_open_c()
            and self.book_type == BookType.L1_MBP
            and (
            order.order_type == OrderType.LIMIT
            or order.order_type == OrderType.LIMIT_IF_TOUCHED
            or order.order_type == OrderType.MARKET_TO_LIMIT
            or order.order_type == OrderType.STOP_LIMIT
            or order.order_type == OrderType.TRAILING_STOP_LIMIT
        )
        ):
            if not self._has_targets and ((order.side == OrderSide.BUY and order.price == self._core.ask) or (order.side == OrderSide.SELL and order.price == self._core.bid)):
                return  # Limit price is equal to top-of-book, no further fills

            if order.liquidity_side == LiquiditySide.MAKER:
                # Market moved through limit price, assumption is there was enough liquidity to fill entire order
                fill_px = order.price
            else:  # Marketable limit order
                # Exhausted simulated book volume (continue aggressive filling into next level)
                # This is a very basic implementation of slipping by a single tick, in the future
                # we will implement more detailed fill modeling.
                if order.side == OrderSide.BUY:
                    fill_px = last_fill_px.add(self.instrument.price_increment)
                elif order.side == OrderSide.SELL:
                    fill_px = last_fill_px.sub(self.instrument.price_increment)
                else:
                    raise ValueError(  # pragma: no cover (design-time error)
                        f"invalid `OrderSide`, was {order.side}",  # pragma: no cover (design-time error)
                    )

            self.fill_order(
                order=order,
                last_px=fill_px,
                last_qty=order.leaves_qty,
                liquidity_side=order.liquidity_side,
                venue_position_id=venue_position_id,
                position=position,
            )

        # Generate leg fills for spread orders after normal combo fill processing
        if order.instrument_id.is_spread() and order.is_closed_c():
            self._generate_spread_leg_fills(order, fills, liquidity_side)

    cdef void _generate_spread_leg_fills(
        self,
        Order order,
        list fills,
        LiquiditySide liquidity_side,
    ):
        """
        Generate individual leg fills for position tracking after spread order is filled.

        This method generates synthetic leg fills with "-LEG-" identifiers that will be
        handled by the ExecutionEngine for position tracking, following the IB pattern.
        """
        if not fills:
            return

        # Parse spread legs from instrument ID
        leg_tuples = order.instrument_id.to_list()
        spread_instrument_ids = [leg[0] for leg in leg_tuples]

        spread_fill_px = fills[0][0]
        spread_fill_qty = fills[0][1]

        # Calculate leg execution prices
        leg_prices = self._calculate_leg_execution_prices(
            leg_tuples=leg_tuples,
            spread_execution_price=spread_fill_px,
            spread_quantity=spread_fill_qty,
        )

        if not leg_prices:
            self._log.warning(f"Could not calculate leg prices for spread {order.instrument_id}")
            return

        # Generate fills for each leg
        for leg_instrument_id, ratio in leg_tuples:
            if leg_instrument_id not in leg_prices:
                continue

            leg_price = leg_prices[leg_instrument_id]

            # Calculate leg quantity: spread_quantity * abs(ratio)
            leg_quantity = Quantity(
                spread_fill_qty.as_double() * abs(ratio),
                precision=spread_fill_qty.precision,
            )

            # Get leg instrument for precision validation
            leg_instrument = self.cache.instrument(leg_instrument_id)

            if leg_instrument is None:
                self._log.warning(f"Leg instrument not found in cache: {leg_instrument_id}")
                continue

            # Generate synthetic leg fill directly
            adjusted_leg_price = leg_price

            # Use make_qty for proper size increment rounding
            adjusted_leg_quantity = leg_instrument.make_qty(
                leg_quantity.as_double(),
                round_down=True,  # Round down to ensure valid size
            )

            # Calculate commission for the leg
            commission = self._fee_model.get_commission(
                order=order,  # Use spread order for fee calculation context
                fill_qty=adjusted_leg_quantity,
                fill_px=adjusted_leg_price,
                instrument=leg_instrument,
            )

            # Generate unique IDs for the leg fill (following IB adapter pattern)
            # Get leg position in spread for unique identification
            leg_position = spread_instrument_ids.index(leg_instrument_id) if leg_instrument_id in spread_instrument_ids else 0

            # Generate unique client order ID for leg fill (avoids order state conflicts)
            leg_client_order_id = ClientOrderId(f"{order.client_order_id.value}-LEG-{leg_instrument_id.symbol.value}")

            # Generate unique venue order ID for leg fill
            leg_venue_order_id = VenueOrderId(f"{order.venue_order_id.value}-LEG-{leg_position}")

            # Generate unique trade ID for the leg fill (matching IB pattern: {execution.execId}-{leg_position})
            # Use the same base execution ID format as combo fills but append leg position
            leg_trade_id = TradeId(f"{self.venue.to_str()}-{self.raw_id}-{self._execution_count:03d}-{leg_position}")

            # Leg side mapping based on spread order direction
            # If spread BUY: positive ratio = BUY leg, negative = SELL leg
            # If spread SELL: positive ratio = SELL leg, negative = BUY leg
            order_side = order.side if ratio > 0 else (OrderSide.SELL if order.side == OrderSide.BUY else OrderSide.BUY)

            # Create OrderFilled event for the leg
            ts_now = self._clock.timestamp_ns()
            leg_fill = OrderFilled(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                instrument_id=leg_instrument_id,
                client_order_id=leg_client_order_id,  # Use unique leg client order ID
                venue_order_id=leg_venue_order_id,  # Use unique leg venue order ID
                account_id=order.account_id,
                trade_id=leg_trade_id,
                order_side=order_side,
                order_type=order.order_type,
                last_qty=adjusted_leg_quantity,
                last_px=adjusted_leg_price,
                currency=leg_instrument.quote_currency,
                liquidity_side=liquidity_side,
                event_id=UUID4(),
                ts_event=ts_now,
                ts_init=ts_now,
                reconciliation=False,
                position_id=None,
                commission=commission,
            )

            # Publish the leg fill event (same as regular order fills)
            self.msgbus.send(endpoint="ExecEngine.process", msg=leg_fill)

    cdef dict _calculate_leg_execution_prices(
        self,
        list leg_tuples,
        Price spread_execution_price,
        Quantity spread_quantity,
    ):
        """
        Calculate leg execution prices using mid-prices with adjustment.

        Uses mid-price for all legs except the highest-priced one, which is
        adjusted to satisfy: Σ(leg_price × ratio) = spread_execution_price
        """
        cdef dict leg_mid_prices = {}
        cdef dict leg_prices = {}
        cdef double highest_mid_price = 0.0
        cdef InstrumentId highest_price_leg_id = None

        # Get mid-prices for all legs
        for leg_instrument_id, ratio in leg_tuples:
            leg_quote = self.cache.quote_tick(leg_instrument_id)

            if leg_quote is None:
                self._log.warning(f"No quote available for leg {leg_instrument_id}")
                return {}

            mid_price = (leg_quote.bid_price.as_double() + leg_quote.ask_price.as_double()) * 0.5
            leg_mid_prices[leg_instrument_id] = mid_price

            # Track the leg with highest mid-price (this will be adjusted)
            if mid_price > highest_mid_price:
                highest_mid_price = mid_price
                highest_price_leg_id = leg_instrument_id

        if highest_price_leg_id is None:
            return {}

        # Calculate weighted sum using mid-prices for all legs except the highest
        cdef double weighted_sum = 0.0
        cdef int highest_price_ratio = 1

        for leg_instrument_id, ratio in leg_tuples:
            if leg_instrument_id != highest_price_leg_id:
                weighted_sum += leg_mid_prices[leg_instrument_id] * ratio

                # Get actual instrument to use its make_price method for proper tick rounding
                leg_instrument = self.cache.instrument(leg_instrument_id)

                if leg_instrument is not None:
                    leg_prices[leg_instrument_id] = leg_instrument.make_price(
                        leg_mid_prices[leg_instrument_id]
                    )
                else:
                    # If instrument not found, log warning and abort
                    self._log.warning(
                        f"Cannot find leg instrument {leg_instrument_id} in cache, "
                        f"aborting leg price calculation for spread"
                    )
                    return {}
            else:
                # Store the ratio for the highest-priced leg for adjustment calculation
                highest_price_ratio = ratio

        # Calculate adjusted price for the highest-priced leg
        # spread_execution_price = Σ(leg_price × ratio)
        # adjusted_price = (spread_execution_price - weighted_sum) / highest_price_ratio
        cdef double adjusted_price = (spread_execution_price.as_double() - weighted_sum) / highest_price_ratio

        # Get actual instrument for highest-priced leg to use its make_price method
        highest_leg_instrument = self.cache.instrument(highest_price_leg_id)

        if highest_leg_instrument is not None:
            leg_prices[highest_price_leg_id] = highest_leg_instrument.make_price(adjusted_price)
        else:
            # If instrument not found, log warning and abort
            self._log.warning(
                f"Cannot find highest-priced leg instrument {highest_price_leg_id} in cache, "
                f"aborting leg price calculation for spread"
            )
            return {}

        return leg_prices

    cpdef void fill_order(
        self,
        Order order,
        Price last_px,
        Quantity last_qty,
        LiquiditySide liquidity_side,
        PositionId venue_position_id: PositionId | None = None,
        Position position: Position | None = None,
    ):
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
        Condition.not_none(order, "order")
        Condition.not_none(last_px, "last_px")
        Condition.not_none(last_qty, "last_qty")
        Condition.not_equal(liquidity_side, LiquiditySide.NO_LIQUIDITY_SIDE, "liquidity_side", "NO_LIQUIDITY_SIDE")

        order.liquidity_side = liquidity_side

        cdef Quantity cached_filled_qty = self._cached_filled_qty.get(order.client_order_id)
        cdef Quantity leaves_qty = None
        if cached_filled_qty is None:
            # Clamp the first fill to the order quantity to avoid over-filling
            last_qty = Quantity.from_raw_c(min(order.quantity._mem.raw, last_qty._mem.raw), last_qty._mem.precision)
            self._cached_filled_qty[order.client_order_id] = Quantity.from_raw_c(last_qty._mem.raw, last_qty._mem.precision)
        else:
            leaves_qty = Quantity.from_raw_c(order.quantity._mem.raw - cached_filled_qty._mem.raw, last_qty._mem.precision)
            last_qty = Quantity.from_raw_c(min(leaves_qty._mem.raw, last_qty._mem.raw), last_qty._mem.precision)
            cached_filled_qty._mem.raw += last_qty._mem.raw

        # Nothing to fill when adjusted last_qty <= 0.
        # Update _cached_filled_qty first to absorb duplicate or out-of-order fills
        # (seen in sandbox/async environments) and avoid emitting zero/negative fills.
        if last_qty <= 0:
            return

        # Calculate commission
        cdef Money commission = self._fee_model.get_commission(
            order=order,
            fill_qty=last_qty,
            fill_px=last_px,
            instrument=self.instrument,
        )

        self._generate_order_filled(
            order=order,
            venue_order_id=self._get_venue_order_id(order),
            venue_position_id=venue_position_id,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=self.instrument.quote_currency,
            commission=commission,
            liquidity_side=order.liquidity_side,
        )

        if order.is_passive_c() and order.is_closed_c():
            # Remove order from market
            self._core.delete_order(order)
            self._cached_filled_qty.pop(order.client_order_id, None)

        if not self._support_contingent_orders:
            return

        # Check contingent orders
        cdef ClientOrderId client_order_id
        cdef Order child_order
        if order.contingency_type == ContingencyType.OTO:
            for client_order_id in order.linked_order_ids or []:
                child_order = self.cache.order(client_order_id)
                assert child_order is not None, "OTO child order not found"

                if child_order.is_closed_c():
                    continue

                if child_order.is_active_local_c():
                    continue  # Order is not on the exchange yet

                if child_order.position_id is None and order.position_id is not None:
                    self.cache.add_position_id(
                        position_id=order.position_id,
                        venue=self.venue,
                        client_order_id=client_order_id,
                        strategy_id=child_order.strategy_id,
                    )
                    self._log.debug(
                        f"Indexed {order.position_id!r} "
                        f"for {child_order.client_order_id!r}",
                    )
                if not child_order.is_open_c() or (child_order.status_c() == OrderStatus.PENDING_UPDATE and child_order._previous_status == OrderStatus.SUBMITTED):
                    self.process_order(
                        order=child_order,
                        account_id=order.account_id or self._account_ids[order.trader_id],
                    )
        elif order.contingency_type == ContingencyType.OCO:
            for client_order_id in order.linked_order_ids or []:
                oco_order = self.cache.order(client_order_id)
                assert oco_order is not None, "OCO order not found"

                if oco_order.is_closed_c():
                    continue

                if oco_order.is_active_local_c():
                    continue  # Order is not on the exchange yet

                self.cancel_order(oco_order)
        elif order.contingency_type == ContingencyType.OUO:
            for client_order_id in order.linked_order_ids or []:
                ouo_order = self.cache.order(client_order_id)
                assert ouo_order is not None, "OUO order not found"

                if ouo_order.is_active_local_c():
                    continue  # Order is not on the exchange yet

                if order.is_closed_c() and ouo_order.is_open_c():
                    self.cancel_order(ouo_order)
                elif order.leaves_qty._mem.raw != 0 and order.leaves_qty._mem.raw != ouo_order.leaves_qty._mem.raw:
                    self.update_order(
                        ouo_order,
                        order.leaves_qty,
                        price=ouo_order.price if ouo_order.has_price_c() else None,
                        trigger_price=ouo_order.trigger_price if ouo_order.has_trigger_price_c() else None,
                        update_contingencies=False,
                    )

        if position is None:
            return  # Fill completed

        # Check reduce only orders for position
        # Previously all reduce-only orders were force-synced to the net position size,
        # which incorrectly merged quantities across independent bracket orders.
        # Instead, prefer syncing each reduce-only child (TP/SL) to its own parent
        # entry order's filled quantity when available; fall back to position size
        # only for standalone reduce-only orders without a parent.
        cdef:
            Order ro_order
            Order parent_order
            Quantity target_qty
        for ro_order in self.cache.orders_for_position(position.id):
            if (
                self._use_reduce_only
                and ro_order.is_reduce_only
                and ro_order.is_open_c()
                and ro_order.is_passive_c()
            ):
                if position.quantity._mem.raw == 0:
                    self.cancel_order(ro_order)
                    continue

                # Determine target quantity for this reduce-only order
                parent_order = None

                if ro_order.parent_order_id is not None:
                    parent_order = self.cache.order(ro_order.parent_order_id)

                # Start with position quantity as default
                target_qty = position.quantity

                if parent_order is not None:
                    # Use the minimum of parent's filled quantity and position quantity
                    # This ensures bracket independence while respecting position reductions
                    if parent_order.filled_qty._mem.raw < position.quantity._mem.raw:
                        target_qty = parent_order.filled_qty
                    else:
                        target_qty = position.quantity

                # Safety clamp: never update total below what's already filled
                # This avoids invalid updates or modify-rejects
                if ro_order.filled_qty._mem.raw > target_qty._mem.raw:
                    target_qty = ro_order.filled_qty

                if ro_order.quantity._mem.raw != target_qty._mem.raw:
                    self.update_order(
                        ro_order,
                        target_qty,
                        price=ro_order.price if ro_order.has_price_c() else None,
                        trigger_price=ro_order.trigger_price if ro_order.has_trigger_price_c() else None,
                    )

# -- IDENTIFIER GENERATORS ------------------------------------------------------------------------

    cdef VenueOrderId _get_venue_order_id(self, Order order):
        # Check existing on order
        cdef VenueOrderId venue_order_id = order.venue_order_id
        if venue_order_id is not None:
            return venue_order_id

        # Check exiting in cache
        venue_order_id = self.cache.venue_order_id(order.client_order_id)
        if venue_order_id is not None:
            return venue_order_id

        venue_order_id = self._generate_venue_order_id()
        self.cache.add_venue_order_id(order.client_order_id, venue_order_id)

        return venue_order_id

    cdef PositionId _get_position_id(self, Order order, bint generate=True):
        cdef PositionId position_id
        if self.oms_type == OmsType.HEDGING:
            position_id = self.cache.position_id(order.client_order_id)

            if position_id is not None:
                return position_id

            if generate:
                # Generate a venue position ID
                return self._generate_venue_position_id()

        ####################################################################
        # NETTING OMS (position ID will be `{instrument_id}-{strategy_id}`)
        ####################################################################
        cdef list positions_open = self.cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=order.instrument_id,
        )
        if positions_open:
            return positions_open[0].id
        else:
            return None

    cdef PositionId _generate_venue_position_id(self):
        if not self._use_position_ids:
            return None

        self._position_count += 1

        if self._use_random_ids:
            return PositionId(str(uuid.uuid4()))
        else:
            return PositionId(f"{self.venue.to_str()}-{self.raw_id}-{self._position_count:03d}")

    cdef VenueOrderId _generate_venue_order_id(self):
        self._order_count += 1

        if self._use_random_ids:
            return VenueOrderId(str(uuid.uuid4()))
        else:
            return VenueOrderId(f"{self.venue.to_str()}-{self.raw_id}-{self._order_count:03d}")

    cdef TradeId _generate_trade_id(self):
        self._execution_count += 1
        return TradeId(self._generate_trade_id_str())

    cdef str _generate_trade_id_str(self):
        if self._use_random_ids:
            return str(uuid.uuid4())
        else:
            return f"{self.venue.to_str()}-{self.raw_id}-{self._execution_count:03d}"

# -- EVENT HANDLING -------------------------------------------------------------------------------

    cpdef void accept_order(self, Order order):
        if order.is_closed_c():
            return  # Temporary guard to prevent invalid processing

        # Check if order already accepted (being added back into the matching engine)
        if not order.status_c() == OrderStatus.ACCEPTED:
            self._generate_order_accepted(order, venue_order_id=self._get_venue_order_id(order))

            if (
                order.order_type == OrderType.TRAILING_STOP_MARKET
                or order.order_type == OrderType.TRAILING_STOP_LIMIT
            ):
                if order.trigger_price is None:
                    self._trail_stop_order(order)

        self._core.add_order(order)

    cpdef void expire_order(self, Order order):
        if self._support_contingent_orders and order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self._cancel_contingent_orders(order)

        self._generate_order_expired(order)

    cpdef void cancel_order(self, Order order, bint cancel_contingencies=True):
        if order.is_active_local_c():
            self._log.error(
                f"Cannot cancel an order with {order.status_string_c()} from the matching engine",
            )
            return

        self._core.delete_order(order)
        self._cached_filled_qty.pop(order.client_order_id, None)

        self._generate_order_canceled(order, venue_order_id=self._get_venue_order_id(order))

        if self._support_contingent_orders and order.contingency_type != ContingencyType.NO_CONTINGENCY and cancel_contingencies:
            self._cancel_contingent_orders(order)

    cpdef void update_order(
        self,
        Order order,
        Quantity qty,
        Price price = None,
        Price trigger_price = None,
        bint update_contingencies = True,
    ):
        if qty is None:
            qty = order.quantity

        if order.order_type == OrderType.LIMIT or order.order_type == OrderType.MARKET_TO_LIMIT:
            if price is None:
                price = order.price

            self._update_limit_order(order, qty, price)
        elif order.order_type == OrderType.STOP_MARKET:
            if trigger_price is None:
                trigger_price = order.trigger_price

            self._update_stop_market_order(order, qty, trigger_price)
        elif order.order_type == OrderType.STOP_LIMIT:
            if price is None:
                price = order.price

            if trigger_price is None:
                trigger_price = order.trigger_price

            self._update_stop_limit_order(order, qty, price, trigger_price)
        elif order.order_type == OrderType.MARKET_IF_TOUCHED:
            if trigger_price is None:
                trigger_price = order.trigger_price

            self._update_market_if_touched_order(order, qty, trigger_price)
        elif order.order_type == OrderType.LIMIT_IF_TOUCHED:
            if price is None:
                price = order.price

            if trigger_price is None:
                trigger_price = order.trigger_price

            self._update_limit_if_touched_order(order, qty, price, trigger_price)
        elif order.order_type == OrderType.TRAILING_STOP_MARKET:
            if trigger_price is None:
                trigger_price = order.trigger_price

            self._update_trailing_stop_market_order(order, qty, trigger_price)
        elif order.order_type == OrderType.TRAILING_STOP_LIMIT:
            if price is None:
                price = order.price

            if trigger_price is None:
                trigger_price = order.trigger_price

            self._update_trailing_stop_limit_order(order, qty, price, trigger_price)
        else:
            raise ValueError(
                f"invalid `OrderType` was {order.order_type}")  # pragma: no cover (design-time error)

        if self._support_contingent_orders and order.contingency_type != ContingencyType.NO_CONTINGENCY and update_contingencies:
            self._update_contingent_orders(order)

    cpdef void trigger_stop_order(self, Order order):
        # Always STOP_LIMIT or LIMIT_IF_TOUCHED orders
        cdef Price trigger_price = order.trigger_price
        cdef Price price = order.price

        if self._fill_model:
            if order.side == OrderSide.BUY and self._core.ask_raw == trigger_price._mem.raw and not self._fill_model.is_stop_filled():
                return  # Not triggered
            elif order.side == OrderSide.SELL and self._core.bid_raw == trigger_price._mem.raw and not self._fill_model.is_stop_filled():
                return  # Not triggered

        self._generate_order_triggered(order)

        # Check for immediate fill (which would fill passively as a maker)
        if order.side == OrderSide.BUY and trigger_price._mem.raw > price._mem.raw > self._core.ask_raw:
            order.liquidity_side = LiquiditySide.MAKER
            self.fill_limit_order(order)
            return
        elif order.side == OrderSide.SELL and trigger_price._mem.raw < price._mem.raw < self._core.bid_raw:
            order.liquidity_side = LiquiditySide.MAKER
            self.fill_limit_order(order)
            return

        if self._core.is_limit_matched(order.side, price):
            if order.is_post_only:
                # Would be liquidity taker
                self._core.delete_order(order)
                self._cached_filled_qty.pop(order.client_order_id, None)
                self._generate_order_rejected(
                    order,
                    f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"limit px of {order.price} would have been a TAKER: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                    True,  # due_post_only
                )
                return

            order.liquidity_side = LiquiditySide.TAKER
            self.fill_limit_order(order)

    cdef void _update_contingent_orders(self, Order order):
        self._log.debug(f"Updating OUO orders from {order.client_order_id}", LogColor.MAGENTA)
        cdef ClientOrderId client_order_id
        cdef Order ouo_order
        for client_order_id in order.linked_order_ids or []:
            ouo_order = self.cache.order(client_order_id)
            assert ouo_order is not None, "OUO order not found"

            if ouo_order.is_active_local_c():
                continue  # Order is not on the exchange yet

            if ouo_order.order_type == OrderType.MARKET or ouo_order.is_closed_c():
                continue

            if order.leaves_qty._mem.raw == 0:
                self.cancel_order(ouo_order)
            elif ouo_order.leaves_qty._mem.raw != order.leaves_qty._mem.raw:
                self.update_order(
                    ouo_order,
                    order.leaves_qty,
                    price=ouo_order.price if ouo_order.has_price_c() else None,
                    trigger_price=ouo_order.trigger_price if ouo_order.has_trigger_price_c() else None,
                    update_contingencies=False,
                )

    cdef void _cancel_contingent_orders(self, Order order):
        # Iterate all contingent orders and cancel if active
        cdef ClientOrderId client_order_id
        cdef Order contingent_order
        for client_order_id in order.linked_order_ids or []:
            contingent_order = self.cache.order(client_order_id)
            assert contingent_order is not None, "Contingency order not found"

            if contingent_order.is_active_local_c():
                continue  # Order is not on the exchange yet

            if not contingent_order.is_closed_c():
                self.cancel_order(contingent_order, cancel_contingencies=False)

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_order_rejected(self, Order order, str reason, bint due_post_only=False):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderRejected event = OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
            due_post_only=due_post_only,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_accepted(self, Order order, VenueOrderId venue_order_id):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderAccepted event = OrderAccepted(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_modify_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderModifyRejected event = OrderModifyRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_cancel_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderCancelRejected event = OrderCancelRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cpdef void _generate_order_updated(
        self,
        Order order,
        Quantity quantity,
        Price price,
        Price trigger_price,
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_canceled(self, Order order, VenueOrderId venue_order_id):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderCanceled event = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_triggered(self, Order order):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderTriggered event = OrderTriggered(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_expired(self, Order order):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderExpired event = OrderExpired(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.client_order_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_filled(
        self,
        Order order,
        VenueOrderId venue_order_id,
        PositionId venue_position_id,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderFilled event = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            trade_id=self._generate_trade_id(),
            position_id=venue_position_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=last_qty,
            last_px=last_px,
            currency=quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)


TimeRangeGenerator = Callable[[int, dict[str, Any]], Generator[int, bool, None]]
cdef dict[str, TimeRangeGenerator] TIME_RANGE_GENERATORS = {}

cpdef void register_time_range_generator(str name, function: TimeRangeGenerator):
    TIME_RANGE_GENERATORS[name] = function
