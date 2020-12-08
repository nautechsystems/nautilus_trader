# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import psutil
import pytz

from cpython.datetime cimport datetime

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.data_producer cimport BacktestDataProducer
from nautilus_trader.backtest.data_container cimport BacktestDataContainer
from nautilus_trader.backtest.data_client cimport BacktestDataClient
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.common.logging cimport TestLogger
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport as_utc_timestamp
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.functions cimport format_bytes
from nautilus_trader.core.functions cimport get_size_of
from nautilus_trader.core.functions cimport pad_string
from nautilus_trader.execution.database cimport BypassExecutionDatabase
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.redis.execution cimport RedisExecutionDatabase
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer
from nautilus_trader.serialization.serializers cimport MsgPackEventSerializer
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies inside a `Trader`
    on historical data.
    """

    def __init__(
            self,
            BacktestDataContainer data not None,
            TraderId trader_id=None,
            list strategies: [TradingStrategy]=None,
            BacktestConfig config=None,
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
        config : BacktestConfig, optional
            The configuration for the backtest engine (if None will be default).

        Raises
        ------
        TypeError
            If strategies contains a type other than TradingStrategy.

        """
        if trader_id is None:
            trader_id = TraderId("BACKTESTER", "000")
        if strategies is None:
            strategies = []
        Condition.list_type(strategies, TradingStrategy, "strategies")
        if config is None:
            config = BacktestConfig()

        self._config = config
        self._clock = LiveClock()
        self.created_time = self._clock.utc_now()

        self._test_clock = TestClock()
        self._test_clock.set_time(self._clock.utc_now())
        self._uuid_factory = UUIDFactory()

        self.analyzer = PerformanceAnalyzer()

        self._logger = TestLogger(
            clock=LiveClock(),
            name=trader_id.value,
            bypass_logging=False,
            level_console=LogLevel.INFO,
            level_file=LogLevel.INFO,
            level_store=LogLevel.WARNING,
            console_prints=True,
            log_thread=config.log_thread,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
        )

        self._log = LoggerAdapter(component_name=type(self).__name__, logger=self._logger)

        self._test_logger = TestLogger(
            clock=self._test_clock,
            name=trader_id.value,
            bypass_logging=config.bypass_logging,
            level_console=config.level_console,
            level_file=config.level_file,
            level_store=config.level_store,
            console_prints=config.console_prints,
            log_thread=config.log_thread,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
        )

        nautilus_header(self._log)
        self._log.info("=================================================================")
        self._log.info("Building engine...")

        # Setup execution database
        if config.exec_db_type == "in-memory":
            exec_db = BypassExecutionDatabase(
                trader_id=trader_id,
                logger=self._logger)
        elif config.exec_db_type == "redis":
            exec_db = RedisExecutionDatabase(
                trader_id=trader_id,
                logger=self._test_logger,
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                config={"host": "localhost", "port": 6379},
            )
        else:
            raise ValueError(f"The exec_db_type in the backtest configuration is unrecognized "
                             f"(can be either \"in-memory\" or \"redis\")")

        if self._config.exec_db_flush:
            exec_db.flush()

        # Setup execution cache
        self._test_clock.set_time(self._clock.utc_now())  # For logging consistency

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
            engine=self._data_engine,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        # Create data client per venue
        for venue in data.venues:
            instruments = {}
            for instrument in data.instruments.values():
                if instrument.symbol.venue == venue:
                    instruments[instrument.symbol] = instrument

            data_client = BacktestDataClient(
                instruments=instruments,
                venue=venue,
                engine=self._data_engine,
                clock=self._clock,
                logger=self._logger,
            )

            self._data_engine.register_client(data_client)

        self._exec_engine = ExecutionEngine(
            database=exec_db,
            portfolio=self.portfolio,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._exec_engine.load_cache()

        self.trader = Trader(
            trader_id=trader_id,
            strategies=strategies,
            data_engine=self._data_engine,
            exec_engine=self._exec_engine,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._exchanges = {}

        self._test_clock.set_time(self._clock.utc_now())  # For logging consistency

        self.iteration = 0

        self.time_to_initialize = self._clock.delta(self.created_time)
        self._log.info(f"Initialized in {self.time_to_initialize}.")
        self._backtest_memory()

    cpdef void add_exchange(
            self,
            Venue venue,
            OMSType oms_type,
            bint generate_position_ids=True,
            FillModel fill_model=None,
            list modules=None,
    ) except *:
        """
        Add a `SimulatedExchange` with the given parameters to the backtest engine.

        Parameters
        ----------
        venue : Venue
            The venue for the exchange.
        oms_type : OMSType
            The order management system type for the exchange.
        generate_position_ids : bool
            If the exchange should generate position identifiers. If oms_type
            is HEDGING then will always generate position identifiers.
        fill_model : FillModel
            The fill model for the exchange (if None then no probabilistic fills).
        modules : list[SimulationModule
            The simulation modules to load into the exchange.

        """
        Condition.not_none(venue, "venue")
        Condition.not_in(venue, self._exchanges, "venue", "self._exchanges")
        Condition.not_equal(oms_type, OMSType.UNDEFINED, "oms_type", "UNDEFINED")
        Condition.type_or_none(fill_model, FillModel, "fill_model")
        if fill_model is None:
            fill_model = FillModel()
        if modules is None:
            modules = []

        account_id = AccountId(venue.value, "000", AccountType.SIMULATED)

        exchange = SimulatedExchange(
            venue=venue,
            oms_type=oms_type,
            generate_position_ids=True,
            exec_cache=self._exec_engine.cache,
            instruments={},
            config=self._config,
            fill_model=fill_model,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        for module in modules:
            exchange.load_module(module)

        self._exchanges[venue] = exchange

        exec_client = BacktestExecClient(
            exchange=exchange,
            account_id=account_id,
            engine=self._exec_engine,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        exchange.register_client(exec_client)
        self._exec_engine.register_client(exec_client)

        for instrument in self._data_engine.cache.instruments():
            if instrument.symbol.venue == venue:
                exchange.add_instrument(instrument)

    cpdef void print_log_store(self) except *:
        """
        Print the contents of the test loggers store to the console.
        """
        self._log.info("")
        self._log.info("=================================================================")
        self._log.info(" LOG STORE")
        self._log.info("=================================================================")

        cdef list log_store = self._test_logger.get_log_store()
        cdef str message
        if not log_store:
            self._log.info("No log messages were stored.")
        else:
            for message in self._test_logger.get_log_store():
                print(message)

    cpdef void reset(self) except *:
        """
        Reset the backtest engine.

        All stateful values are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        if self._data_engine.state_c() == ComponentState.RUNNING:
            self._data_engine.stop()
        self._data_engine.reset()

        if self._exec_engine.state_c() == ComponentState.RUNNING:
            self._exec_engine.stop()
        if self._config.exec_db_flush:
            self._exec_engine.flush_db()
        self._exec_engine.reset()

        self.trader.reset()

        for exchange in self._exchanges.values():
            exchange.reset()

        self._logger.clear_log_store()
        self._test_logger.clear_log_store()

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

        self._exchanges[venue].change_fill_model(model)

    cpdef void run(
            self,
            datetime start=None,
            datetime stop=None,
            list strategies=None,
            bint print_log_store=True
    ) except *:
        """
        Run a backtest from the start datetime to the stop datetime.

        Parameters
        ----------
        start : datetime, optional
            The start (UTC) for the backtest run. If None engine will run from the start of the data.
        stop : datetime, optional
            The stop (UTC) for the backtest run. If None engine will run to the end of the data.
        strategies : list, optional
            The strategies for the backtest run (if None will use previous).
        print_log_store : bool
            If the log store should be printed at the end of the run.

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
        Condition.true(start >= self._data_producer.min_timestamp, "start >= data_client.min_timestamp")
        Condition.true(start <= self._data_producer.max_timestamp, "stop <= data_client.max_timestamp")
        Condition.true(start < stop, "start < stop")
        if strategies:
            Condition.not_empty(strategies, "strategies")
            Condition.list_type(strategies, TradingStrategy, "strategies")

        cdef datetime run_started = self._clock.utc_now()

        # Setup logging
        self._test_logger.clear_log_store()
        if self._config.log_to_file:
            backtest_log_name = f"{self._logger.name}-{format_iso8601(run_started)}"
            self._logger.change_log_file_name(backtest_log_name)
            self._test_logger.change_log_file_name(backtest_log_name)

        self._backtest_header(run_started, start, stop)
        self._log.info(f"Setting up backtest...")

        # Reset engine to fresh state (in case already run)
        self.reset()

        # Setup clocks
        self._test_clock.set_time(start)

        # Setup data
        self._data_producer.setup(start, stop)

        # Setup new strategies
        if strategies is not None:
            self.trader.initialize_strategies(strategies)

        # Run the backtest
        self._log.info(f"Running backtest...")

        for strategy in self.trader.strategies_c():
            strategy.clock.set_time(start)

        # TODO: Temporary fix to initialize account
        for exchange in self._exchanges.values():
            exchange.adjust_account(Money(0, exchange.account_currency))

        # Start main components
        self._data_engine.start()
        self._exec_engine.start()
        self.trader.start()

        cdef Tick tick
        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        while self._data_producer.has_tick_data:
            tick = self._data_producer.next_tick()
            self._advance_time(tick.timestamp)
            self._exchanges[tick.symbol.venue].process_tick(tick)
            self._data_engine.process(tick)
            self.iteration += 1
        # ---------------------------------------------------------------------#

        self.trader.stop()

        self._backtest_footer(run_started, self._clock.utc_now(), start, stop)
        if print_log_store:
            self.print_log_store()

    cdef void _advance_time(self, datetime timestamp) except *:
        cdef TradingStrategy strategy
        cdef TimeEventHandler event_handler
        cdef list time_events = []  # type: list[TimeEventHandler]
        for strategy in self.trader.strategies_c():
            time_events += strategy.clock.advance_time(timestamp)
        for event_handler in sorted(time_events):
            self._test_clock.set_time(event_handler.event.timestamp)
            event_handler.handle()
        self._test_clock.set_time(timestamp)

    cdef void _backtest_memory(self) except *:
        self._log.info("=================================================================")
        self._log.info(" MEMORY USAGE")
        self._log.info("=================================================================")
        ram_total_mb = round(psutil.virtual_memory()[0] / 1000000)
        ram_used__mb = round(psutil.virtual_memory()[3] / 1000000)
        ram_avail_mb = round(psutil.virtual_memory()[1] / 1000000)
        ram_avail_pc = round(100 - psutil.virtual_memory()[2], 2)
        self._log.info(f"RAM-Total: {ram_total_mb:,} MB")
        self._log.info(f"RAM-Used:  {ram_used__mb:,} MB ({round(100.0 - ram_avail_pc, 2)}%)")
        self._log.info(f"RAM-Avail: {ram_avail_mb:,} MB ({ram_avail_pc}%)")
        self._log.info(f"Data size: {format_bytes(get_size_of(self._data_engine))}")

    cdef void _backtest_header(
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
        if self._config.frozen_accounts:
            self._log.warning(f"ACCOUNTS FROZEN")
        else:
            self._log.info(f"Account balance (starting): {self._config.starting_capital.to_str()}")
        self._log.info("=================================================================")

    cdef void _backtest_footer(
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
            self._log.info(exchange.account.id.value)
            self._log.info("=================================================================")
            if self._config.frozen_accounts:
                self._log.warning(f"ACCOUNT(S) FROZEN")
            else:
                account_balance_starting = self._config.starting_capital.to_str()
                account_starting_length = len(account_balance_starting)
                account_balance_ending = pad_string(exchange.account_balance.to_str(), account_starting_length)
                commissions_total = pad_string(exchange.total_commissions.to_str(), account_starting_length)
                self._log.info(f"Account balance (starting): {account_balance_starting}")
                self._log.info(f"Account balance (ending):   {account_balance_ending}")
                self._log.info(f"Commissions (total):        {commissions_total}")
            # Log output diagnostics for all simulation modules

            for module in exchange.modules:
                module.log_diagnostics(self._log)

            self._log.info("=================================================================")
            self._log.info(" PERFORMANCE STATISTICS")
            self._log.info("=================================================================")

            # Find all positions for exchange venue
            positions = []
            for position in self._exec_engine.cache.positions():
                if position.symbol.venue == exchange.venue:
                    positions.append(position)
            self.analyzer.calculate_statistics(exchange.account, positions)

            for statistic in self.analyzer.get_performance_stats_formatted(exchange.account.currency):
                self._log.info(statistic)
