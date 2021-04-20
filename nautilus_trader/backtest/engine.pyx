# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

# cython: always_allow_keywords=False

import pytz

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.backtest.data_client cimport BacktestDataClient
from nautilus_trader.backtest.data_client cimport BacktestMarketDataClient
from nautilus_trader.backtest.data_container cimport BacktestDataContainer
from nautilus_trader.backtest.data_producer cimport BacktestDataProducer
from nautilus_trader.backtest.data_producer cimport CachedProducer
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport log_memory
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_timestamp
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.functions cimport format_bytes

from nautilus_trader.core.functions import get_size_of  # Not cimport

from nautilus_trader.core.functions cimport pad_string
from nautilus_trader.execution.database cimport BypassExecutionDatabase
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.orderbook.book cimport OrderBookData
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.redis.execution cimport RedisExecutionDatabase
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer
from nautilus_trader.serialization.serializers cimport MsgPackEventSerializer
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies over historical
    data.
    """

    def __init__(
        self,
        BacktestDataContainer data not None,
        TraderId trader_id=None,
        list strategies=None,
        int tick_capacity=1000,
        int bar_capacity=1000,
        bint use_data_cache=False,
        str exec_db_type not None="in-memory",
        bint exec_db_flush=True,
        dict risk_config=None,
        bint bypass_logging=False,
        int level_stdout=LogLevel.INFO,
        bint calculate_data_size=True
    ):
        """
        Initialize a new instance of the `BacktestEngine` class.

        Parameters
        ----------
        data : BacktestDataContainer
            The data for the backtest engine.
        trader_id : TraderId, optional
            The trader identifier.
        strategies : list[TradingStrategy], optional
            The initial strategies for the backtest engine.
        tick_capacity : int, optional
            The length for the data engines internal ticks deque (> 0).
        bar_capacity : int, optional
            The length for the data engines internal bars deque (> 0).
        use_data_cache : bool, optional
            If use cache for DataProducer (increased performance with repeated backtests on same data).
        exec_db_type : str, optional
            The type for the execution cache (can be the default 'in-memory' or redis).
        exec_db_flush : bool, optional
            If the execution cache should be flushed on each run.
        risk_config : dict[str, object]
            The configuration for the risk engine.
        bypass_logging : bool, optional
            If logging should be bypassed.
        level_stdout : int, optional
            The minimum log level for logging messages to stdout.

        Raises
        ------
        ValueError
            If tick_capacity is not positive (> 0).
        ValueError
            If bar_capacity is not positive (> 0).
        TypeError
            If strategies contains a type other than TradingStrategy.

        """
        Condition.positive_int(tick_capacity, "tick_capacity")
        Condition.positive_int(bar_capacity, "bar_capacity")
        Condition.valid_string(exec_db_type, "exec_db_type")
        if trader_id is None:
            trader_id = TraderId("BACKTESTER", "000")
        if strategies is None:
            strategies = []
        if risk_config is None:
            risk_config = {}
        Condition.list_type(strategies, TradingStrategy, "strategies")

        self._clock = LiveClock()
        self.created_time = self._clock.utc_now()

        self._test_clock = TestClock()
        self._test_clock.set_time(self._clock.timestamp_ns())
        self._uuid_factory = UUIDFactory()
        self.system_id = self._uuid_factory.generate()

        self._logger = Logger(
            clock=LiveClock(),
            trader_id=trader_id,
            system_id=self.system_id,
        )

        self._log = LoggerAdapter(
            component=type(self).__name__,
            logger=self._logger,
        )

        self._test_logger = Logger(
            clock=self._test_clock,
            trader_id=trader_id,
            system_id=self.system_id,
            level_stdout=level_stdout,
            bypass_logging=bypass_logging,
        )

        nautilus_header(self._log)
        self._log.info("=================================================================")
        self._log.info("Building engine...")

        # Setup execution database
        self._exec_db_flush = exec_db_flush

        if exec_db_type == "in-memory":
            exec_db = BypassExecutionDatabase(
                trader_id=trader_id,
                logger=self._logger)
        elif exec_db_type == "redis":
            exec_db = RedisExecutionDatabase(
                trader_id=trader_id,
                logger=self._test_logger,
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                config={"host": "localhost", "port": 6379},
            )
        else:
            raise ValueError(f"The exec_db_type in the backtest configuration is unrecognized, "
                             f"can be either \"in-memory\" or \"redis\"")

        if self._exec_db_flush:
            exec_db.flush()

        self._test_clock.set_time(self._clock.timestamp_ns())  # For logging consistency

        self.analyzer = PerformanceAnalyzer()

        self.portfolio = Portfolio(
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self._test_clock,
            logger=self._test_logger,
            config={'use_previous_close': False},  # Ensures bars match historical data
        )

        self.portfolio.register_cache(self._data_engine.cache)

        self._data_producer = BacktestDataProducer(
            data=data,
            logger=self._test_logger,
        )

        # Prepare instruments
        for instrument in self._data_producer.instruments():
            self._data_engine.process(instrument)

        if use_data_cache:
            self._data_producer = CachedProducer(self._data_producer)

        # Create data clients
        for client_id, client_type in data.clients.items():
            if client_type == BacktestDataClient:
                data_client = BacktestDataClient(
                    client_id=client_id,
                    engine=self._data_engine,
                    clock=self._test_clock,
                    logger=self._test_logger,
                )
            elif client_type == BacktestMarketDataClient:
                instruments = []
                for instrument in data.instruments.values():
                    if instrument.id.venue.client_id == client_id:
                        instruments.append(instrument)

                data_client = BacktestMarketDataClient(
                    instruments=instruments,
                    client_id=client_id,
                    engine=self._data_engine,
                    clock=self._test_clock,
                    logger=self._test_logger,
                )
            else:
                raise RuntimeError(f"DataClient type invalid, was {client_type}")

            self._data_engine.register_client(data_client)

        self._exec_engine = ExecutionEngine(
            database=exec_db,
            portfolio=self.portfolio,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._risk_engine = RiskEngine(
            exec_engine=self._exec_engine,
            portfolio=self.portfolio,
            clock=self._test_clock,
            logger=self._test_logger,
            config=risk_config,
        )

        self._exec_engine.load_cache()
        self._exec_engine.register_risk_engine(self._risk_engine)

        self.trader = Trader(
            trader_id=trader_id,
            strategies=strategies,
            portfolio=self.portfolio,
            data_engine=self._data_engine,
            exec_engine=self._exec_engine,
            risk_engine=self._risk_engine,
            clock=self._test_clock,
            logger=self._test_logger,
            warn_no_strategies=False,
        )

        self._exchanges = {}

        self._test_clock.set_time(self._clock.timestamp_ns())  # For logging consistency

        self.iteration = 0

        self.time_to_initialize = self._clock.delta(self.created_time)
        self._log.info(f"Initialized in {self.time_to_initialize.total_seconds():.3f}s.")
        log_memory(self._log)
        if calculate_data_size:
            self._log.info(f"Data size: {format_bytes(get_size_of(self._data_engine))}")

    cpdef ExecutionEngine get_exec_engine(self):
        """
        Return the execution engine for the backtest engine (used for testing).

        Returns
        -------
        ExecutionEngine

        """
        return self._exec_engine

    cpdef void add_exchange(
        self,
        Venue venue,
        OMSType oms_type,
        list starting_balances,
        bint is_frozen_account=False,
        list modules=None,
        FillModel fill_model=None,
    ) except *:
        """
        Add a `SimulatedExchange` with the given parameters to the backtest engine.

        Parameters
        ----------
        venue : Venue
            The venue for the exchange.
        oms_type : OMSType (Enum)
            The order management system type for the exchange. If HEDGING and
            no position_id for an order then will generate a new position_id.
        starting_balances : list[Money]
            The starting account balances (specify one for a single asset account).
        is_frozen_account : bool, optional
            If the account for this exchange is frozen (balances will not change).
        modules : list[SimulationModule, optional
            The simulation modules to load into the exchange.
        fill_model : FillModel, optional
            The fill model for the exchange (if None then no probabilistic fills).

        Raises
        ------
        ValueError
            If an exchange of venue is already registered with the engine.

        """
        if modules is None:
            modules = []
        if fill_model is None:
            fill_model = FillModel()
        Condition.not_none(venue, "venue")
        Condition.not_in(venue, self._exchanges, "venue", "self._exchanges")
        Condition.not_none(starting_balances, "starting_balances")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules")
        Condition.type_or_none(fill_model, FillModel, "fill_model")

        account_id = AccountId(venue.value, "001")

        # Gather instruments for exchange
        instruments = []
        for instrument in self._data_engine.cache.instruments():
            if instrument.id.venue == venue:
                instruments.append(instrument)

        # Create exchange
        exchange = SimulatedExchange(
            venue=venue,
            oms_type=oms_type,
            is_frozen_account=is_frozen_account,
            starting_balances=starting_balances,
            instruments=instruments,
            modules=modules,
            exec_cache=self._exec_engine.cache,
            fill_model=fill_model,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._exchanges[venue] = exchange

        # Create execution client for exchange
        exec_client = BacktestExecClient(
            exchange=exchange,
            account_id=account_id,
            engine=self._exec_engine,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        exchange.register_client(exec_client)
        self._exec_engine.register_client(exec_client)

    cpdef void reset(self) except *:
        """
        Reset the backtest engine.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        # Reset DataEngine
        if self._data_engine.state_c() == ComponentState.RUNNING:
            self._data_engine.stop()
        self._data_engine.reset()

        # Reset ExecEngine
        if self._exec_engine.state_c() == ComponentState.RUNNING:
            self._exec_engine.stop()
        if self._exec_db_flush:
            self._exec_engine.flush_db()
        self._exec_engine.reset()

        # Reset RiskEngine
        if self._risk_engine.state_c() == ComponentState.RUNNING:
            self._risk_engine.stop()
        self._risk_engine.reset()

        self.trader.reset()

        for exchange in self._exchanges.values():
            exchange.reset()

        self.iteration = 0

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.
        """
        self.trader.dispose()

        if self._data_engine.state_c() == ComponentState.RUNNING:
            self._data_engine.stop()
        if self._exec_engine.state_c() == ComponentState.RUNNING:
            self._exec_engine.stop()

        self._data_engine.dispose()
        self._exec_engine.dispose()
        self._risk_engine.dispose()

    cpdef void change_fill_model(self, Venue venue, FillModel model) except *:
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
        Condition.is_in(venue, self._exchanges, "venue", "self._exchanges")

        self._exchanges[venue].set_fill_model(model)

    cpdef void run(
        self,
        datetime start=None,
        datetime stop=None,
        list strategies=None,
    ) except *:
        """
        Run a backtest from the start datetime to the stop datetime.

        Parameters
        ----------
        start : datetime, optional
            The start datetime (UTC) for the backtest run. If None engine will
            run from the start of the data.
        stop : datetime, optional
            The stop datetime (UTC) for the backtest run. If None engine will
            run to the end of the data.
        strategies : list, optional
            The strategies for the backtest run (if None will use previous).

        Raises
        ------
        ValueError
            If the stop is >= the start datetime.

        """
        # Setup start datetime
        if start is None:
            start = self._data_producer.min_timestamp
        else:
            start = max(as_utc_timestamp(start), self._data_producer.min_timestamp)

        # Setup stop datetime
        if stop is None:
            stop = self._data_producer.max_timestamp
        else:
            stop = min(as_utc_timestamp(stop), self._data_producer.max_timestamp)

        Condition.equal(start.tz, pytz.utc, "start.tz", "UTC")
        Condition.equal(stop.tz, pytz.utc, "stop.tz", "UTC")
        Condition.true(start >= self._data_producer.min_timestamp, "start was < data_client.min_timestamp")
        Condition.true(start <= self._data_producer.max_timestamp, "stop was > data_client.max_timestamp")
        Condition.true(start < stop, "start was >= stop")
        if strategies:
            Condition.not_empty(strategies, "strategies")
            Condition.list_type(strategies, TradingStrategy, "strategies")

        cdef datetime run_started = self._clock.utc_now()

        self._log_header(run_started, start, stop)
        self._log.info(f"Setting up backtest...")

        # Reset engine to fresh state (in case already run)
        self.reset()

        cdef int64_t start_ns = dt_to_unix_nanos(start)
        cdef int64_t stop_ns = dt_to_unix_nanos(stop)

        # Setup clocks
        self._test_clock.set_time(start_ns)

        # Setup data
        self._data_producer.setup(start_ns=start_ns, stop_ns=stop_ns)

        # Prepare instruments
        for instrument in self._data_producer.instruments():
            self._data_engine.process(instrument)

        # Setup new strategies
        if strategies is not None:
            self.trader.initialize_strategies(strategies, warn_no_strategies=False)

        # Run the backtest
        self._log.info(f"Running backtest...")

        for strategy in self.trader.strategies_c():
            strategy.clock.set_time(start_ns)

        for exchange in self._exchanges.values():
            exchange.initialize_account()

        # Start main components
        self._data_engine.start()
        self._exec_engine.start()
        self.trader.start()

        cdef Data data
        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        while self._data_producer.has_data:
            data = self._data_producer.next()
            self._advance_time(data.timestamp_ns)
            if isinstance(data, OrderBookData):
                self._exchanges[data.instrument_id.venue].process_order_book(data)
            elif isinstance(data, Tick):
                self._exchanges[data.instrument_id.venue].process_tick(data)
            self._data_engine.process(data)
            self._process_modules(data.timestamp_ns)
            self.iteration += 1
        # ---------------------------------------------------------------------#

        self.trader.stop()

        self._log_footer(run_started, self._clock.utc_now(), start, stop)

    cdef inline void _advance_time(self, int64_t now_ns) except *:
        cdef TradingStrategy strategy
        cdef TimeEventHandler event_handler
        cdef list time_events = []  # type: list[TimeEventHandler]
        for strategy in self.trader.strategies_c():
            time_events += strategy.clock.advance_time(now_ns)
        for event_handler in sorted(time_events):
            self._test_clock.set_time(event_handler.event.event_timestamp_ns)
            event_handler.handle()
        self._test_clock.set_time(now_ns)

    cdef inline void _process_modules(self, int64_t now_ns) except *:
        cdef SimulatedExchange exchange
        for exchange in self._exchanges.values():
            exchange.process_modules(now_ns)

    cdef inline void _log_header(
        self,
        datetime run_started,
        datetime start,
        datetime stop,
    ) except *:
        self._log.info("=================================================================")
        self._log.info(" BACKTEST RUN")
        self._log.info("=================================================================")
        self._log.info(f"Run started:    {format_iso8601(run_started)}")
        self._log.info(f"Backtest start: {format_iso8601(start)}")
        self._log.info(f"Backtest stop:  {format_iso8601(stop)}")
        for resolution in self._data_producer.execution_resolutions:
            self._log.info(f"Execution resolution: {resolution}")

        for exchange in self._exchanges.values():
            self._log.info("=================================================================")
            self._log.info(exchange.exec_client.account_id.value)
            self._log.info("=================================================================")
            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                balances = ', '.join([b.to_str() for b in exchange.starting_balances])
                self._log.info(f"Account balances (starting): {balances}")

    cdef inline void _log_footer(
        self,
        datetime run_started,
        datetime run_finished,
        datetime start,
        datetime stop,
    ) except *:
        self._log.info("=================================================================")
        self._log.info(" BACKTEST DIAGNOSTICS")
        self._log.info("=================================================================")
        self._log.info(f"Run started:    {format_iso8601(run_started)}")
        self._log.info(f"Run finished:   {format_iso8601(run_finished)}")
        self._log.info(f"Backtest start: {format_iso8601(start)}")
        self._log.info(f"Backtest stop:  {format_iso8601(stop)}")
        self._log.info(f"Elapsed time:   {run_finished - run_started}")
        for resolution in self._data_producer.execution_resolutions:
            self._log.info(f"Execution resolution: {resolution}")
        self._log.info(f"Iterations: {self.iteration:,}")
        self._log.info(f"Total events: {self._exec_engine.event_count:,}")
        self._log.info(f"Total orders: {self._exec_engine.cache.orders_total_count():,}")
        self._log.info(f"Total positions: {self._exec_engine.cache.positions_total_count():,}")

        for exchange in self._exchanges.values():
            self._log.info("=================================================================")
            self._log.info(f" {exchange.exec_client.account_id.value}")
            self._log.info("=================================================================")
            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                account_balances_starting = ', '.join([b.to_str() for b in exchange.starting_balances])
                account_balances_ending = ', '.join([b.to_str() for b in exchange.account_balances.values()])
                account_commissions = ', '.join([b.to_str() for b in exchange.total_commissions.values()])
                account_starting_length = len(account_balances_starting)
                account_balances_ending = pad_string(account_balances_ending, account_starting_length)
                account_commissions = pad_string(account_commissions, account_starting_length)
                self._log.info(f"Account balances (starting): {account_balances_starting}")
                self._log.info(f"Account balances (ending):   {account_balances_ending}")
                self._log.info(f"Commissions (total):         {account_commissions}")

            # Log output diagnostics for all simulation modules
            for module in exchange.modules:
                module.log_diagnostics(self._log)

            self._log.info("=================================================================")
            self._log.info(" PERFORMANCE STATISTICS")
            self._log.info("=================================================================")

            # Find all positions for exchange venue
            positions = []
            for position in self._exec_engine.cache.positions():
                if position.instrument_id.venue == exchange.id:
                    positions.append(position)

            # Calculate statistics
            account = self._exec_engine.cache.account_for_venue(exchange.id)
            self.analyzer.calculate_statistics(account, positions)

            # Present PnL performance stats per asset
            for currency in account.currencies():
                self._log.info(f" {str(currency)}")
                self._log.info("-----------------------------------------------------------------")
                for statistic in self.analyzer.get_performance_stats_pnls_formatted(currency):
                    self._log.info(statistic)
                self._log.info("-----------------------------------------------------------------")

            self._log.info(" Returns")
            self._log.info("-----------------------------------------------------------------")
            for statistic in self.analyzer.get_performance_stats_returns_formatted():
                self._log.info(statistic)
