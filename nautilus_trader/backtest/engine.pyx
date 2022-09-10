# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import pickle
from decimal import Decimal
from typing import Dict, List, Optional, Union

import pandas as pd

from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.common import Environment
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config.error import InvalidConfiguration

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.backtest.data_client cimport BacktestDataClient
from nautilus_trader.backtest.data_client cimport BacktestMarketDataClient
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.models cimport LatencyModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport LogLevelParser
from nautilus_trader.common.logging cimport log_memory
from nautilus_trader.common.timer cimport TimeEventHandler
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.datetime cimport maybe_dt_to_unix_nanos
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.aggregation_source cimport AggregationSource
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.base cimport GenericData
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.portfolio.base cimport PortfolioFacade
from nautilus_trader.system.kernel cimport NautilusKernel
from nautilus_trader.trading.strategy cimport Strategy
from nautilus_trader.trading.trader cimport Trader


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

    def __init__(self, config: Optional[BacktestEngineConfig] = None):
        if config is None:
            config = BacktestEngineConfig()
        Condition.type(config, BacktestEngineConfig, "config")

        self._config: BacktestEngineConfig  = config

        # Setup components
        self._clock: Clock = LiveClock()  # Real-time for the engine

        # Run IDs
        self.run_config_id: Optional[str] = None
        self.run_id: Optional[UUID4] = None
        self.iteration: int = 0

        # Venues and data
        self._venues: Dict[Venue, SimulatedExchange] = {}
        self._data: List[Data] = []
        self._data_len: int = 0
        self._index: int = 0

        # Timing
        self.run_started: Optional[datetime] = None
        self.run_finished: Optional[datetime] = None
        self.backtest_start: Optional[datetime] = None
        self.backtest_end: Optional[datetime] = None

        # Build core system kernel
        self.kernel = NautilusKernel(
            environment=Environment.BACKTEST,
            name=type(self).__name__,
            trader_id=TraderId(config.trader_id),
            cache_config=config.cache or CacheConfig(),
            cache_database_config=CacheDatabaseConfig(type="in-memory", flush=True),
            data_config=config.data_engine or DataEngineConfig(),
            risk_config=config.risk_engine or RiskEngineConfig(),
            exec_config=config.exec_engine or ExecEngineConfig(),
            streaming_config=config.streaming,
            actor_configs=config.actors,
            strategy_configs=config.strategies,
            log_level=LogLevelParser.from_str(config.log_level.upper()),
            bypass_logging=config.bypass_logging,
        )

        # Setup engine logging
        self._logger = Logger(
            clock=LiveClock(),
            trader_id=self.kernel.trader_id,
            machine_id=self.kernel.machine_id,
            instance_id=self.kernel.instance_id,
            bypass=config.bypass_logging,
        )

        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=self._logger,
        )

    @property
    def trader_id(self) -> TraderId:
        """
        Return the nodes trader ID.

        Returns
        -------
        TraderId

        """
        return self.kernel.trader_id

    @property
    def machine_id(self) -> str:
        """
        Return the nodes machine ID.

        Returns
        -------
        str

        """
        return self.kernel.machine_id

    @property
    def instance_id(self) -> UUID4:
        """
        Return the nodes instance ID.

        Returns
        -------
        UUID4

        """
        return self.kernel.instance_id

    @property
    def trader(self) -> Trader:
        """
        Return the engines internal trader.

        Returns
        -------
        Trader

        """
        return self.kernel.trader

    @property
    def cache(self) -> CacheFacade:
        """
        Return the engines internal read-only cache.

        Returns
        -------
        CacheFacade

        """
        return self.kernel.cache

    @property
    def data(self) -> List[Data]:
        """
        Return the engines internal data stream.
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
        return self.kernel.portfolio

    def list_venues(self):
        """
        Return the venues contained within the engine.

        Returns
        -------
        list[Venue]

        """
        return list(self._venues)

    def add_venue(
        self,
        Venue venue,
        OMSType oms_type,
        AccountType account_type,
        Currency base_currency,
        list starting_balances,
        default_leverage = None,
        dict leverages = None,
        list modules = None,
        FillModel fill_model = None,
        LatencyModel latency_model = None,
        BookType book_type = BookType.L1_TBBO,
        bint routing: bool = False,
        bint frozen_account = False,
        bint reject_stop_orders: bool = True,
    ) -> None:
        """
        Add a `SimulatedExchange` with the given parameters to the backtest engine.

        Parameters
        ----------
        venue : Venue
            The venue ID.
        oms_type : OMSType {``HEDGING``, ``NETTING``}
            The order management system type for the exchange. If ``HEDGING`` will
            generate new position IDs.
        account_type : AccountType
            The account type for the client.
        base_currency : Currency, optional
            The account base currency for the client. Use ``None`` for multi-currency accounts.
        starting_balances : list[Money]
            The starting account balances (specify one for a single asset account).
        default_leverage : Decimal, optional
            The account default leverage (for margin accounts).
        leverages : Dict[InstrumentId, Decimal]
            The instrument specific leverage configuration (for margin accounts).
        modules : list[SimulationModule, optional
            The simulation modules to load into the exchange.
        fill_model : FillModel, optional
            The fill model for the exchange.
        latency_model : LatencyModel, optional
            The latency model for the exchange.
        book_type : BookType, default ``BookType.L1_TBBO``
            The default order book type for fill modelling.
        routing : bool, default False
            If multi-venue routing should be enabled for the execution client.
        frozen_account : bool, default False
            If the account for this exchange is frozen (balances will not change).
        reject_stop_orders : bool, default True
            If stop orders are rejected on submission if trigger price is in the market.

        Raises
        ------
        ValueError
            If `venue` is already registered with the engine.

        """
        if modules is None:
            modules = []
        if fill_model is None:
            fill_model = FillModel()
        Condition.not_none(venue, "venue")
        Condition.not_in(venue, self._venues, "venue", "_venues")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules")
        Condition.type_or_none(fill_model, FillModel, "fill_model")

        # Create exchange
        exchange = SimulatedExchange(
            venue=venue,
            oms_type=oms_type,
            account_type=account_type,
            base_currency=base_currency,
            starting_balances=starting_balances,
            default_leverage=default_leverage or Decimal(10),
            leverages=leverages or {},
            instruments=[],
            modules=modules,
            cache=self.kernel.cache,
            fill_model=fill_model,
            latency_model=latency_model,
            book_type=book_type,
            clock=self.kernel.clock,
            logger=self.kernel.logger,
            frozen_account=frozen_account,
            reject_stop_orders=reject_stop_orders,
        )

        self._venues[venue] = exchange

        # Create execution client for exchange
        exec_client = BacktestExecClient(
            exchange=exchange,
            msgbus=self.kernel.msgbus,
            cache=self.kernel.cache,
            clock=self.kernel.clock,
            logger=self.kernel.logger,
            routing=routing,
            frozen_account=frozen_account,
        )

        exchange.register_client(exec_client)
        self.kernel.exec_engine.register_client(exec_client)

        self._log.info(f"Added {exchange}.")

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
                f"Please add the {instrument.id.venue} venue using the `add_venue` method."
            )

        # Validate the instrument is correct for the venue
        account_type = self._venues[instrument.id.venue]

        # Check client has been registered
        self._add_market_data_client_if_not_exists(instrument.id.venue)

        # Add data
        self.kernel.data_engine.process(instrument)  # Adds to cache
        self._venues[instrument.id.venue].add_instrument(instrument)

        self._log.info(f"Added {instrument.id} Instrument.")

    def add_data(self, list data, ClientId client_id = None) -> None:
        """
        Add the given data to the backtest engine.

        Parameters
        ----------
        data : list[Data]
            The data to add.
        client_id : ClientId, optional
            The data client ID to associate with generic data.

        Raises
        ------
        ValueError
            If `data` is empty.
        ValueError
            If `instrument_id` for the data is not found in the cache.
        ValueError
            If `data` elements do not have an `instrument_id` and `client_id` is ``None``.

        Warnings
        --------
        Assumes all data elements are of the same type. Adding lists of varying
        data types could result in incorrect backtest logic.

        """
        Condition.not_empty(data, "data")

        first = data[0]

        cdef str data_prepend_str = ""
        if hasattr(first, "instrument_id"):
            Condition.true(
                first.instrument_id in self.kernel.cache.instrument_ids(),
                f"`Instrument` {first.instrument_id} for the given data not found in the cache. "
                "Please add the instrument through `add_instrument()` prior to adding related data.",
            )
            # Check client has been registered
            self._add_market_data_client_if_not_exists(first.instrument_id.venue)
            data_prepend_str = f"{first.instrument_id} "
        elif isinstance(first, Bar):
            Condition.true(
                first.type.instrument_id in self.kernel.cache.instrument_ids(),
                f"`Instrument` {first.type.instrument_id} for the given data not found in the cache. "
                "Please add the instrument through `add_instrument()` prior to adding related data.",
            )
            Condition.equal(
                first.type.aggregation_source,
                AggregationSource.EXTERNAL,
                "bar_type.aggregation_source",
                "required source",
            )
            data_prepend_str = f"{first.type} "
        else:
            Condition.not_none(client_id, "client_id")
            # Check client has been registered
            self._add_data_client_if_not_exists(client_id)
            if isinstance(first, GenericData):
                data_prepend_str = f"{type(data[0].data).__name__} "

        # Add data
        self._data = sorted(self._data + data, key=lambda x: x.ts_init)

        self._log.info(
            f"Added {len(data):,} {data_prepend_str}"
            f"{type(first).__name__} element{'' if len(data) == 1 else 's'}.",
        )

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
            f"Loaded {len(self._data):,} data "
            f"element{'' if len(data) == 1 else 's'} from pickle.",
        )

    def add_actor(self, actor: Actor) -> None:
        # Checked inside trader
        self.kernel.trader.add_actor(actor)

    def add_actors(self, actors: List[Actor]) -> None:
        # Checked inside trader
        self.kernel.trader.add_actors(actors)

    def add_strategy(self, strategy: Strategy) -> None:
        # Checked inside trader
        self.kernel.trader.add_strategy(strategy)

    def add_strategies(self, strategies: List[Strategy]) -> None:
        # Checked inside trader
        self.kernel.trader.add_strategies(strategies)

    def reset(self) -> None:
        """
        Reset the backtest engine.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        if self.kernel.trader.is_running_c():
            # End current backtest run
            self._end()

        # Change logger clock back to live clock for consistent time stamping
        self.kernel.logger.change_clock_c(self._clock)

        # Reset DataEngine
        if self.kernel.data_engine.is_running_c():
            self.kernel.data_engine.stop()
        self.kernel.data_engine.reset()

        # Reset ExecEngine
        if self.kernel.exec_engine.is_running_c():
            self.kernel.exec_engine.stop()
        if self._config.cache_database is not None and self._config.cache_database.flush:
            self.kernel.exec_engine.flush_db()
        self.kernel.exec_engine.reset()

        # Reset RiskEngine
        if self.kernel.risk_engine.is_running_c():
            self.kernel.risk_engine.stop()
        self.kernel.risk_engine.reset()

        self.kernel.trader.reset()

        for exchange in self._venues.values():
            exchange.reset()

        # Reset run IDs
        self.run_config_id = None
        self.run_id = None

        # Reset timing
        self.iteration = 0
        self.run_started = None
        self.run_finished = None
        self.backtest_start = None
        self.backtest_end = None

        self._log.info("Reset.")

    def clear_data(self):
        """
        Clear the engines internal data stream.

        Does not clear added instruments.

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
        self.kernel.trader.dispose()

        if self.kernel.data_engine.is_running_c():
            self.kernel.data_engine.stop()
        if self.kernel.exec_engine.is_running_c():
            self.kernel.exec_engine.stop()
        if self.kernel.risk_engine.is_running_c():
            self.kernel.risk_engine.stop()

        self.kernel.data_engine.dispose()
        self.kernel.exec_engine.dispose()
        self.kernel.risk_engine.dispose()

        if self.kernel.writer is not None:
            self.kernel.writer.close()

    def run(
        self,
        start: Optional[Union[datetime, str, int]] = None,
        end: Optional[Union[datetime, str, int]] = None,
        run_config_id: Optional[str] = None,
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
        run_config_id : str, optional
            The tokenized `BacktestRunConfig` ID.

        Raises
        ------
        ValueError
            If no data has been added to the engine.
        ValueError
            If the `start` is >= the `end` datetime.

        """
        self._run(start, end, run_config_id)
        self._end()

    def run_streaming(
        self,
        start: Optional[Union[datetime, str, int]] = None,
        end: Optional[Union[datetime, str, int]] = None,
        run_config_id: Optional[str] = None,
    ):
        """
        Run a backtest in streaming mode.

        If more data than can fit in memory is to be run through the backtest
        engine, then streaming mode can be utilized. The expected sequence is as
        follows:
        - Add initial data batch and strategies.
        - Call `run_streaming()`.
        - Call `clear_data()`.
        - Add next batch of data stream.
        - Call `run_streaming()`.
        - Call `end_streaming()` when there is no more data to run on.

        Parameters
        ----------
        start : Union[datetime, str, int], optional
            The start datetime (UTC) for the current batch of data. If ``None``
            engine runs from the start of the data.
        end : Union[datetime, str, int], optional
            The end datetime (UTC) for the current batch of data. If ``None`` engine runs
            to the end of the data.
        run_config_id : str, optional
            The tokenized backtest run configuration ID.

        Raises
        ------
        ValueError
            If no data has been added to the engine.
        ValueError
            If the `start` is >= the `end` datetime.

        """
        self._run(start, end, run_config_id)

    def end_streaming(self):
        """
        End the backtest streaming run.

        The following sequence of events will occur:
         - The trader will be stopped which in turn stops the strategies.
         - The exchanges will process all pending messages.
         - Post-run analysis is performed.

        """
        self._end()

    def get_result(self):
        """
        Return the backtest result from the last run.

        Returns
        -------
        BacktestResult

        """
        stats_pnls: Dict[str, Dict[str, float]] = {}

        for currency in self.kernel.portfolio.analyzer.currencies:
            stats_pnls[currency.code] = self.kernel.portfolio.analyzer.get_performance_stats_pnls(currency)

        return BacktestResult(
            trader_id=self.kernel.trader_id.to_str(),
            machine_id=self.machine_id,
            run_config_id=self.run_config_id,
            instance_id=self.kernel.instance_id.to_str(),
            run_id=self.run_id.to_str() if self.run_id is not None else None,
            run_started=maybe_dt_to_unix_nanos(self.run_started),
            run_finished=maybe_dt_to_unix_nanos(self.run_finished),
            backtest_start=maybe_dt_to_unix_nanos(self.backtest_start),
            backtest_end=maybe_dt_to_unix_nanos(self.backtest_end),
            elapsed_time=(self.backtest_end - self.backtest_start).total_seconds(),
            iterations=self.iteration,
            total_events=self.kernel.exec_engine.event_count,
            total_orders=self.kernel.cache.orders_total_count(),
            total_positions=self.kernel.cache.positions_total_count(),
            stats_pnls=stats_pnls,
            stats_returns=self.kernel.portfolio.analyzer.get_performance_stats_returns(),
        )

    def _run(
        self,
        start: Optional[Union[datetime, str, int]] = None,
        end: Optional[Union[datetime, str, int]] = None,
        run_config_id: Optional[str] = None,
    ):
        cdef uint64_t start_ns
        cdef uint64_t end_ns
        # Time range check and set
        if start is None:
            # Set `start` to start of data
            start_ns = self._data[0].ts_init
            start = unix_nanos_to_dt(start_ns)
        else:
            start = pd.to_datetime(start, utc=True)
            start_ns = int(start.to_datetime64())
        if end is None:
            # Set `end` to end of data
            end_ns = self._data[-1].ts_init
            end = unix_nanos_to_dt(end_ns)
        else:
            end = pd.to_datetime(end, utc=True)
            end_ns = int(end.to_datetime64())
        Condition.true(start_ns < end_ns, "start was >= end")
        Condition.not_empty(self._data, "data")

        # Set clocks
        self.kernel.clock.set_time(start_ns)
        for actor in self.kernel.trader.actors_c():
            actor.clock.set_time(start_ns)
        for strategy in self.kernel.trader.strategies_c():
            strategy.clock.set_time(start_ns)

        cdef SimulatedExchange exchange
        if self.iteration == 0:
            # Initialize run
            self.run_config_id = run_config_id  # Can be None
            self.run_id = UUID4()
            self.run_started = self._clock.utc_now()
            self.backtest_start = start
            for exchange in self._venues.values():
                exchange.initialize_account()
            self.kernel.data_engine.start()
            self.kernel.exec_engine.start()
            self.kernel.trader.start()
            # Change logger clock for the run
            self.kernel.logger.change_clock_c(self.kernel.clock)
            self._log_pre_run()

        self._log_run(start, end)

        # Set data stream length
        self._data_len = len(self._data)

        # Set starting index
        cdef uint64_t i
        for i in range(self._data_len):
            if start_ns <= self._data[i].ts_init:
                self._index = i
                break

        # -- MAIN BACKTEST LOOP -----------------------------------------------#
        cdef list now_events
        cdef Data data = self._next()
        while data is not None:
            if data.ts_init > end_ns:
                break
            now_events = self._advance_time(data.ts_init)
            if isinstance(data, OrderBookData):
                self._venues[data.instrument_id.venue].process_order_book(data)
            elif isinstance(data, QuoteTick):
                self._venues[data.instrument_id.venue].process_quote_tick(data)
            elif isinstance(data, TradeTick):
                self._venues[data.instrument_id.venue].process_trade_tick(data)
            elif isinstance(data, Bar):
                self._venues[data.type.instrument_id.venue].process_bar(data)
            self.kernel.data_engine.process(data)
            for event_handler in now_events:
                event_handler.handle()
            for exchange in self._venues.values():
                exchange.process(data.ts_init)
            self.iteration += 1
            data = self._next()
        # ---------------------------------------------------------------------#
        # Process remaining messages
        for exchange in self._venues.values():
            exchange.process(self.kernel.clock.timestamp_ns())
        # ---------------------------------------------------------------------#

    def _end(self):
        self.kernel.trader.stop()
        # Process remaining messages
        for exchange in self._venues.values():
            exchange.process(self.kernel.clock.timestamp_ns())

        self.run_finished = self._clock.utc_now()
        self.backtest_end = self.kernel.clock.utc_now()

        self._log_post_run()

    cdef Data _next(self):
        cdef uint64_t cursor = self._index
        self._index += 1
        if cursor < self._data_len:
            return self._data[cursor]

    cdef list _advance_time(self, uint64_t now_ns):
        cdef list all_events = []  # type: list[TimeEventHandler]
        cdef list now_events = []  # type: list[TimeEventHandler]
        cdef:
            Actor actor
            Strategy strategy
        for actor in self.kernel.trader.actors_c():
            all_events += actor.clock.advance_time(now_ns)
        for strategy in self.kernel.trader.strategies_c():
            all_events += strategy.clock.advance_time(now_ns)

        all_events += self.kernel.clock.advance_time(now_ns)

        # Handle all events prior to the `now_ns`
        cdef TimeEventHandler event_handler
        for event_handler in sorted(all_events):
            if event_handler.event.ts_event == now_ns:
                now_events.append(event_handler)
                continue
            event_handler.handle()

        # Return the remaining events to be handled
        return now_events

    def _log_pre_run(self):
        log_memory(self._log)

        for exchange in self._venues.values():
            account = exchange.exec_client.get_account()
            self._log.info("\033[36m=================================================================")
            self._log.info(f"\033[36m SimulatedVenue {exchange.id}")
            self._log.info("\033[36m=================================================================")
            self._log.info(f"{repr(account)}")
            self._log.info("\033[36m-----------------------------------------------------------------")
            self._log.info(f"Balances starting:")
            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                for b in account.starting_balances().values():
                    self._log.info(b.to_str())

    def _log_run(self, start: pd.Timestamp, end: pd.Timestamp):
        self._log.info("\033[36m=================================================================")
        self._log.info("\033[36m BACKTEST RUN")
        self._log.info("\033[36m=================================================================")
        self._log.info(f"Run config ID:  {self.run_config_id}")
        self._log.info(f"Run ID:         {self.run_id}")
        self._log.info(f"Run started:    {self.run_started}")
        self._log.info(f"Backtest start: {self.backtest_start}")
        self._log.info(f"Batch start:    {start}")
        self._log.info(f"Batch end:      {end}")
        self._log.info("\033[36m-----------------------------------------------------------------")

    def _log_post_run(self):
        self._log.info("\033[36m=================================================================")
        self._log.info("\033[36m BACKTEST POST-RUN")
        self._log.info("\033[36m=================================================================")
        self._log.info(f"Run config ID:  {self.run_config_id}")
        self._log.info(f"Run ID:         {self.run_id}")
        self._log.info(f"Run started:    {self.run_started}")
        self._log.info(f"Run finished:   {self.run_finished}")
        self._log.info(f"Elapsed time:   {self.run_finished - self.run_started}")
        self._log.info(f"Backtest start: {self.backtest_start}")
        self._log.info(f"Backtest end:   {self.backtest_end}")
        self._log.info(f"Backtest range: {self.backtest_end - self.backtest_start}")
        self._log.info(f"Iterations: {self.iteration:,}")
        self._log.info(f"Total events: {self.kernel.exec_engine.event_count:,}")
        self._log.info(f"Total orders: {self.kernel.cache.orders_total_count():,}")

        # Get all positions for venue
        cdef list positions = []
        for position in self.kernel.cache.positions() + self.kernel.cache.position_snapshots():
            positions.append(position)

        self._log.info(f"Total positions: {len(positions):,}")

        if not self._config.run_analysis:
            return

        for exchange in self._venues.values():
            account = exchange.exec_client.get_account()
            self._log.info("\033[36m=================================================================")
            self._log.info(f"\033[36m SimulatedVenue {exchange.id}")
            self._log.info("\033[36m=================================================================")
            self._log.info(f"{repr(account)}")
            self._log.info("\033[36m-----------------------------------------------------------------")
            if exchange.is_frozen_account:
                self._log.warning(f"ACCOUNT FROZEN")
            else:
                if account is None:
                    continue
                self._log.info(f"Balances starting:")
                for b in account.starting_balances().values():
                    self._log.info(b.to_str())
                self._log.info("\033[36m-----------------------------------------------------------------")
                self._log.info(f"Balances ending:")
                for b in account.balances_total().values():
                    self._log.info(b.to_str())
                self._log.info("\033[36m-----------------------------------------------------------------")
                self._log.info(f"Commissions:")
                for c in account.commissions().values():
                    self._log.info(Money(-c.as_double(), c.currency).to_str())  # Display commission as negative
                self._log.info("\033[36m-----------------------------------------------------------------")
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

            self._log.info("\033[36m=================================================================")
            self._log.info("\033[36m PORTFOLIO PERFORMANCE")
            self._log.info("\033[36m=================================================================")

            # Find all positions for venue
            exchange_positions = []
            for position in positions:
                if position.instrument_id.venue == exchange.id:
                    exchange_positions.append(position)

            # Calculate statistics
            self.kernel.portfolio.analyzer.calculate_statistics(account, exchange_positions)

            # Present PnL performance stats per asset
            for currency in account.currencies():
                self._log.info(f" PnL Statistics ({str(currency)})")
                self._log.info("\033[36m-----------------------------------------------------------------")
                for stat in self.kernel.portfolio.analyzer.get_stats_pnls_formatted(currency):
                    self._log.info(stat)
                self._log.info("\033[36m-----------------------------------------------------------------")

            self._log.info(" Returns Statistics")
            self._log.info("\033[36m-----------------------------------------------------------------")
            for stat in self.kernel.portfolio.analyzer.get_stats_returns_formatted():
                self._log.info(stat)
            self._log.info("\033[36m-----------------------------------------------------------------")

            self._log.info(" General Statistics")
            self._log.info("\033[36m-----------------------------------------------------------------")
            for stat in self.kernel.portfolio.analyzer.get_stats_general_formatted():
                self._log.info(stat)
            self._log.info("\033[36m-----------------------------------------------------------------")

    def _add_data_client_if_not_exists(self, ClientId client_id) -> None:
        if client_id not in self.kernel.data_engine.registered_clients:
            client = BacktestDataClient(
                client_id=client_id,
                msgbus=self.kernel.msgbus,
                cache=self.kernel.cache,
                clock=self.kernel.clock,
                logger=self.kernel.logger,
            )
            self.kernel.data_engine.register_client(client)

    def _add_market_data_client_if_not_exists(self, Venue venue) -> None:
        cdef ClientId client_id = ClientId(venue.to_str())
        if client_id not in self.kernel.data_engine.registered_clients:
            client = BacktestMarketDataClient(
                client_id=client_id,
                msgbus=self.kernel.msgbus,
                cache=self.kernel.cache,
                clock=self.kernel.clock,
                logger=self.kernel.logger,
            )
            self.kernel.data_engine.register_client(client)
