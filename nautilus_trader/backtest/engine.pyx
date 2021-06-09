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

import pandas as pd
import pytz

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.backtest.data_client cimport BacktestDataClient
from nautilus_trader.backtest.data_client cimport BacktestMarketDataClient
from nautilus_trader.backtest.data_producer cimport BacktestDataProducer
from nautilus_trader.backtest.data_producer cimport CachedProducer
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.cache.database cimport BypassCacheDatabase
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
from nautilus_trader.core.functions cimport pad_string
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.infrastructure.cache cimport RedisCacheDatabase
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.c_enums.venue_type cimport VenueType
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.data cimport GenericData
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.orderbook.book cimport OrderBookData
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer
from nautilus_trader.serialization.serializers cimport MsgPackEventSerializer
from nautilus_trader.serialization.serializers cimport MsgPackInstrumentSerializer
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies over historical
    data.
    """

    def __init__(
        self,
        TraderId trader_id=None,
        dict config_cache=None,
        dict config_data=None,
        dict config_risk=None,
        dict config_exec=None,
        str cache_db_type not None="in-memory",
        bint cache_db_flush=True,
        bint use_data_cache=False,
        bint bypass_logging=False,
        int level_stdout=LogLevel.INFO,
    ):
        """
        Initialize a new instance of the ``BacktestEngine`` class.

        Parameters
        ----------
        trader_id : TraderId, optional
            The trader identifier.
        config_data : dict[str, object]
            The configuration for the cache.
        config_data : dict[str, object]
            The configuration for the data engine.
        config_risk : dict[str, object]
            The configuration for the risk engine.
        config_exec : dict[str, object]
            The configuration for the execution engine.
        cache_db_type : str, optional
            The type for the cache (can be the default 'in-memory' or redis).
        cache_db_flush : bool, optional
            If the cache should be flushed on each run.
        use_data_cache : bool, optional
            If use cache for DataProducer (increased performance with repeated backtests on same data).
        bypass_logging : bool, optional
            If logging should be bypassed.
        level_stdout : int, optional
            The minimum log level for logging messages to stdout.

        """
        if trader_id is None:
            trader_id = TraderId("BACKTESTER-000")
        Condition.valid_string(cache_db_type, "cache_db_type")

        # Options
        self._cache_db_flush = cache_db_flush
        self._use_data_cache = use_data_cache

        # Data
        self._generic_data = []     # type: list[GenericData]
        self._order_book_data = []  # type: list[OrderBookData]
        self._quote_ticks = {}      # type: dict[InstrumentId, pd.DataFrame]
        self._trade_ticks = {}      # type: dict[InstrumentId, pd.DataFrame]
        self._bars_bid = {}         # type: dict[InstrumentId, dict[BarAggregation, pd.DataFrame]]
        self._bars_ask = {}         # type: dict[InstrumentId, dict[BarAggregation, pd.DataFrame]]

        # Setup components
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

        if cache_db_type == "in-memory":
            cache_db = BypassCacheDatabase(
                trader_id=trader_id,
                logger=self._logger)
        elif cache_db_type == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=trader_id,
                logger=self._test_logger,
                instrument_serializer=MsgPackInstrumentSerializer(),
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                config={"host": "localhost", "port": 6379},
            )
        else:
            raise ValueError(f"The exec_db_type in the backtest configuration is unrecognized, "
                             f"can be either \"in-memory\" or \"redis\"")

        if self._cache_db_flush:
            cache_db.flush()

        cache = Cache(
            database=cache_db,
            logger=self._test_logger,
            config=config_cache,
        )

        self._test_clock.set_time(self._clock.timestamp_ns())  # For logging consistency

        self.portfolio = Portfolio(
            cache=cache,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._data_producer = None  # Instantiated on first run

        if config_data is None:
            config_data = {}
        config_data["use_previous_close"] = False  # Ensures bars match historical data
        self._data_engine = DataEngine(
            portfolio=self.portfolio,
            cache=cache,
            clock=self._test_clock,
            logger=self._test_logger,
            config=config_data,
        )

        self._exec_engine = ExecutionEngine(
            portfolio=self.portfolio,
            cache=cache,
            clock=self._test_clock,
            logger=self._test_logger,
            config=config_exec,
        )

        self._risk_engine = RiskEngine(
            exec_engine=self._exec_engine,
            portfolio=self.portfolio,
            cache=cache,
            clock=self._test_clock,
            logger=self._test_logger,
            config=config_risk,
        )

        # Wire up components
        self._exec_engine.register_risk_engine(self._risk_engine)
        self._exec_engine.load_cache()

        self.trader = Trader(
            trader_id=trader_id,
            strategies=[],  # Added in `run()`
            portfolio=self.portfolio,
            data_engine=self._data_engine,
            risk_engine=self._risk_engine,
            exec_engine=self._exec_engine,
            clock=self._test_clock,
            logger=self._test_logger,
            warn_no_strategies=False,
        )

        self.analyzer = PerformanceAnalyzer()

        self._exchanges = {}

        self._test_clock.set_time(self._clock.timestamp_ns())  # For logging consistency

        self.iteration = 0

        self.time_to_initialize = self._clock.delta(self.created_time)
        self._log.info(f"Initialized in {self.time_to_initialize.total_seconds():.3f}s.")

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
            The data client identifier to associate with the generic data.
        data : list[GenericData]
            The data to add.

        Raises
        ------
        ValueError
            If data is empty.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(data, "data")
        Condition.not_empty(data, "data")
        Condition.list_type(data, GenericData, "data")

        # Check client has been registered
        self._add_data_client_if_not_exists(client_id)

        # Add data
        self._generic_data = sorted(
            self._generic_data + data,
            key=lambda x: x.ts_recv_ns,
        )

        self._log.info(f"Added {len(data)} GenericData points.")

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
            If data is empty.
        ValueError
            If instrument_id is not contained in the data cache.

        """
        Condition.not_none(data, "data")
        Condition.not_empty(data, "data")
        Condition.list_type(data, OrderBookData, "data")
        cdef InstrumentId instrument_id = data[0].instrument_id
        Condition.true(
            instrument_id in self._data_engine.cache.instrument_ids(),
            "Instrument for given data not found in the data cache. "
            "Please call `add_instrument()` before adding related data.",
        )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument_id.venue)

        # Add data
        self._order_book_data = sorted(
            self._order_book_data + data,
            key=lambda x: x.ts_recv_ns,
        )

        self._log.info(f"Added {len(data)} {instrument_id} OrderBookData elements.")

    def add_quote_ticks(self, InstrumentId instrument_id, data: pd.DataFrame) -> None:
        """
        Add the quote tick data to the backtest engine.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'bid', 'ask' data columns.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the quote tick data.
        data : pd.DataFrame
            The quote tick data to add.

        Raises
        ------
        ValueError
            If data is empty.
        ValueError
            If instrument_id is not contained in the data cache.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")
        Condition.false(data.empty, "data was empty")
        Condition.true(
            instrument_id in self._data_engine.cache.instrument_ids(),
            "Instrument for given data not found in the data cache. "
            "Please call `add_instrument()` before adding related data.",
        )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument_id.venue)

        # Add data
        self._quote_ticks[instrument_id] = data
        self._quote_ticks = dict(sorted(self._quote_ticks.items()))

        self._log.info(f"Added {len(data)} {instrument_id} QuoteTick data elements.")

    def add_trade_ticks(self, InstrumentId instrument_id, data: pd.DataFrame) -> None:
        """
        Add the trade tick data to the backtest engine.

        The format of the dataframe is expected to be a DateTimeIndex (times are
        assumed to be UTC, and are converted to tz-aware in pre-processing).

        With index column named 'timestamp', and 'trade_id', 'price', 'quantity',
        'buyer_maker' data columns.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the trade tick data.
        data : pd.DataFrame
            The trade tick data to add.

        Raises
        ------
        ValueError
            If data is empty.
        ValueError
            If instrument_id is not contained in the data cache.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(data, "data")
        Condition.type(data, pd.DataFrame, "data")
        Condition.false(data.empty, "data was empty")
        Condition.true(
            instrument_id in self._data_engine.cache.instrument_ids(),
            "Instrument for given data not found in the data cache. "
            "Please call `add_instrument()` before adding related data.",
        )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument_id.venue)

        # Add data
        self._trade_ticks[instrument_id] = data
        self._trade_ticks = dict(sorted(self._trade_ticks.items()))

        self._log.info(f"Added {len(data)} {instrument_id} TradeTick data elements.")

    def add_trade_tick_objects(self, InstrumentId instrument_id, list data) -> None:
        """
        Add the built trade tick data to the backtest engine.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the trade tick data.
        data : list[TradeTick]
            The trade tick data to add.

        Raises
        ------
        ValueError
            If data is empty.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(data, "data")
        Condition.not_empty(data, "data")
        Condition.list_type(data, TradeTick, "data")

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument_id.venue)

        # Add data
        self._trade_ticks[instrument_id] = data
        self._trade_ticks = dict(sorted(self._trade_ticks.items()))

        self._log.info(f"Added {len(data)} {instrument_id} TradeTick data elements.")

    def add_bars(
        self,
        InstrumentId instrument_id,
        BarAggregation aggregation,
        PriceType price_type,
        data: pd.DataFrame
    ) -> None:
        """
        Add the bar data to the backtest engine.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the bar data.
        aggregation : BarAggregation
            The bar aggregation of the data.
        price_type : PriceType
            The price type of the data.
        data : pd.DataFrame
            The bar data to add.

        Raises
        ------
        ValueError
            If price_type is LAST.
        ValueError
            If data is empty.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(data, "data")
        Condition.true(price_type != PriceType.LAST, "price_type was PriceType.LAST")
        Condition.false(data.empty, "data was empty")
        Condition.true(
            instrument_id in self._data_engine.cache.instrument_ids(),
            "Instrument for given data not found in the data cache. "
            "Please call `add_instrument()` before adding related data.",
        )

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument_id.venue)

        # Add data
        if price_type == PriceType.BID:
            if instrument_id not in self._bars_bid:
                self._bars_bid[instrument_id] = {}
                self._bars_bid = dict(sorted(self._bars_bid.items()))
            self._bars_bid[instrument_id][aggregation] = data
            self._bars_bid[instrument_id] = dict(sorted(self._bars_bid[instrument_id].items()))
        elif price_type == PriceType.ASK:
            if instrument_id not in self._bars_ask:
                self._bars_ask[instrument_id] = {}
                self._bars_ask = dict(sorted(self._bars_ask.items()))
            self._bars_ask[instrument_id][aggregation] = data
            self._bars_ask[instrument_id] = dict(sorted(self._bars_ask[instrument_id].items()))

        cdef dict shapes = {}  # type: dict[BarAggregation, tuple]
        cdef dict indices = {}  # type: dict[BarAggregation, DatetimeIndex]
        for instrument_id, data in self._bars_bid.items():
            for aggregation, dataframe in data.items():
                if aggregation not in shapes:
                    shapes[aggregation] = dataframe.shape
                if aggregation not in indices:
                    indices[aggregation] = dataframe.index
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal")
                if not all(dataframe.index == indices[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal")
        for instrument_id, data in self._bars_ask.items():
            for aggregation, dataframe in data.items():
                if dataframe.shape != shapes[aggregation]:
                    raise RuntimeError(f"{dataframe} bid ask shape is not equal")
                if not all(dataframe.index == indices[aggregation]):
                    raise RuntimeError(f"{dataframe} bid ask index is not equal")

        self._log.info(
            f"Added {len(data)} {instrument_id} "
            f"{BarAggregationParser.to_str(aggregation)}-{PriceTypeParser.to_str(price_type)} "
            f"Bar data elements."
        )

    def add_venue(
        self,
        Venue venue,
        VenueType venue_type,
        OMSType oms_type,
        AccountType account_type,
        Currency base_currency,
        list starting_balances,
        bint is_frozen_account=False,
        list modules=None,
        FillModel fill_model=None,
        OrderBookLevel order_book_level=OrderBookLevel.L1
    ) -> None:
        """
        Add a `SimulatedExchange` with the given parameters to the backtest engine.

        Parameters
        ----------
        venue : Venue
            The exchange venue identifier.
        venue_type : VenueType
            The type of venue (will determine venue -> client_id mapping).
        oms_type : OMSType
            The order management system type for the exchange. If HEDGING and
            no position_id for an order then will generate a new position_id.
        account_type : AccountType
            The account type for the client.
        base_currency : Currency, optional
            The account base currency for the client. Use ``None`` for multi-currency accounts.
        starting_balances : list[Money]
            The starting account balances (specify one for a single asset account).
        is_frozen_account : bool
            If the account for this exchange is frozen (balances will not change).
        modules : list[SimulationModule, optional
            The simulation modules to load into the exchange.
        fill_model : FillModel, optional
            The fill model for the exchange (if None then no probabilistic fills).
        order_book_level : OrderBookLevel
            The default order book level for fill modelling.

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

        # Create exchange
        exchange = SimulatedExchange(
            venue=venue,
            venue_type=venue_type,
            oms_type=oms_type,
            account_type=account_type,
            base_currency=base_currency,
            starting_balances=starting_balances,
            is_frozen_account=is_frozen_account,
            instruments=self._data_engine.cache.instruments(venue),
            modules=modules,
            cache=self._exec_engine.cache,
            fill_model=fill_model,
            exchange_order_book_level=order_book_level,
            clock=self._test_clock,
            logger=self._test_logger,
        )

        self._exchanges[venue] = exchange

        # Create execution client for exchange
        exec_client = BacktestExecClient(
            exchange=exchange,
            account_id=AccountId(venue.value, "001"),
            account_type=account_type,
            base_currency=base_currency,
            engine=self._exec_engine,
            clock=self._test_clock,
            logger=self._test_logger,
            is_frozen_account=is_frozen_account,
        )

        exchange.register_client(exec_client)
        self._exec_engine.register_client(exec_client)

        self._log.info(f"Added {venue} SimulatedExchange.")

    def reset(self) -> None:
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
        if self._cache_db_flush:
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

    def dispose(self) -> None:
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

    def run(
        self,
        datetime start=None,
        datetime stop=None,
        list strategies=None,
    ) -> None:
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
        if self._data_producer is None:
            self._data_producer = BacktestDataProducer(
                logger=self._test_logger,
                instruments=self._data_engine.cache.instruments(),
                generic_data=self._generic_data,
                order_book_data=self._order_book_data,
                quote_ticks=self._quote_ticks,
                trade_ticks=self._trade_ticks,
                bars_bid=self._bars_bid,
                bars_ask=self._bars_ask,
            )

            if self._use_data_cache:
                self._data_producer = CachedProducer(self._data_producer)

        log_memory(self._log)

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
            self._exec_engine.cache.add_instrument(instrument)

        # Setup new strategies
        if strategies:
            self.trader.initialize_strategies(strategies, warn_no_strategies=False)

        # Run the backtest
        self._log.info(f"Running backtest...")

        for strategy in self.trader.strategies_c():
            strategy.clock.set_time(start_ns)

        # Start main components
        self._data_engine.start()
        self._exec_engine.start()
        self.trader.start()

        cdef Data data
        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        while self._data_producer.has_data:
            data = self._data_producer.next()
            self._advance_time(data.ts_recv_ns)
            if isinstance(data, OrderBookData):
                self._exchanges[data.instrument_id.venue].process_order_book(data)
            elif isinstance(data, Tick):
                self._exchanges[data.instrument_id.venue].process_tick(data)
            self._data_engine.process(data)
            self._process_modules(data.ts_recv_ns)
            self.iteration += 1
        # ---------------------------------------------------------------------#

        self.trader.stop()

        self._log_footer(run_started, self._clock.utc_now(), start, stop)

    cdef void _advance_time(self, int64_t now_ns) except *:
        cdef TradingStrategy strategy
        cdef TimeEventHandler event_handler
        cdef list time_events = []  # type: list[TimeEventHandler]
        for strategy in self.trader.strategies_c():
            time_events += strategy.clock.advance_time(now_ns)
        for event_handler in sorted(time_events):
            self._test_clock.set_time(event_handler.event.event_timestamp_ns)
            event_handler.handle()
        self._test_clock.set_time(now_ns)

    cdef void _process_modules(self, int64_t now_ns) except *:
        cdef SimulatedExchange exchange
        for exchange in self._exchanges.values():
            exchange.process_modules(now_ns)

    cdef void _log_header(
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

    cdef void _log_footer(
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
            account = exchange.exec_client.get_account()
            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                if account is None:
                    continue
                account_balances_starting = ', '.join([b.to_str() for b in account.starting_balances().values()])
                account_balances_ending = ', '.join([b.to_str() for b in account.balances_total().values()])
                account_commissions = ', '.join([b.to_str() for b in account.commissions().values()])
                unrealized_pnls = ', '.join([b.to_str() for b in self.portfolio.unrealized_pnls(Venue(exchange.id.value)).values()])
                account_starting_length = len(account_balances_starting)
                account_balances_ending = pad_string(account_balances_ending, account_starting_length)
                account_commissions = pad_string(account_commissions, account_starting_length)
                unrealized_pnls = pad_string(unrealized_pnls, account_starting_length)
                self._log.info(f"Account balances (starting): {account_balances_starting}")
                self._log.info(f"Account balances (ending):   {account_balances_ending}")
                self._log.info(f"Commissions (total):         {account_commissions}")
                self._log.info(f"Unrealized PnLs:             {unrealized_pnls}")

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

    def _add_data_client_if_not_exists(self, ClientId client_id) -> None:
        if client_id not in self._data_engine.registered_clients:
            client = BacktestDataClient(
                client_id=client_id,
                engine=self._data_engine,
                clock=self._test_clock,
                logger=self._test_logger,
            )
            self._data_engine.register_client(client)

    def _add_market_data_client_if_not_exists(self, Venue venue) -> None:
        # TODO(cs): Assumption that client_id = venue
        cdef ClientId client_id = ClientId(venue.value)
        if client_id not in self._data_engine.registered_clients:
            client = BacktestMarketDataClient(
                client_id=client_id,
                engine=self._data_engine,
                clock=self._test_clock,
                logger=self._test_logger,
            )
            self._data_engine.register_client(client)
