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

import socket
from decimal import Decimal
from typing import List, Optional, Union

import pandas as pd
import pydantic
import pytz

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.backtest.data_client cimport BacktestDataClient
from nautilus_trader.backtest.data_client cimport BacktestMarketDataClient
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport LogLevelParser
from nautilus_trader.common.logging cimport log_memory
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.infrastructure.cache cimport RedisCacheDatabase
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.aggregation_source cimport AggregationSource
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.venue_type cimport VenueType
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.base cimport GenericData
from nautilus_trader.model.data.tick cimport Tick
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.portfolio.portfolio cimport Portfolio
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.serialization.msgpack.serializer cimport MsgPackSerializer
from nautilus_trader.trading.strategy cimport TradingStrategy

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.cache.cache import CacheConfig
from nautilus_trader.data.engine import DataEngineConfig
from nautilus_trader.execution.engine import ExecEngineConfig
from nautilus_trader.infrastructure.cache import CacheDatabaseConfig
from nautilus_trader.risk.engine import RiskEngineConfig


class BacktestEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``BacktestEngine`` instances.

    trader_id : str, default="BACKTESTER-000"
        The trader ID.
    log_level : str, default="INFO"
        The minimum log level for logging messages to stdout.
    cache : CacheConfig, optional
        The configuration for the cache.
    cache_database : CacheDatabaseConfig, optional
        The configuration for the cache database.
    data_engine : DataEngineConfig, optional
        The configuration for the data engine.
    risk_engine : RiskEngineConfig, optional
        The configuration for the risk engine.
    exec_engine : ExecEngineConfig, optional
        The configuration for the execution engine.
    bypass_logging : bool, default=False
        If logging should be bypassed.
    run_analysis : bool, default=True
        If post backtest performance analysis should be run.

    """

    trader_id: str = "BACKTESTER-000"
    log_level: str = "INFO"
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: Optional[DataEngineConfig] = None
    risk_engine: Optional[RiskEngineConfig] = None
    exec_engine: Optional[ExecEngineConfig] = None
    bypass_logging: bool = False
    run_analysis: bool = True

    def __dask_tokenize__(self):
        return tuple(self.dict().items())


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies over historical
    data.
    """

    def __init__(self, config: Optional[BacktestEngineConfig]=None):
        """
        Initialize a new instance of the ``BacktestEngine`` class.

        Parameters
        ----------
        config : BacktestEngineConfig, optional
            The configuration for the instance.

        Raises
        ------
        TypeError
            If `config` is not of type `BacktestEngineConfig`.

        """
        if config is None:
            config = BacktestEngineConfig()
        Condition.type(config, BacktestEngineConfig, "config")

        # Configuration
        self._config = config

        self._exchanges = {}

        # Data
        self._data = []
        self._data_len = 0
        self._index = 0

        # Setup components
        self._clock = LiveClock()
        created_time = self._clock.utc_now()
        self._test_clock = TestClock()
        self._uuid_factory = UUIDFactory()

        # Identifiers
        self.trader_id = TraderId(config.trader_id)
        self.machine_id = socket.gethostname()
        self.instance_id = self._uuid_factory.generate()

        self._logger = Logger(
            clock=LiveClock(),
            trader_id=self.trader_id,
            machine_id=self.machine_id,
            instance_id=self.instance_id,
        )

        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=self._logger,
        )

        self._test_logger = Logger(
            clock=self._clock,
            trader_id=self.trader_id,
            machine_id=self.machine_id,
            instance_id=self.instance_id,
            level_stdout=LogLevelParser.from_str(config.log_level.upper()),
            bypass=config.bypass_logging,
        )

        nautilus_header(self._log)
        self._log.info("=================================================================")
        self._log.info("Building engine...")

        ########################################################################
        # Build platform
        ########################################################################
        if config.cache_database is None or config.cache_database.type == "in-memory":
            cache_db = None
        elif config.cache_database.type == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=self.trader_id,
                logger=self._test_logger,
                serializer=MsgPackSerializer(timestamps_as_str=True),
                config=config.cache_database,
            )
        else:
            raise ValueError(
                f"The cache_db_type in the configuration is unrecognized, "
                f"can one of {{\'in-memory\', \'redis\'}}.",
            )

        self._msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._cache = Cache(
            database=cache_db,
            logger=self._test_logger,
            config=config.cache,
        )
        # Set external facade
        self.cache = self._cache

        self._portfolio = Portfolio(
            msgbus=self._msgbus,
            cache=self.cache,
            clock=self._test_clock,
            logger=self._test_logger,
        )
        # Set external facade
        self.portfolio = self._portfolio

        self._data_engine = DataEngine(
            msgbus=self._msgbus,
            cache=self.cache,
            clock=self._test_clock,
            logger=self._test_logger,
            config=config.data_engine,
        )

        self._exec_engine = ExecutionEngine(
            msgbus=self._msgbus,
            cache=self.cache,
            clock=self._test_clock,
            logger=self._test_logger,
            config=config.exec_engine,
        )
        self._exec_engine.load_cache()

        self._risk_engine = RiskEngine(
            portfolio=self._portfolio,
            msgbus=self._msgbus,
            cache=self.cache,
            clock=self._test_clock,
            logger=self._test_logger,
            config=config.risk_engine,
        )

        self.trader = Trader(
            trader_id=self.trader_id,
            msgbus=self._msgbus,
            cache=self._cache,
            portfolio=self.portfolio,
            data_engine=self._data_engine,
            risk_engine=self._risk_engine,
            exec_engine=self._exec_engine,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self.analyzer = PerformanceAnalyzer()

        self.iteration = 0
        self._run_started = None
        self._backtest_start = None

        self._log.info(
            f"Initialized in "
            f"{int(self._clock.delta(created_time).total_seconds() * 1000)}ms.",
        )

    def list_venues(self):
        """
        Return the venues contained within the engine.

        Returns
        -------
        List[Venue]

        """
        return list(self._exchanges)

    def get_exec_engine(self) -> ExecutionEngine:
        """
        Return the execution engine for the backtest engine (used for testing).

        Returns
        -------
        ExecutionEngine

        """
        return self._exec_engine

    def add_generic_data(self, ClientId client_id, list data) -> None:
        """
        Add the generic data to the container.

        Parameters
        ----------
        client_id : ClientId
            The data client ID to associate with the generic data.
        data : list[GenericData]
            The data to add.

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_empty(data, "data")
        Condition.list_type(data, GenericData, "data")

        # Check client has been registered
        self._add_data_client_if_not_exists(client_id)

        # Add data
        self._data = sorted(self._data + data, key=lambda x: x.ts_init)

        self._log.info(
            f"Added {len(data)} {type(data[0].data).__name__} "
            f"GenericData element{'' if len(data) == 1 else 's'}.",
        )

    def add_instrument(self, Instrument instrument) -> None:
        """
        Add the instrument to the backtest engine.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        Condition.not_none(instrument, "instrument")

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument.id.venue)

        # Add data
        self._data_engine.process(instrument)
        self._cache.add_instrument(instrument)

        self._log.info(f"Added {instrument.id} Instrument.")

    def add_order_book_data(self, list data) -> None:
        """
        Add the order book data to the backtest engine.

        Parameters
        ----------
        data : list[OrderBookData]
            The order book data to add.

        Raises
        ------
        ValueError
            If `data` is empty.
        ValueError
            If `instrument_id` is not found in the cache.

        """
        Condition.not_empty(data, "data")
        Condition.list_type(data, OrderBookData, "data")
        cdef OrderBookData first = data[0]
        Condition.true(
            first.instrument_id in self._cache.instrument_ids(),
            "Instrument for given data not found in the cache. "
            "Please call `add_instrument()` before adding related data.",
        )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(first.instrument_id.venue)

        # Add data
        self._data = sorted(self._data + data, key=lambda x: x.ts_init)

        self._log.info(
            f"Added {len(data):,} {first.instrument_id} "
            f"OrderBookData element{'' if len(data) == 1 else 's'}.",
        )

    def add_ticks(self, list data) -> None:
        """
        Add the tick data to the backtest engine.

        Parameters
        ----------
        data : list[Tick]
            The tick data to add.

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        Condition.not_empty(data, "data")
        Condition.list_type(data, Tick, "data")
        cdef Tick first = data[0]
        Condition.true(
            first.instrument_id in self._cache.instrument_ids(),
            "Instrument for given data not found in the cache. "
            "Please call `add_instrument()` before adding related data.",
        )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(first.instrument_id.venue)

        # Add data
        self._data = sorted(self._data + data, key=lambda x: x.ts_init)

        self._log.info(
            f"Added {len(data):,} {first.instrument_id} "
            f"{type(first).__name__} element{'' if len(data) == 1 else 's'}.",
        )

    def add_bars(self, list data) -> None:
        """
        Add the built bar data objects to the backtest engines. Suitable for
        running externally aggregated bar subscriptions (bar type aggregation
        source must be ``EXTERNAL``).

        Parameters
        ----------
        data : list[Bar]
            The bars to add.

        Raises
        ------
        ValueError
            If `bar_type.aggregation_source` is not equal to ``EXTERNAL``.
        ValueError
            If `data` is empty.
        ValueError
            If `instrument_id` is not found in the cache.

        """
        Condition.not_empty(data, "data")
        Condition.list_type(data, Bar, "data")
        cdef Bar first = data[0]
        Condition.true(
            first.type.instrument_id in self._cache.instrument_ids(),
            "Instrument for given data not found in the cache. "
            "Please call `add_instrument()` before adding related data.",
        )
        Condition.equal(
            first.type.aggregation_source,
            AggregationSource.EXTERNAL,
            "bar_type.aggregation_source",
            "required source",
        )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(first.type.instrument_id.venue)

        # Add data
        self._data = sorted(self._data + data, key=lambda x: x.ts_init)

        self._log.info(
            f"Added {len(data):,} {first.type} "
            f"Bar element{'' if len(data) == 1 else 's'}.",
        )

    def add_venue(
        self,
        Venue venue,
        VenueType venue_type,
        OMSType oms_type,
        AccountType account_type,
        Currency base_currency,
        list starting_balances,
        default_leverage=None,
        dict leverages=None,
        bint is_frozen_account=False,
        list modules=None,
        FillModel fill_model=None,
        BookType book_type=BookType.L1_TBBO,
        fill_limit_at_price=False,
    ) -> None:
        """
        Add a `SimulatedExchange` with the given parameters to the backtest engine.

        Parameters
        ----------
        venue : Venue
            The exchange venue ID.
        venue_type : VenueType
            The type of venue (will determine venue -> client_id mapping).
        oms_type : OMSType {``HEDGING``, ``NETTING``}
            The order management system type for the exchange. If ``HEDGING`` will
            generate new position IDs.
        account_type : AccountType
            The account type for the client.
        base_currency : Currency, optional
            The account base currency for the client. Use ``None`` for multi-currency accounts.
        starting_balances : list[Money]
            The starting account balances (specify one for a single asset account).
        default_leverage : Decimal
            The account default leverage (for margin accounts).
        leverages : Dict[InstrumentId, Decimal]
            The instrument specific leverage configuration (for margin accounts).
        is_frozen_account : bool
            If the account for this exchange is frozen (balances will not change).
        modules : list[SimulationModule, optional
            The simulation modules to load into the exchange.
        fill_model : FillModel, optional
            The fill model for the exchange (if None then no probabilistic fills).
        book_type : BookType
            The default order book type for fill modelling.
        fill_limit_at_price : bool
            If limit orders should be filled at their original price only (overrides slippage).

        Raises
        ------
        ValueError
            If an exchange of `venue` is already registered with the engine.

        """
        if modules is None:
            modules = []
        if fill_model is None:
            fill_model = FillModel()
        Condition.not_none(venue, "venue")
        Condition.not_in(venue, self._exchanges, "venue", "self._exchanges")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules")
        Condition.type_or_none(fill_model, FillModel, "fill_model")

        # Create exchange
        exchange = SimulatedExchange(
            venue=venue,
            venue_type=venue_type,
            oms_type=oms_type,
            account_type=account_type,
            base_currency=base_currency,
            starting_balances=starting_balances,
            default_leverage=default_leverage or Decimal(10),
            leverages=leverages or {},
            is_frozen_account=is_frozen_account,
            instruments=self._cache.instruments(venue),
            modules=modules,
            cache=self._cache,
            fill_model=fill_model,
            book_type=book_type,
            clock=self._test_clock,
            logger=self._test_logger,
            fill_limit_at_price=fill_limit_at_price,
        )

        self._exchanges[venue] = exchange

        # Create execution client for exchange
        exec_client = BacktestExecClient(
            exchange=exchange,
            account_id=AccountId(venue.value, "001"),
            account_type=account_type,
            base_currency=base_currency,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._test_clock,
            logger=self._test_logger,
            is_frozen_account=is_frozen_account,
        )

        exchange.register_client(exec_client)
        self._exec_engine.register_client(exec_client)

        self._log.info(f"Added {exchange}.")

    def reset(self) -> None:
        """
        Reset the backtest engine.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        if self.trader.is_running_c():
            # End current backtest run
            self._end()

        # Reset DataEngine
        if self._data_engine.is_running_c():
            self._data_engine.stop()
        self._data_engine.reset()

        # Reset ExecEngine
        if self._exec_engine.is_running_c():
            self._exec_engine.stop()
        if self._config.cache_database is not None and self._config.cache_database.flush:
            self._exec_engine.flush_db()
        self._exec_engine.reset()

        # Reset RiskEngine
        if self._risk_engine.is_running_c():
            self._risk_engine.stop()
        self._risk_engine.reset()

        self.trader.reset()

        for exchange in self._exchanges.values():
            exchange.reset()

        self.iteration = 0
        self._run_started = None

        self._log.info("Reset.")

    def clear_data(self):
        """
        Clear the engines internal data stream.
        """
        self._data.clear()
        self._data_len = 0
        self._index = 0

    def dispose(self) -> None:
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.
        """
        self.trader.dispose()

        if self._data_engine.is_running_c():
            self._data_engine.stop()
        if self._exec_engine.is_running_c():
            self._exec_engine.stop()

        self._data_engine.dispose()
        self._exec_engine.dispose()
        self._risk_engine.dispose()

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
        Condition.is_in(venue, self._exchanges, "venue", "self._exchanges")

        self._exchanges[venue].set_fill_model(model)

    def add_strategy(self, strategy: TradingStrategy) -> None:
        # Checked inside trader
        self.trader.add_strategy(strategy)

    def add_strategies(self, strategies: List[TradingStrategy]) -> None:
        # Checked inside trader
        self.trader.add_strategies(strategies)

    def run(
        self,
        start: Union[datetime, str, int]=None,
        end: Union[datetime, str, int]=None,
    ) -> None:
        """
        Run a backtest.

        At the end of the run the trader and strategies will be stopped, then
        post-run analysis performed.

        Parameters
        ----------
        start : Union[datetime, str, int], optional
            The start datetime (UTC) for the backtest run. If ``None`` engine runs
            from the start of the data.
        end : Union[datetime, str, int], optional
            The end datetime (UTC) for the backtest run. If ``None`` engine runs
            to the end of the data.

        Raises
        ------
        ValueError
            If no data has been added to the engine.
        ValueError
            If the `start` is >= the `end` datetime.

        """
        self._run(start, end)
        self._end()

    def run_streaming(
        self,
        start: Union[datetime, str, int]=None,
        end: Union[datetime, str, int]=None,
    ):
        """
        Run a backtest in streaming mode.

        If more data than can fit in memory is to be run through the backtest
        engine, then streaming mode can be utilized. The expected sequence is as
        follows:
         - Add initial data block and strategies.
         - Call `run_streaming()`.
         - Call `clear_data()`.
         - Add next block of data stream.
         - Call `run_streaming()`.
         - Call `end_streaming()` when there is no more data to run on.

        Parameters
        ----------
        start : Union[datetime, str, int], optional
            The start datetime (UTC) for the current block of data. If ``None``
            engine runs from the start of the data.
        end : Union[datetime, str, int], optional
            The end datetime (UTC) for the current block of data. If ``None`` engine runs
            to the end of the data.

        Raises
        ------
        ValueError
            If no data has been added to the engine.
        ValueError
            If the `start` is >= the `end` datetime.

        """
        self._run(start, end)

    def end_streaming(self):
        """
        End the backtest streaming run.

        The following sequence of events will occur:
         - The trader will be stopped which in turn stops the strategies.
         - The exchanges will process all pending messages.
         - Post-run analysis is performed.

        """
        self._end()

    def _run(
        self,
        start: Union[datetime, str, int]=None,
        end: Union[datetime, str, int]=None,
    ):
        cdef int64_t start_ns
        cdef int64_t end_ns
        # Time range check and set
        if start is None:
            # Set `start` to start of data
            start_ns = self._data[0].ts_init
            start = pd.Timestamp(start_ns, tz=pytz.utc)
        else:
            start = pd.to_datetime(start, utc=True)
            start_ns = int(start.to_datetime64())
        if end is None:
            # Set `end` to end of data
            end_ns = self._data[-1].ts_init
            end = pd.Timestamp(end_ns, tz=pytz.utc)
        else:
            end = pd.to_datetime(end, utc=True)
            end_ns = int(end.to_datetime64())
        Condition.true(start_ns < end_ns, "start was >= end")
        Condition.not_empty(self._data, "data")

        # Set clocks
        self._test_clock.set_time(start_ns)
        for strategy in self.trader.strategies_c():
            strategy.clock.set_time(start_ns)

        if self.iteration == 0:
            # Initialize engine
            self._run_started = self._clock.utc_now()
            self._backtest_start = start
            for exchange in self._exchanges.values():
                exchange.initialize_account()
            self._data_engine.start()
            self._exec_engine.start()
            self.trader.start()
            self._test_logger.change_clock_c(self._test_clock)
            self._pre_run()

        self._log.info("=================================================================")
        self._log.info(" BACKTEST RUN")
        self._log.info("=================================================================")
        self._log.info(f"Run started:    {self._run_started}")
        self._log.info(f"Backtest start: {self._backtest_start}")
        self._log.info(f"Batch start:    {start}.")
        self._log.info(f"Batch end:      {end}.")
        self._log.info("-----------------------------------------------------------------")

        # Set data stream length
        self._data_len = len(self._data)

        # Set starting index
        cdef int i
        for i in range(self._data_len):
            if start_ns <= self._data[i].ts_init:
                self._index = i
                break

        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        cdef Data data = self._next()
        while data is not None:
            if data.ts_init > end_ns:
                break
            self._advance_time(data.ts_init)
            self._data_engine.process(data)
            if isinstance(data, OrderBookData):
                self._exchanges[data.instrument_id.venue].process_order_book(data)
            elif isinstance(data, Tick):
                self._exchanges[data.instrument_id.venue].process_tick(data)
            for exchange in self._exchanges.values():
                exchange.process(data.ts_init)
            self.iteration += 1
            data = self._next()
        # ---------------------------------------------------------------------#
        # Process remaining messages
        for exchange in self._exchanges.values():
            exchange.process(self._test_clock.timestamp_ns())
        # ---------------------------------------------------------------------#

    def _end(self):
        self.trader.stop()
        # Process remaining messages
        for exchange in self._exchanges.values():
            exchange.process(self._test_clock.timestamp_ns())

        self._post_run()

    cdef Data _next(self):
        cdef int64_t cursor = self._index
        self._index += 1
        if cursor < self._data_len:
            return self._data[cursor]

    cdef void _advance_time(self, int64_t now_ns) except *:
        cdef TradingStrategy strategy
        cdef TimeEventHandler event_handler
        cdef list time_events = []  # type: list[TimeEventHandler]
        for strategy in self.trader.strategies_c():
            time_events += strategy.clock.advance_time(now_ns)
        for event_handler in sorted(time_events):
            self._test_clock.set_time(event_handler.event.ts_event)
            event_handler.handle()
        self._test_clock.set_time(now_ns)

    def _pre_run(self):
        log_memory(self._log)

        for exchange in self._exchanges.values():
            account = exchange.exec_client.get_account()
            self._log.info("=================================================================")
            self._log.info(f"SimulatedVenue {exchange.id}")
            self._log.info("=================================================================")
            self._log.info(f"{repr(account)}")
            self._log.info("-----------------------------------------------------------------")
            self._log.info(f"Balances starting:")
            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                for b in account.starting_balances().values():
                    self._log.info(b.to_str())

    def _post_run(self):
        run_finished = self._clock.utc_now()
        self._log.info("=================================================================")
        self._log.info(" BACKTEST POST-RUN")
        self._log.info("=================================================================")
        self._log.info(f"Run started:    {self._run_started}")
        self._log.info(f"Run finished:   {run_finished}")
        self._log.info(f"Backtest start: {self._backtest_start}")
        self._log.info(f"Backtest end:   {self._test_clock.utc_now()}")
        self._log.info(f"Elapsed time:   {run_finished - self._run_started}")
        self._log.info(f"Iterations: {self.iteration:,}")
        self._log.info(f"Total events: {self._exec_engine.event_count:,}")
        self._log.info(f"Total orders: {self.cache.orders_total_count():,}")
        self._log.info(f"Total positions: {self.cache.positions_total_count():,}")

        if not self._config.run_analysis:
            return

        for exchange in self._exchanges.values():
            account = exchange.exec_client.get_account()
            self._log.info("=================================================================")
            self._log.info(f"SimulatedVenue {exchange.id}")
            self._log.info("=================================================================")
            self._log.info(f"{repr(account)}")
            self._log.info("-----------------------------------------------------------------")
            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                if account is None:
                    continue
                self._log.info(f"Balances starting:")
                for b in account.starting_balances().values():
                    self._log.info(b.to_str())
                self._log.info("-----------------------------------------------------------------")
                self._log.info(f"Balances ending:")
                for b in account.balances_total().values():
                    self._log.info(b.to_str())
                self._log.info("-----------------------------------------------------------------")
                self._log.info(f"Commissions:")
                for b in account.commissions().values():
                    self._log.info(b.to_str())
                self._log.info("-----------------------------------------------------------------")
                self._log.info(f"Unrealized PnLs:")
                unrealized_pnls = self.portfolio.unrealized_pnls(Venue(exchange.id.value)).values()
                if not unrealized_pnls:
                    self._log.info("None")
                else:
                    for b in self.portfolio.unrealized_pnls(Venue(exchange.id.value)).values():
                        self._log.info(b.to_str())

            # Log output diagnostics for all simulation modules
            for module in exchange.modules:
                module.log_diagnostics(self._log)

            self._log.info("=================================================================")
            self._log.info(" PERFORMANCE STATISTICS")
            self._log.info("=================================================================")

            # Find all positions for exchange venue
            positions = []
            for position in self.cache.positions():
                if position.instrument_id.venue == exchange.id:
                    positions.append(position)

            # Calculate statistics
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
            self._log.info("-----------------------------------------------------------------")

    def _add_data_client_if_not_exists(self, ClientId client_id) -> None:
        if client_id not in self._data_engine.registered_clients():
            client = BacktestDataClient(
                client_id=client_id,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._test_clock,
                logger=self._test_logger,
            )
            self._data_engine.register_client(client)

    def _add_market_data_client_if_not_exists(self, Venue venue) -> None:
        # TODO(cs): Assumption that client_id = venue
        cdef ClientId client_id = ClientId(venue.value)
        if client_id not in self._data_engine.registered_clients():
            client = BacktestMarketDataClient(
                client_id=client_id,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._test_clock,
                logger=self._test_logger,
            )
            self._data_engine.register_client(client)
