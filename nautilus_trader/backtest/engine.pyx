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
from nautilus_trader.backtest.data cimport BacktestDataClient
from nautilus_trader.backtest.data cimport BacktestDataContainer
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.logging cimport TestLogger
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.common.uuid cimport TestUUIDFactory
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
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.redis.execution cimport RedisExecutionDatabase
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer
from nautilus_trader.serialization.serializers cimport MsgPackEventSerializer
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies inside a Trader
    on historical data.
    """

    def __init__(
            self,
            BacktestDataContainer data not None,
            list strategies not None: [TradingStrategy],
            Venue venue not None,
            OMSType oms_type,
            generate_position_ids,
            BacktestConfig config=None,
            FillModel fill_model=None,
    ):
        """
        Initialize a new instance of the `BacktestEngine` class.

        Parameters
        ----------
        data : BacktestDataContainer
            The data for the backtest engine.
        strategies : list[TradingStrategy]
            The initial strategies for the backtest engine.
        config : BacktestConfig
            The optional configuration for the backtest engine (if None will be default).
        fill_model : FillModel
            The optional initial fill model for the backtest engine,
            (if None then no probabilistic fills).

        Raises
        ------
        TypeError
            If strategies contains a type other than TradingStrategy.

        """
        if config is None:
            config = BacktestConfig()
        if fill_model is None:
            fill_model = FillModel()
        Condition.list_type(strategies, TradingStrategy, "strategies")

        self.trader_id = TraderId("BACKTESTER", "000")
        self.account_id = AccountId(venue.value, "000", AccountType.SIMULATED)
        self.config = config
        self.clock = LiveClock()
        self.created_time = self.clock.utc_now()

        self.test_clock = TestClock()
        self.test_clock.set_time(self.clock.utc_now())
        self.uuid_factory = TestUUIDFactory()

        self.analyzer = PerformanceAnalyzer()

        self.logger = TestLogger(
            clock=LiveClock(),
            name=self.trader_id.value,
            bypass_logging=False,
            level_console=LogLevel.INFO,
            level_file=LogLevel.INFO,
            level_store=LogLevel.WARNING,
            console_prints=True,
            log_thread=config.log_thread,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
        )

        self.log = LoggerAdapter(component_name=type(self).__name__, logger=self.logger)

        self.test_logger = TestLogger(
            clock=self.test_clock,
            name=self.trader_id.value,
            bypass_logging=config.bypass_logging,
            level_console=config.level_console,
            level_file=config.level_file,
            level_store=config.level_store,
            console_prints=config.console_prints,
            log_thread=config.log_thread,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
        )

        nautilus_header(self.log)
        self.log.info("=================================================================")
        self.log.info("Building engine...")

        # Setup execution database
        if config.exec_db_type == "in-memory":
            exec_db = BypassExecutionDatabase(
                trader_id=self.trader_id,
                logger=self.logger)
        elif config.exec_db_type == "redis":
            exec_db = RedisExecutionDatabase(
                trader_id=self.trader_id,
                logger=self.test_logger,
                host="localhost",
                port=6379,
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
            )
        else:
            raise ValueError(f"The exec_db_type in the backtest configuration is unrecognized "
                             f"(can be either \"in-memory\" or \"redis\")")

        if self.config.exec_db_flush:
            exec_db.flush()

        # Setup execution cache
        self.test_clock.set_time(self.clock.utc_now())  # For logging consistency

        self.portfolio = Portfolio(
            clock=self.test_clock,
            uuid_factory=self.uuid_factory,
            logger=self.test_logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.test_clock,
            uuid_factory=self.uuid_factory,
            logger=self.test_logger,
            config={'use_previous_close': False},
        )

        self.exec_engine = ExecutionEngine(
            database=exec_db,
            portfolio=self.portfolio,
            clock=self.test_clock,
            uuid_factory=self.uuid_factory,
            logger=self.test_logger,
        )

        self.exec_engine.load_cache()

        self.exchange = SimulatedExchange(
            venue=venue,
            oms_type=oms_type,
            generate_position_ids=True,
            exec_cache=self.exec_engine.cache,
            instruments=data.instruments,
            config=config,
            fill_model=fill_model,
            clock=self.test_clock,
            uuid_factory=self.uuid_factory,
            logger=self.test_logger,
        )

        self.data_client = BacktestDataClient(
            data=data,
            venue=venue,
            engine=self.data_engine,
            clock=self.test_clock,
            uuid_factory=self.uuid_factory,
            logger=self.test_logger,
        )

        self.exec_client = BacktestExecClient(
            market=self.exchange,
            account_id=self.account_id,
            engine=self.exec_engine,
            logger=self.test_logger,
        )

        self.data_engine.register_client(self.data_client)
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.trader = Trader(
            trader_id=self.trader_id,
            strategies=strategies,
            data_engine=self.data_engine,
            exec_engine=self.exec_engine,
            clock=self.test_clock,
            uuid_factory=self.uuid_factory,
            logger=self.test_logger,
        )

        self.test_clock.set_time(self.clock.utc_now())  # For logging consistency

        self.iteration = 0

        self.time_to_initialize = self.clock.delta(self.created_time)
        self.log.info(f"Initialized in {self.time_to_initialize}.")
        self._backtest_memory()

    cpdef void run(
            self,
            datetime start=None,
            datetime stop=None,
            FillModel fill_model=None,
            list strategies=None,
            bint print_log_store=True
    ) except *:
        """
        Run a backtest from the start datetime to the stop datetime.

        If start datetime is None engine will run from the start of the data.
        If stop datetime is None engine will run to the end of the data.

        Parameters
        ----------
        start : datetime
            The optional start datetime (UTC) for the backtest run.
        stop : datetime
            The optional stop datetime (UTC) for the backtest run.
        fill_model : FillModel
            The optional fill model change for the backtest run (if None will use previous).
        strategies : list
            The optional strategies change for the backtest run (if None will use previous).
        print_log_store : bool
            If the log store should be printed at the end of the run.

        Raises
        ------
        ValueError
            If the stop is >= the start datetime.

        """
        # Setup start datetime
        if start is None:
            start = self.data_client.min_timestamp
        else:
            start = max(as_utc_timestamp(start), self.data_client.min_timestamp)

        # Setup stop datetime
        if stop is None:
            stop = self.data_client.max_timestamp
        else:
            stop = min(as_utc_timestamp(stop), self.data_client.max_timestamp)

        Condition.equal(start.tz, pytz.utc, "start.tz", "UTC")
        Condition.equal(stop.tz, pytz.utc, "stop.tz", "UTC")
        Condition.true(start >= self.data_client.min_timestamp, "start >= data_client.min_timestamp")
        Condition.true(start <= self.data_client.max_timestamp, "stop <= data_client.max_timestamp")
        Condition.true(start < stop, "start < stop")
        Condition.type_or_none(fill_model, FillModel, "fill_model")
        if strategies:
            Condition.not_empty(strategies, "strategies")
            Condition.list_type(strategies, TradingStrategy, "strategies")

        cdef datetime run_started = self.clock.utc_now()

        # Setup logging
        self.test_logger.clear_log_store()
        if self.config.log_to_file:
            backtest_log_name = f"{self.logger.name}-{format_iso8601(run_started)}"
            self.logger.change_log_file_name(backtest_log_name)
            self.test_logger.change_log_file_name(backtest_log_name)

        self._backtest_header(run_started, start, stop)
        self.log.info(f"Setting up backtest...")

        # Reset engine to fresh state (in case already run)
        self.reset()

        # Setup clocks
        self.test_clock.set_time(start)

        # Setup data
        self.data_client.setup(start, stop)

        # Setup new fill model
        if fill_model is not None:
            self.exchange.change_fill_model(fill_model)

        # Setup new strategies
        if strategies is not None:
            self.trader.initialize_strategies(strategies)

        # Run the backtest
        self.log.info(f"Running backtest...")

        for strategy in self.trader.strategies():
            strategy.clock.set_time(start)

        self.trader.start()

        cdef QuoteTick tick
        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        while self.data_client.has_data:
            tick = self.data_client.generate_tick()
            self._advance_time(tick.timestamp)
            self.exchange.process_tick(tick)
            self.data_engine.process(tick)
            self.iteration += 1
        # ---------------------------------------------------------------------#

        self.log.debug("Stopping...")
        self.trader.stop()
        self.log.info("Stopped.")
        self._backtest_footer(run_started, self.clock.utc_now(), start, stop)
        if print_log_store:
            self.print_log_store()

    cdef void _advance_time(self, datetime timestamp) except *:
        cdef TradingStrategy strategy
        cdef TimeEventHandler event_handler
        cdef list time_events = []  # type: [TimeEventHandler]
        for strategy in self.trader.strategies():
            # noinspection: Object has warned attribute
            # noinspection PyUnresolvedReferences
            time_events += sorted(strategy.clock.advance_time(timestamp))
        for event_handler in time_events:
            self.test_clock.set_time(event_handler.event.timestamp)
            event_handler.handle()
        self.test_clock.set_time(timestamp)

    cpdef list get_log_store(self):
        """
        Return the store of log message strings for the test logger.


        Returns
        -------
        list[str]

        """
        return self.test_logger.get_log_store()

    cpdef void print_log_store(self) except *:
        """
        Print the contents of the test loggers store to the console.

        """
        self.log.info("")
        self.log.info("=================================================================")
        self.log.info(" LOG STORE")
        self.log.info("=================================================================")

        cdef list log_store = self.test_logger.get_log_store()
        cdef str message
        if not log_store:
            self.log.info("No log messages were stored.")
        else:
            for message in self.test_logger.get_log_store():
                print(message)

    cpdef void reset(self) except *:
        """
        Reset the backtest engine.

        All stateful values are reset to their initial value.
        """
        self.log.debug(f"Resetting...")

        self.iteration = 0
        self.data_engine.reset()
        if self.config.exec_db_flush:
            self.exec_engine.flush_db()
        self.exec_engine.reset()
        self.exec_client.reset()
        self.trader.reset()
        self.exchange.reset()
        self.logger.clear_log_store()
        self.test_logger.clear_log_store()

        self.log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.

        """
        self.trader.dispose()
        self.data_engine.dispose()
        self.exec_engine.dispose()

    cdef void _backtest_memory(self) except *:
        self.log.info("=================================================================")
        self.log.info(" MEMORY USAGE")
        self.log.info("=================================================================")
        ram_total_mb = round(psutil.virtual_memory()[0] / 1000000)
        ram_used__mb = round(psutil.virtual_memory()[3] / 1000000)
        ram_avail_mb = round(psutil.virtual_memory()[1] / 1000000)
        ram_avail_pc = round(100 - psutil.virtual_memory()[2], 2)
        self.log.info(f"RAM-Total: {ram_total_mb:,} MB")
        self.log.info(f"RAM-Used:  {ram_used__mb:,} MB ({round(100.0 - ram_avail_pc, 2)}%)")
        self.log.info(f"RAM-Avail: {ram_avail_mb:,} MB ({ram_avail_pc}%)")
        self.log.info(f"Data size: {format_bytes(get_size_of(self.data_engine))}")

    cdef void _backtest_header(
            self,
            datetime run_started,
            datetime start,
            datetime stop,
    ) except *:
        self.log.info("=================================================================")
        self.log.info(" BACKTEST RUN")
        self.log.info("=================================================================")
        self.log.info(f"Run started:    {format_iso8601(run_started)}")
        self.log.info(f"Backtest start: {format_iso8601(start)}")
        self.log.info(f"Backtest stop:  {format_iso8601(stop)}")
        for resolution in self.data_client.execution_resolutions:
            self.log.info(f"Execution resolution: {resolution}")
        if self.exchange.frozen_account:
            self.log.warning(f"ACCOUNT FROZEN")
        else:
            self.log.info(f"Account balance (starting): {self.config.starting_capital.to_string()}")
        self.log.info("=================================================================")

    cdef void _backtest_footer(
            self,
            datetime run_started,
            datetime run_finished,
            datetime start,
            datetime stop,
    ) except *:
        self.log.info("=================================================================")
        self.log.info(" BACKTEST DIAGNOSTICS")
        self.log.info("=================================================================")
        self.log.info(f"Run started:    {format_iso8601(run_started)}")
        self.log.info(f"Run finished:   {format_iso8601(run_finished)}")
        self.log.info(f"Backtest start: {format_iso8601(start)}")
        self.log.info(f"Backtest stop:  {format_iso8601(stop)}")
        self.log.info(f"Elapsed time:   {run_finished - run_started}")
        for resolution in self.data_client.execution_resolutions:
            self.log.info(f"Execution resolution: {resolution}")
        self.log.info(f"Iterations: {self.iteration:,}")
        self.log.info(f"Total events: {self.exec_engine.event_count:,}")
        self.log.info(f"Total orders: {self.exec_engine.cache.orders_total_count():,}")
        self.log.info(f"Total positions: {self.exec_engine.cache.positions_total_count():,}")
        if self.exchange.frozen_account:
            self.log.warning(f"ACCOUNT FROZEN")
        account_balance_starting = self.config.starting_capital.to_string()
        account_starting_length = len(account_balance_starting)
        account_balance_ending = pad_string(self.exchange.account_balance.to_string(), account_starting_length)
        commissions_total = pad_string(self.exchange.total_commissions.to_string(), account_starting_length)
        rollover_interest = pad_string(self.exchange.total_rollover.to_string(), account_starting_length)
        self.log.info(f"Account balance (starting): {account_balance_starting}")
        self.log.info(f"Account balance (ending):   {account_balance_ending}")
        self.log.info(f"Commissions (total):        {commissions_total}")
        self.log.info(f"Rollover interest (total):  {rollover_interest}")
        self.log.info("")

        self.log.info("=================================================================")
        self.log.info(" PERFORMANCE STATISTICS")
        self.log.info("=================================================================")
        self.analyzer.calculate_statistics(self.exchange.account, self.exec_engine.cache.positions())

        for statistic in self.analyzer.get_performance_stats_formatted(self.exchange.account.currency):
            self.log.info(statistic)
