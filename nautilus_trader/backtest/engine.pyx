# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import psutil
import pytz

from cpython.datetime cimport datetime
from typing import List, Dict, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.functions cimport as_utc_timestamp, format_zulu_datetime, format_bytes, pad_string, get_size_of
from nautilus_trader.common.logger cimport LogLevel
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.objects cimport Tick
from nautilus_trader.model.events cimport TimeEvent
from nautilus_trader.model.identifiers cimport Venue, TraderId, AccountId
from nautilus_trader.common.clock cimport LiveClock, TestClock
from nautilus_trader.common.guid cimport TestGuidFactory
from nautilus_trader.common.logger cimport TestLogger, nautilus_header
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.common.execution cimport ExecutionEngine, InMemoryExecutionDatabase
from nautilus_trader.trade.strategy cimport TradingStrategy
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.data cimport BacktestDataContainer, BacktestDataClient
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.live.execution cimport RedisExecutionDatabase
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer, MsgPackEventSerializer


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies inside a Trader
    on historical data.
    """

    def __init__(self,
                 BacktestDataContainer data not None,
                 list strategies not None: List[TradingStrategy],
                 BacktestConfig config=None,
                 FillModel fill_model=None):
        """
        Initializes a new instance of the BacktestEngine class.

        :param data: The data for the backtest engine.
        :param strategies: The initial strategies for the backtest engine.
        :param config: The optional configuration for the backtest engine (if None will be default).
        :param fill_model: The optional initial fill model for the backtest engine
        (if None then no probabilistic fills).
        :raises ConditionFailed: If the strategies list contains a type other than TradingStrategy.
        """
        if config is None:
            config = BacktestConfig()
        if fill_model is None:
            fill_model = FillModel()
        Condition.list_type(strategies, TradingStrategy, 'strategies')

        self.trader_id = TraderId('BACKTESTER', '000')
        self.account_id = AccountId.from_string('NAUTILUS-001-SIMULATED')
        self.config = config
        self.clock = LiveClock()
        self.created_time = self.clock.time_now()

        self.test_clock = TestClock()
        self.test_clock.set_time(self.clock.time_now())
        self.guid_factory = TestGuidFactory()

        self.logger = TestLogger(
            name=self.trader_id.value,
            bypass_logging=False,
            level_console=LogLevel.INFO,
            level_file=LogLevel.INFO,
            level_store=LogLevel.WARNING,
            console_prints=True,
            log_thread=config.log_thread,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
            clock=LiveClock())

        self.log = LoggerAdapter(component_name=self.__class__.__name__, logger=self.logger)

        self.test_logger = TestLogger(
            name=self.trader_id.value,
            bypass_logging=config.bypass_logging,
            level_console=config.level_console,
            level_file=config.level_file,
            level_store=config.level_store,
            console_prints=config.console_prints,
            log_thread=config.log_thread,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
            clock=self.test_clock)

        nautilus_header(self.log)
        self.log.info("#---------------------------------------------------------------#")
        self.log.info("Building engine...")

        if config.exec_db_type == 'in-memory':
            self.exec_db = InMemoryExecutionDatabase(
                trader_id=self.trader_id,
                logger=self.test_logger)
        elif config.exec_db_type == 'redis':
            self.exec_db = RedisExecutionDatabase(
                trader_id=self.trader_id,
                host='localhost',
                port=6379,
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                logger=self.test_logger)
        else:
            raise RuntimeError(f'The exec_db_type in the backtest configuration is unrecognized '
                               f'(can be either \'in-memory\' or \'redis\').')
        if self.config.exec_db_flush:
            self.exec_db.flush()

        self.test_clock.set_time(self.clock.time_now())  # For logging consistency
        self.data_client = BacktestDataClient(
            venue=Venue('BACKTEST'),
            data=data,
            clock=self.test_clock,
            logger=self.test_logger)

        self.portfolio = Portfolio(
            clock=self.test_clock,
            guid_factory=self.guid_factory,
            logger=self.test_logger)

        self.analyzer = PerformanceAnalyzer()

        self.exec_engine = ExecutionEngine(
            trader_id=self.trader_id,
            account_id=self.account_id,
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=self.test_clock,
            guid_factory=self.guid_factory,
            logger=self.test_logger)

        self.exec_client = BacktestExecClient(
            exec_engine=self.exec_engine,
            instruments=data.instruments,
            config=config,
            fill_model=fill_model,
            clock=self.test_clock,
            guid_factory=self.guid_factory,
            logger=self.test_logger)

        self.exec_engine.register_client(self.exec_client)
        self.exec_client.register_exec_db(self.exec_db)

        self._change_clocks_and_loggers(strategies)

        self.test_clock.set_time(self.clock.time_now())  # For logging consistency
        self.trader = Trader(
            trader_id=self.trader_id,
            account_id=self.account_id,
            strategies=strategies,
            data_client=self.data_client,
            exec_engine=self.exec_engine,
            clock=self.test_clock,
            guid_factory=self.guid_factory,
            logger=self.test_logger)

        self.iteration = 0

        self.time_to_initialize = self.clock.get_delta(self.created_time)
        self.log.info(f'Initialized in {self.time_to_initialize}.')
        self._backtest_memory()

    cpdef void run(
            self,
            datetime start=None,
            datetime stop=None,
            FillModel fill_model=None,
            list strategies=None,
            bint print_log_store=True) except *:
        """
        Run a backtest from the start datetime to the stop datetime.
        Note: If start datetime is None will run from the start of the data.
        Note: If stop datetime is None will run to the end of the data.

        :param start: The optional start datetime (UTC) for the backtest run.
        :param stop: The optional stop datetime (UTC) for the backtest run.
        :param fill_model: The optional fill model change for the backtest run (if None will use previous).
        :param strategies: The optional strategies change for the backtest run (if None will use previous).
        :param print_log_store: If the log store should be printed at the end of the run.
        :raises: ConditionFailed: If the stop is >= the start datetime.
        :raises: ConditionFailed: If the fill_model is a type other than FillModel or None.
        :raises: ConditionFailed: If the strategies contains a type other than TradingStrategy.
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

        Condition.true(start.tz == pytz.UTC, 'start.tz == UTC')
        Condition.true(stop.tz == pytz.UTC, 'stop.tz == UTC')
        Condition.true(start >= self.data_client.min_timestamp, 'start >= data_client.min_timestamp')
        Condition.true(start <= self.data_client.max_timestamp, 'stop <= data_client.max_timestamp')
        Condition.true(start < stop, 'start < stop')
        Condition.type_or_none(fill_model, FillModel, 'fill_model')
        if strategies:
            Condition.not_empty(strategies, 'strategies')
            Condition.list_type(strategies, TradingStrategy, 'strategies')
        # ---------------------------------------------------------------------#

        cdef datetime run_started = self.clock.time_now()

        # Setup logging
        self.test_logger.clear_log_store()
        if self.config.log_to_file:
            backtest_log_name = self.logger.name + '-' + format_zulu_datetime(run_started)
            self.logger.change_log_file_name(backtest_log_name)
            self.test_logger.change_log_file_name(backtest_log_name)

        self._backtest_header(run_started, start, stop)
        self.log.info(f"Setting up backtest...")

        # Setup clocks
        self.test_clock.set_time(start)
        assert(self.data_client.time_now() == start)
        assert(self.exec_client.time_now() == start)

        # Setup data
        self.data_client.setup(start, stop)

        # Setup new fill model
        if fill_model is not None:
            self.exec_client.change_fill_model(fill_model)

        # Setup new strategies
        if strategies is not None:
            self._change_clocks_and_loggers(strategies)
            self.trader.initialize_strategies(strategies)

        # Run the backtest
        self.log.info(f"Running backtest...")
        self.trader.start()

        cdef Tick tick
        cdef TradingStrategy strategy
        cdef TimeEvent event
        cdef dict time_events = {}  # type: Dict[TimeEvent, Callable]

        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        while self.data_client.has_data:
            tick = self.data_client.generate_tick()
            for strategy in self.trader.strategies:
                if strategy.clock.has_timers:
                    strategy.clock.advance_time(tick.timestamp)
                    time_events.update(strategy.clock.pop_events())
                else:
                    strategy.clock.set_time(tick.timestamp)
            if time_events:
                for event, handler in dict(sorted(time_events.items())).items():
                    self.test_clock.set_time(event.timestamp)
                    handler(event)
                time_events.clear()
            self.test_clock.set_time(tick.timestamp)
            self.exec_client.process_tick(tick)
            self.data_client.process_tick(tick)
            self.iteration += 1
        # ---------------------------------------------------------------------#

        self.log.debug("Stopping...")
        self.trader.stop()
        self.log.info("Stopped.")
        self._backtest_footer(run_started, self.clock.time_now(), start, stop)
        if print_log_store:
            self.print_log_store()

    cpdef list get_log_store(self):
        """
        Return the store of log message strings for the test logger.
        
        :return List[str].
        """
        return self.test_logger.get_log_store()

    cpdef void print_log_store(self) except *:
        """
        Print the contents of the test loggers store to the console.
        """
        self.log.info("")
        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#-------------------------- LOG STORE --------------------------#")
        self.log.info("#---------------------------------------------------------------#")

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
        
        The following components are reset;
        
        - DataClient
        - ExecutionEngine
        - ExecutionClient
        - Trader (including all strategies)
        """
        self.log.info(f"Resetting...")
        self.iteration = 0
        self.data_client.reset()
        self.exec_db.reset()
        if self.config.exec_db_flush:
            self.exec_db.flush()
        self.exec_engine.reset()
        self.exec_client.reset()
        self.trader.reset()
        self.logger.clear_log_store()
        self.test_logger.clear_log_store()
        self.log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.
        """
        self.trader.dispose()

    cdef void _backtest_memory(self) except *:
        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#----------------------- MEMORY USAGE --------------------------#")
        self.log.info("#---------------------------------------------------------------#")
        ram_total_mb = round(psutil.virtual_memory()[0] / 1000000)
        ram_used__mb = round(psutil.virtual_memory()[3] / 1000000)
        ram_avail_mb = round(psutil.virtual_memory()[1] / 1000000)
        ram_avail_pc = round(100 - psutil.virtual_memory()[2], 2)
        self.log.info(f"RAM-Total: {ram_total_mb:,} MB")
        self.log.info(f"RAM-Used:  {ram_used__mb:,} MB ({round(100.0 - ram_avail_pc, 2)}%)")
        self.log.info(f"RAM-Avail: {ram_avail_mb:,} MB ({ram_avail_pc}%)")
        self.log.info(f"Data size: {format_bytes(get_size_of(self.data_client))}")

    cdef void _backtest_header(
            self,
            datetime run_started,
            datetime start,
            datetime stop) except *:
        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#----------------------- BACKTEST RUN --------------------------#")
        self.log.info("#---------------------------------------------------------------#")
        self.log.info(f"Run started:    {format_zulu_datetime(run_started)}")
        self.log.info(f"Backtest start: {format_zulu_datetime(start)}")
        self.log.info(f"Backtest stop:  {format_zulu_datetime(stop)}")
        self.log.info(f"Execution resolutions: {self.data_client.execution_resolutions}")
        if self.exec_client.frozen_account:
            self.log.warning(f"ACCOUNT FROZEN")
        else:
            currency = currency_to_string(self.config.account_currency)
            self.log.info(f"Account balance (starting): {self.config.starting_capital.to_string(format_commas=True)} {currency}")
        self.log.info("#---------------------------------------------------------------#")

    cdef void _backtest_footer(
            self,
            datetime run_started,
            datetime run_finished,
            datetime start,
            datetime stop) except *:
        cdef str account_currency = currency_to_string(self.config.account_currency)
        cdef int account_starting_length = len(self.config.starting_capital.to_string(format_commas=True))

        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#-------------------- BACKTEST DIAGNOSTICS ---------------------#")
        self.log.info("#---------------------------------------------------------------#")
        self.log.info(f"Run started:    {format_zulu_datetime(run_started)}")
        self.log.info(f"Run finished:   {format_zulu_datetime(run_finished)}")
        self.log.info(f"Backtest start: {format_zulu_datetime(start)}")
        self.log.info(f"Backtest stop:  {format_zulu_datetime(stop)}")
        self.log.info(f"Elapsed time:   {run_finished - run_started}")
        self.log.info(f"Execution resolutions: {self.data_client.execution_resolutions}")
        self.log.info(f"Iterations: {self.iteration}")
        self.log.info(f"Total events: {self.exec_engine.event_count}")
        self.log.info(f"Total orders: {self.exec_engine.database.count_orders_total()}")
        self.log.info(f"Total positions: {self.exec_engine.database.count_positions_total()}")
        if self.exec_client.frozen_account:
            self.log.warning(f"ACCOUNT FROZEN")
        account_balance_starting = self.config.starting_capital.to_string(format_commas=True)
        account_balance_ending = pad_string(self.exec_client.account_capital.to_string(format_commas=True), account_starting_length)
        commissions_total = pad_string(self.exec_client.total_commissions.to_string(format_commas=True), account_starting_length)
        rollover_interest = pad_string(self.exec_client.total_rollover.to_string(format_commas=True), account_starting_length)
        self.log.info(f"Account balance (starting): {account_balance_starting} {account_currency}")
        self.log.info(f"Account balance (ending):   {account_balance_ending} {account_currency}")
        self.log.info(f"Commissions (total):        {commissions_total} {account_currency}")
        self.log.info(f"Rollover interest (total):  {rollover_interest} {account_currency}")
        self.log.info("")

        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#-------------------- PERFORMANCE STATISTICS -------------------#")
        self.log.info("#---------------------------------------------------------------#")
        self.log.info("Calculating statistics...")
        self.analyzer.calculate_statistics(self.exec_engine.account, self.exec_engine.database.get_positions())

        for statistic in self.analyzer.get_performance_stats_formatted():
            self.log.info(statistic)

    cdef void _change_clocks_and_loggers(self, list strategies) except *:
        # Replace the clocks and loggers for every strategy in the given list
        for strategy in strategies:
            # Separate test clocks to iterate independently
            strategy.change_clock(TestClock(initial_time=self.test_clock.time_now()))
            # Replace the strategies logger with the engines test logger
            strategy.change_logger(self.test_logger)
