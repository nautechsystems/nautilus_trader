# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import psutil
import pytz

from cpython.datetime cimport datetime, timedelta
from pandas import DataFrame
from typing import List, Dict, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport as_utc_timestamp, format_zulu_datetime, pad_string
from nautilus_trader.common.logger cimport LogLevel
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.c_enums.resolution cimport Resolution, resolution_to_string
from nautilus_trader.model.objects cimport Instrument, Tick
from nautilus_trader.model.events cimport TimeEvent
from nautilus_trader.model.identifiers cimport Symbol, Venue, TraderId
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.brokerage import CommissionCalculator
from nautilus_trader.common.clock cimport LiveClock, TestClock
from nautilus_trader.common.guid cimport TestGuidFactory
from nautilus_trader.common.logger cimport TestLogger, nautilus_header
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.execution cimport ExecutionEngine, InMemoryExecutionDatabase

from nautilus_trader.trade.strategy cimport TradingStrategy
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.data cimport BidAskBarPair, BacktestDataClient
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
                 TraderId trader_id,
                 Venue venue,
                 list instruments: List[Instrument],
                 dict data_ticks: Dict[Symbol, DataFrame],
                 dict data_bars_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict data_bars_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 list strategies: List[TradingStrategy],
                 FillModel fill_model=FillModel(),
                 BacktestConfig config=BacktestConfig()):
        """
        Initializes a new instance of the BacktestEngine class.

        :param trader_id: The trader identifier for the backtest engine.
        :param venue: The venue for the backtest engine.
        :param data_ticks: The tick data for the backtest engine.
        :param data_bars_bid: The bid bar data needed for the backtest engine.
        :param data_bars_ask: The ask bar data needed for the backtest engine.
        :param strategies: The initial strategies for the backtest engine.
        :param fill_model: The initial fill model for the backtest engine.
        :param config: The configuration for the backtest engine.
        :raises ConditionFailed: If the instruments list contains a type other than Instrument.
        :raises ConditionFailed: If the strategies list contains a type other than TradingStrategy.
        """
        if trader_id is None:
            trader_id = TraderId('BACKTESTER', '000')

        # Data checked in BacktestDataClient
        Condition.list_type(instruments, Instrument, 'instruments')
        Condition.list_type(strategies, TradingStrategy, 'strategies')

        self.config = config
        self.clock = LiveClock()
        self.created_time = self.clock.time_now()

        self.test_clock = TestClock()
        self.test_clock.set_time(self.clock.time_now())
        self.guid_factory = TestGuidFactory()

        self.logger = TestLogger(
            name=trader_id.value,
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
            name=trader_id.value,
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
        self.log.info("Building engine...")

        # Execution Database
        if config.exec_db_type == 'in-memory':
            self.exec_db = InMemoryExecutionDatabase(trader_id=trader_id, logger=self.test_logger)
        elif config.exec_db_type == 'redis':
            self.exec_db = RedisExecutionDatabase(
                trader_id=trader_id,
                host='localhost',
                port=6379,
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                logger=self.test_logger,
                load_cache=False)
        else:
            raise RuntimeError(f'The exec_db_type in the backtest configuration is unrecognized '
                               f'(can be either \'in-memory\' or \'redis\').')
        if self.config.exec_db_flush:
            self.exec_db.flush()

        self.instruments = instruments
        self.data_client = BacktestDataClient(
            venue=venue,
            instruments=instruments,
            data_ticks=data_ticks,
            data_bars_bid=data_bars_bid,
            data_bars_ask=data_bars_ask,
            clock=self.test_clock,
            logger=self.test_logger)

        self.account = Account(currency=config.account_currency)

        self.portfolio = Portfolio(
            clock=self.test_clock,
            guid_factory=self.guid_factory,
            logger=self.test_logger)

        self.exec_engine = ExecutionEngine(
            database=self.exec_db,
            account=self.account,
            portfolio=self.portfolio,
            clock=self.test_clock,
            guid_factory=self.guid_factory,
            logger=self.test_logger)

        self.exec_client = BacktestExecClient(
            exec_engine=self.exec_engine,
            instruments=instruments,
            frozen_account=config.frozen_account,
            starting_capital=config.starting_capital,
            fill_model=fill_model,
            commission_calculator=CommissionCalculator(default_rate_bp=config.commission_rate_bp),
            account=self.account,
            portfolio=self.portfolio,
            clock=self.test_clock,
            guid_factory=self.guid_factory,
            logger=self.test_logger)

        self.exec_engine.register_client(self.exec_client)

        for strategy in strategies:
            # Replace strategies clocks with separate test clocks for independent iteration
            strategy.change_clock(TestClock())

        self.trader = Trader(
            trader_id=trader_id,
            strategies=strategies,
            data_client=self.data_client,
            exec_engine=self.exec_engine,
            clock=self.test_clock,
            logger=self.test_logger)

        self.iteration = 0

        self.time_to_initialize = self.clock.get_delta(self.created_time)
        self.log.info(f'Initialized in {self.time_to_initialize}.')

    cpdef run(
            self,
            datetime start=None,
            datetime stop=None,
            timedelta time_step=None,
            FillModel fill_model=None,
            list strategies=None,
            bint print_log_store=True):
        """
        Run the backtest with the given parameters.

        :param start: The start UTC datetime for the backtest run (optional can be None - will run from the start of the data).
        :param stop: The stop UTC datetime for the backtest run (optional can be None - will run to the end of the data).
        :param time_step: The iteration time step for the backtest run.
        :param fill_model: The fill model change for the backtest run (optional can be None - will use previous).
        :param strategies: The strategies change for the backtest run (optional can be None - will use previous).
        :param print_log_store: The flag indicating whether the log store should be printed at the end of the run.

        :raises: ValueError: If the start is not None and timezone is not UTC.
        :raises: ValueError: If the start is not < the stop datetime.
        :raises: ValueError: If the start is not >= the execution_data_index_min datetime.
        :raises: ValueError: If the stop is not None and timezone is not UTC.
        :raises: ValueError: If the stop is not <= the execution_data_index_max datetime.
        :raises: ValueError: If the time_step is not None and is > the max time step for the execution resolution.
        :raises: ValueError: If the fill_model is a type other than FillModel or None.
        :raises: ValueError: If the strategies is a type other than list or None.
        :raises: ValueError: If the strategies list is not None and is empty, or contains a type other than TradingStrategy.
        """
        #  Setup start datetime
        if start is None:
            start = self.data_client.execution_data_index_min
        else:
            start = as_utc_timestamp(start)

        # Setup stop datetime
        if stop is None:
            stop = self.data_client.execution_data_index_max
        else:
            stop = as_utc_timestamp(stop)

        # Setup time step
        if time_step is None:
            time_step = self.data_client.max_time_step
        else:
            Condition.true(time_step <= self.data_client.max_time_step, 'time_step <= data_client.max_time_step')

        Condition.true(start.tz == pytz.UTC, 'start.tz == UTC')
        Condition.true(stop.tz == pytz.UTC, 'stop.tz == UTC')
        Condition.true(start >= self.data_client.execution_data_index_min, 'start >= execution_data_index_min')
        Condition.true(start <= self.data_client.execution_data_index_max, 'stop <= execution_data_index_max')
        Condition.true(start < stop, 'start < stop')
        Condition.type_or_none(fill_model, FillModel, 'fill_model')
        Condition.type_or_none(strategies, list, 'strategies')
        if strategies is not None:
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

        # Setup clocks
        self.test_clock.set_time(start)
        assert(self.data_client.time_now() == start)
        assert(self.exec_client.time_now() == start)

        # Setup data
        self._backtest_header(run_started, start, stop, time_step)
        self.log.info(f"Setting up backtest...")
        self.log.debug("Setting initial iterations...")
        self.data_client.set_initial_iteration_indexes(start)  # Also sets clock to start time

        # Setup new fill model
        if fill_model is not None:
            self.exec_client.change_fill_model(fill_model)

        # Setup new strategies
        if strategies is not None:
            self.trader.initialize_strategies(strategies)
        self._change_clocks_and_loggers(self.trader.strategies)

        # -- RUN BACKTEST -----------------------------------------------------#
        self.log.info(f"Running backtest...")
        self.trader.start()

        if self.data_client.execution_resolution == Resolution.TICK:
            self._run_with_tick_execution(start, stop, time_step)
        else:
            self._run_with_bar_execution(start, stop, time_step)
        # ---------------------------------------------------------------------#

        self.log.info("Stopping...")
        self.trader.stop()
        self.log.info("Stopped.")
        self._backtest_footer(run_started, start, stop, time_step)
        if print_log_store:
            self.print_log_store()

    cdef void _run_with_tick_execution(
            self,
            datetime start,
            datetime stop,
            timedelta time_step):
        # Run the backtest with tick level execution resolution

        cdef Tick tick
        cdef TradingStrategy strategy
        cdef dict time_events

        cdef datetime time = start

        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        while time <= stop:
            for tick in self.data_client.iterate_ticks(time):
                self.test_clock.set_time(tick.timestamp)
                self.exec_client.process_tick(tick)
                time_events = {}  # type: Dict[TimeEvent, Callable]
                for strategy in self.trader.strategies:
                    time_events = {**time_events, **strategy.iterate(tick.timestamp)}
                for event, handler in dict(sorted(time_events.items())).items():
                    self.test_clock.set_time(event.timestamp)
                    handler(event)
                self.test_clock.set_time(tick.timestamp)
                self.data_client.process_tick(tick)
            self.test_clock.set_time(time)
            self.data_client.process_bars(self.data_client.iterate_bars(time))
            time += time_step
            self.iteration += 1
        # ---------------------------------------------------------------------#

    cdef void _run_with_bar_execution(
            self,
            datetime start,
            datetime stop,
            timedelta time_step):
        # Run the backtest with bar level execution resolution

        cdef Symbol symbol
        cdef Tick tick
        cdef TradingStrategy strategy
        cdef BidAskBarPair execution_bars
        cdef dict time_events

        cdef datetime time = start

        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        while time <= stop:
            time_events = {}  # type: Dict[TimeEvent, Callable]
            for strategy in self.trader.strategies:
                time_events = {**time_events, **strategy.iterate(time)}
            for event, handler in dict(sorted(time_events.items())).items():
                self.test_clock.set_time(event.timestamp)
                handler(event)
            for symbol, execution_bars in self.data_client.get_next_execution_bars(time).items():
                self.exec_client.process_bars(symbol, execution_bars.bid, execution_bars.ask)
            for tick in self.data_client.iterate_ticks(time):
                self.data_client.process_tick(tick)
            self.test_clock.set_time(time)
            self.data_client.process_bars(self.data_client.iterate_bars(time))
            time += time_step
            self.iteration += 1
        # ---------------------------------------------------------------------#

    cpdef void create_returns_tear_sheet(self):
        """
        Create a returns tear sheet based on analyzer data from the last run.
        """
        self.trader.create_returns_tear_sheet()

    cpdef void create_full_tear_sheet(self):
        """
        Create a full tear sheet based on analyzer data from the last run.
        """
        self.trader.create_full_tear_sheet()

    cpdef dict get_performance_stats(self):
        """
        Return the performance statistics from the last backtest run.
        
        Note: Money objects are converted to floats.
        
        :return Dict[str, float].
        """
        return self.portfolio.analyzer.get_performance_stats()

    cpdef object get_orders_report(self):
        """
        Return an orders report dataframe.
        """
        return self.trader.get_orders_report()

    cpdef object get_order_fills_report(self):
        """
        Return an order fill report dataframe.
        """
        return self.trader.get_order_fills_report()

    cpdef object get_positions_report(self):
        """
        Return a positions report dataframe.
        """
        return self.trader.get_positions_report()

    cpdef list get_log_store(self):
        """
        Return the store of log message strings for the test logger.
        
        :return List[str].
        """
        return self.test_logger.get_log_store()

    cpdef void print_log_store(self):
        """
        Print the contents of the test loggers store to the console.
        """
        self.log.info("")
        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#-------------------------- LOG STORE --------------------------#")
        self.log.info("#---------------------------------------------------------------#")

        cdef list log_store = self.test_logger.get_log_store()
        cdef str message
        if len(log_store) == 0:
            self.log.info("No log messages were stored.")
        else:
            for message in self.test_logger.get_log_store():
                print(message)

    cpdef void reset(self):
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

    cpdef void dispose(self):
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.
        """
        self.trader.dispose()

    cdef void _backtest_header(
            self,
            datetime run_started,
            datetime start,
            datetime stop,
            timedelta time_step):
        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#----------------------- BACKTEST RUN --------------------------#")
        self.log.info("#---------------------------------------------------------------#")
        self.log.info(f"RAM-Used:  {round(psutil.virtual_memory()[3] / 1000000)}MB")
        self.log.info(f"RAM-Avail: {round(psutil.virtual_memory()[1] / 1000000)}MB ({round(100 - psutil.virtual_memory()[2], 2)}%)")
        self.log.info(f"Run started datetime: {format_zulu_datetime(run_started)}")
        self.log.info(f"Backtest start datetime: {format_zulu_datetime(start)}")
        self.log.info(f"Backtest stop datetime:  {format_zulu_datetime(stop)}")
        self.log.info(f"Iteration time-step: {time_step}")
        self.log.info(f"Execution resolution: {resolution_to_string(self.data_client.execution_resolution)}")
        if self.exec_client.frozen_account:
            self.log.warning(f"ACCOUNT FROZEN")
        else:
            self.log.info(f"Account balance (starting): {self.config.starting_capital} {currency_to_string(self.account.currency)}")
        self.log.info("#---------------------------------------------------------------#")

    cdef void _backtest_footer(
            self,
            datetime run_started,
            datetime start,
            datetime stop,
            timedelta time_step):
        cdef str account_currency = currency_to_string(self.account.currency)
        cdef int account_starting_length = len(str(self.config.starting_capital))

        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#-------------------- BACKTEST DIAGNOSTICS ---------------------#")
        self.log.info("#---------------------------------------------------------------#")
        self.log.info(f"Run started datetime: {format_zulu_datetime(run_started)}")
        self.log.info(f"Elapsed time (engine initialization): {self.time_to_initialize}")
        self.log.info(f"Elapsed time (running backtest):      {self.clock.get_delta(run_started)}")
        self.log.info(f"Backtest start datetime: {format_zulu_datetime(start)}")
        self.log.info(f"Backtest stop datetime:  {format_zulu_datetime(stop)}")
        self.log.info(f"Time-step iterations: {self.iteration} of {time_step}")
        self.log.info(f"Execution resolution: {resolution_to_string(self.data_client.execution_resolution)}")
        self.log.info(f"Total events: {self.exec_engine.event_count}")
        self.log.info(f"Total orders: {self.exec_engine.database.count_orders_total()}")
        self.log.info(f"Total positions: {self.exec_engine.database.count_positions_total()}")
        if self.exec_client.frozen_account:
            self.log.warning(f"ACCOUNT FROZEN")
        self.log.info(f"Account balance (starting): {self.config.starting_capital} {account_currency}")
        self.log.info(f"Account balance (ending):   {pad_string(str(self.account.cash_balance), account_starting_length)} {account_currency}")
        self.log.info(f"Commissions (total):        {pad_string(str(self.exec_client.total_commissions), account_starting_length)} {account_currency}")
        self.log.info("")

        self.log.info("#---------------------------------------------------------------#")
        self.log.info("#-------------------- PERFORMANCE STATISTICS -------------------#")
        self.log.info("#---------------------------------------------------------------#")

        for statistic in self.portfolio.analyzer.get_performance_stats_formatted():
            self.log.info(statistic)

    cdef void _change_clocks_and_loggers(self, list strategies):
        # Replace the clocks and loggers for every strategy in the given list

        for strategy in strategies:
            # Separate test clocks to iterate independently
            strategy.change_clock(TestClock())
            strategy.set_time(self.test_clock.time_now())
            # Replace the strategies logger with the engines test logger
            strategy.change_logger(self.test_logger)
