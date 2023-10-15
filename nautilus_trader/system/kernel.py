# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

import asyncio
import concurrent.futures
import platform
import signal
import socket
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import timedelta
from typing import Callable

import msgspec

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common import Environment
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import Clock
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.enums import log_level_from_str
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import nautilus_header
from nautilus_trader.config import ActorFactory
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StrategyFactory
from nautilus_trader.config import StreamingConfig
from nautilus_trader.config.common import ControllerFactory
from nautilus_trader.config.common import ExecAlgorithmFactory
from nautilus_trader.config.common import LoggingConfig
from nautilus_trader.config.common import NautilusKernelConfig
from nautilus_trader.config.common import TracingConfig
from nautilus_trader.config.error import InvalidConfiguration
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import nanos_to_millis
from nautilus_trader.core.nautilus_pyo3 import LogGuard
from nautilus_trader.core.nautilus_pyo3 import set_global_log_collector
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.algorithm import ExecAlgorithm
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.infrastructure.cache import RedisCacheDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.writer import StreamingFeatherWriter
from nautilus_trader.portfolio.base import PortfolioFacade
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.serialization.msgpack.serializer import MsgPackSerializer
from nautilus_trader.trading.controller import Controller
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.trader import Trader


try:
    import uvloop

    asyncio.set_event_loop_policy(uvloop.EventLoopPolicy())
except ImportError:  # pragma: no cover
    uvloop = None


class NautilusKernel:
    """
    Provides the core Nautilus system kernel.

    The kernel is common between ``backtest``, ``sandbox`` and ``live`` environment context types.

    Parameters
    ----------
    name : str
        The name for the kernel (will prepend all log messages).
    config : NautilusKernelConfig
        The configuration for the kernel instance.
    loop : asyncio.AbstractEventLoop, optional
        The event loop for the kernel.
    loop_sig_callback : Callable, optional
        The callback for the signal handler.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    TypeError
        If any configuration object is not of the expected type.
    InvalidConfiguration
        If any configuration object is mismatched with the environment context,
        (live configurations for 'backtest', or backtest configurations for 'live').

    """

    def __init__(  # noqa (too complex)
        self,
        name: str,
        config: NautilusKernelConfig,
        loop: asyncio.AbstractEventLoop | None = None,
        loop_sig_callback: Callable | None = None,
    ) -> None:
        PyCondition.valid_string(name, "name")
        PyCondition.type(config, NautilusKernelConfig, "config")

        self._config: NautilusKernelConfig = config
        self._environment: Environment = config.environment
        self._load_state: bool = config.load_state
        self._save_state: bool = config.save_state

        # Identifiers
        self._name: str = name
        self._trader_id: TraderId = TraderId(config.trader_id)
        self._machine_id: str = socket.gethostname()
        self._instance_id: UUID4 = (
            UUID4(config.instance_id) if config.instance_id is not None else UUID4()
        )
        self._ts_created: int = time.time_ns()

        # Components
        if self._environment == Environment.BACKTEST:
            self._clock = TestClock()
        elif self.environment in (Environment.SANDBOX, Environment.LIVE):
            self._clock = LiveClock(loop=loop)
        else:
            raise NotImplementedError(  # pragma: no cover (design-time error)
                f"environment {self._environment} not recognized",  # pragma: no cover (design-time error)
            )

        # Set the global tracing collector
        # This should only be set once for the whole duration of the application
        tracing: TracingConfig = config.tracing or TracingConfig()
        self._log_guard: LogGuard = set_global_log_collector(
            tracing.stdout_level,
            tracing.stderr_level,
            tracing.file_level,
        )

        logging: LoggingConfig = config.logging or LoggingConfig()

        # Setup the logger with a `LiveClock` initially,
        # which is later swapped out for a `TestClock` in the `BacktestEngine`.
        self._logger: Logger = Logger(
            clock=self._clock if isinstance(self._clock, LiveClock) else LiveClock(),
            trader_id=self._trader_id,
            machine_id=self._machine_id,
            instance_id=self._instance_id,
            level_stdout=log_level_from_str(logging.log_level),
            level_file=log_level_from_str(logging.log_level_file)
            if logging.log_level_file is not None
            else LogLevel.DEBUG,
            file_logging=logging.log_level_file is not None,
            directory=logging.log_directory,
            file_name=logging.log_file_name,
            file_format=logging.log_file_format,
            component_levels=logging.log_component_levels,
            bypass=False if self._environment == Environment.LIVE else logging.bypass_logging,
        )

        # Setup logging
        self._log: LoggerAdapter = LoggerAdapter(
            component_name=name,
            logger=self._logger,
        )

        nautilus_header(self._log)
        self.log.info("Building system kernel...")

        # Setup loop (if sandbox live)
        self._loop: asyncio.AbstractEventLoop | None = None
        if self._environment != Environment.BACKTEST:
            self._loop = loop or asyncio.get_running_loop()
            if loop is not None:
                self._executor = concurrent.futures.ThreadPoolExecutor()
                self._loop.set_default_executor(self.executor)
                self._loop.set_debug(config.loop_debug)
                self._loop_sig_callback = loop_sig_callback
                if platform.system() != "Windows":
                    # Windows does not support signal handling
                    # https://stackoverflow.com/questions/45987985/asyncio-loops-add-signal-handler-in-windows
                    self._setup_loop()

        if config.cache_database is None or config.cache_database.type == "in-memory":
            cache_db = None
        elif config.cache_database.type == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=self._trader_id,
                logger=self._logger,
                serializer=MsgPackSerializer(
                    timestamps_as_str=True,  # Hardcoded for now
                    timestamps_as_iso8601=config.cache_database.timestamps_as_iso8601,
                ),
                config=config.cache_database,
            )
        else:
            raise ValueError(
                "The `cache_db_config.type` is unrecognized. "
                "Use one of {{'in-memory', 'redis'}}.",
            )

        ########################################################################
        # Core components
        ########################################################################
        self._msgbus = MessageBus(
            trader_id=self._trader_id,
            clock=self._clock,
            logger=self._logger,
        )

        self._cache = Cache(
            database=cache_db,
            logger=self._logger,
            config=config.cache,
        )

        self._portfolio = Portfolio(
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
        )

        ########################################################################
        # Data components
        ########################################################################
        if isinstance(config.data_engine, LiveDataEngineConfig):
            if config.environment == Environment.BACKTEST:
                raise InvalidConfiguration(
                    f"Cannot use `LiveDataEngineConfig` in a '{config.environment.value}' environment. "
                    "Try using a `DataEngineConfig`.",
                )
            self._data_engine = LiveDataEngine(
                loop=self.loop,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.data_engine,
            )
        elif isinstance(config.data_engine, DataEngineConfig):
            if config.environment != Environment.BACKTEST:
                raise InvalidConfiguration(
                    f"Cannot use `DataEngineConfig` in a '{config.environment.value}' environment. "
                    "Try using a `LiveDataEngineConfig`.",
                )
            self._data_engine = DataEngine(
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.data_engine,
            )

        ########################################################################
        # Risk components
        ########################################################################
        if isinstance(config.risk_engine, LiveRiskEngineConfig):
            if config.environment == Environment.BACKTEST:
                raise InvalidConfiguration(
                    f"Cannot use `LiveRiskEngineConfig` in a '{config.environment.value}' environment. "
                    "Try using a `RiskEngineConfig`.",
                )
            self._risk_engine = LiveRiskEngine(
                loop=self.loop,
                portfolio=self._portfolio,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.risk_engine,
            )
        elif isinstance(config.risk_engine, RiskEngineConfig):
            if config.environment != Environment.BACKTEST:
                raise InvalidConfiguration(
                    f"Cannot use `RiskEngineConfig` in a '{config.environment.value}' environment. "
                    "Try using a `LiveRiskEngineConfig`.",
                )
            self._risk_engine = RiskEngine(
                portfolio=self._portfolio,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.risk_engine,
            )

        ########################################################################
        # Execution components
        ########################################################################
        if isinstance(config.exec_engine, LiveExecEngineConfig):
            if config.environment == Environment.BACKTEST:
                raise InvalidConfiguration(
                    f"Cannot use `LiveExecEngineConfig` in a '{config.environment.value}' environment. "
                    "Try using a `ExecEngineConfig`.",
                )
            self._exec_engine = LiveExecutionEngine(
                loop=self.loop,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.exec_engine,
            )
        elif isinstance(config.exec_engine, ExecEngineConfig):
            if config.environment != Environment.BACKTEST:
                raise InvalidConfiguration(
                    f"Cannot use `ExecEngineConfig` in a '{config.environment.value}' environment. "
                    "Try using a `LiveExecEngineConfig`.",
                )
            self._exec_engine = ExecutionEngine(
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.exec_engine,
            )

        if config.exec_engine and config.exec_engine.load_cache:
            self.exec_engine.load_cache()

        self._emulator = OrderEmulator(
            trader_id=self._trader_id,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
            config=config.emulator,
        )

        ########################################################################
        # Trader
        ########################################################################
        self._trader = Trader(
            trader_id=self._trader_id,
            msgbus=self._msgbus,
            cache=self._cache,
            portfolio=self._portfolio,
            data_engine=self._data_engine,
            risk_engine=self._risk_engine,
            exec_engine=self._exec_engine,
            clock=self._clock,
            logger=self._logger,
            loop=self._loop,
            config={
                "has_controller": self._config.controller is not None,
            },
        )

        if self._load_state:
            self._trader.load()

        # Add controller
        self._controller: Controller | None = None
        if self._config.controller:
            self._controller = ControllerFactory.create(
                config=self._config.controller,
                trader=self._trader,
            )
            self._controller.register_base(
                cache=self._cache,
                msgbus=self._msgbus,
                clock=self._clock,
                logger=self._logger,
            )

        # Setup stream writer
        self._writer: StreamingFeatherWriter | None = None
        if config.streaming:
            self._setup_streaming(config=config.streaming)

        # Setup data catalog
        self._catalog: ParquetDataCatalog | None = None
        if config.catalog:
            self._catalog = ParquetDataCatalog(
                path=config.catalog.path,
                fs_protocol=config.catalog.fs_protocol,
                fs_storage_options=config.catalog.fs_storage_options,
            )
            self._data_engine.register_catalog(catalog=self._catalog)

        # Create importable actors
        for actor_config in config.actors:
            actor: Actor = ActorFactory.create(actor_config)
            self._trader.add_actor(actor)

        # Create importable strategies
        for strategy_config in config.strategies:
            strategy: Strategy = StrategyFactory.create(strategy_config)
            self._trader.add_strategy(strategy)

        # Create importable execution algorithms
        for exec_algorithm_config in config.exec_algorithms:
            exec_algorithm: ExecAlgorithm = ExecAlgorithmFactory.create(exec_algorithm_config)
            self._trader.add_exec_algorithm(exec_algorithm)

        build_time_ms = nanos_to_millis(time.time_ns() - self.ts_created)
        self.log.info(f"Initialized in {build_time_ms}ms.")

    def __del__(self) -> None:
        if hasattr(self, "_writer") and self._writer and not self._writer.is_closed:
            self._writer.close()

    def _setup_loop(self) -> None:
        if self._loop is None:
            raise RuntimeError("No event loop available for the node")

        if self._loop.is_closed():
            self.log.error("Cannot setup signal handling (event loop was closed).")
            return

        signal.signal(signal.SIGINT, signal.SIG_DFL)
        signals = (signal.SIGTERM, signal.SIGINT, signal.SIGABRT)
        for sig in signals:
            self._loop.add_signal_handler(sig, self._loop_sig_handler, sig)
        self.log.debug(f"Event loop signal handling setup for {signals}.")

    def _loop_sig_handler(self, sig: signal.Signals) -> None:
        if self._loop is None:
            raise RuntimeError("No event loop available for the node")

        self._loop.remove_signal_handler(signal.SIGTERM)
        self._loop.add_signal_handler(signal.SIGINT, lambda: None)
        if self._loop_sig_callback:
            self._loop_sig_callback(sig)

    def _setup_streaming(self, config: StreamingConfig) -> None:
        # Setup persistence
        path = f"{config.catalog_path}/{self._environment.value}/{self.instance_id}"
        self._writer = StreamingFeatherWriter(
            path=path,
            fs_protocol=config.fs_protocol,
            flush_interval_ms=config.flush_interval_ms,
            include_types=config.include_types,  # type: ignore  # TODO(cs)
            logger=self.log,
        )
        self._trader.subscribe("*", self._writer.write)
        self.log.info(f"Writing data & events to {path}")

        # Save a copy of the config for this kernel to the streaming folder.
        full_path = f"{self._writer.path}/config.json"
        with self._writer.fs.open(full_path, "wb") as f:
            f.write(msgspec.json.encode(self._config))

    @property
    def environment(self) -> Environment:
        """
        Return the kernels environment context { ``BACKTEST``, ``SANDBOX``, ``LIVE`` }.

        Returns
        -------
        Environment

        """
        return self._environment

    @property
    def loop(self) -> asyncio.AbstractEventLoop:
        """
        Return the kernels event loop.

        Returns
        -------
        AbstractEventLoop

        """
        return self._loop or asyncio.get_running_loop()

    @property
    def loop_sig_callback(self) -> Callable | None:
        """
        Return the kernels signal handling callback.

        Returns
        -------
        Callable or ``None``

        """
        return self._loop_sig_callback

    @property
    def executor(self) -> ThreadPoolExecutor | None:
        """
        Return the kernels default executor.

        Returns
        -------
        ThreadPoolExecutor or ``None``

        """
        return self._executor

    @property
    def name(self) -> str:
        """
        Return the kernels name.

        Returns
        -------
        str

        """
        return self._name

    @property
    def trader_id(self) -> TraderId:
        """
        Return the kernels trader ID.

        Returns
        -------
        TraderId

        """
        return self._trader_id

    @property
    def machine_id(self) -> str:
        """
        Return the kernels machine ID.

        Returns
        -------
        str

        """
        return self._machine_id

    @property
    def instance_id(self) -> UUID4:
        """
        Return the kernels instance ID.

        Returns
        -------
        UUID4

        """
        return self._instance_id

    @property
    def ts_created(self) -> int:
        """
        Return the UNIX timestamp (nanoseconds) when the kernel was created.

        Returns
        -------
        uint64_t

        """
        return self._ts_created

    @property
    def load_state(self) -> bool:
        """
        If the kernel has been configured to load actor and strategy state.

        Returns
        -------
        bool

        """
        return self._load_state

    @property
    def save_state(self) -> bool:
        """
        If the kernel has been configured to save actor and strategy state.

        Returns
        -------
        bool

        """
        return self._save_state

    @property
    def clock(self) -> Clock:
        """
        Return the kernels clock.

        Returns
        -------
        Clock

        """
        return self._clock

    @property
    def log(self) -> LoggerAdapter:
        """
        Return the kernels logger adapter.

        Returns
        -------
        LoggerAdapter

        """
        return self._log

    @property
    def logger(self) -> Logger:
        """
        Return the kernels logger.

        Returns
        -------
        Logger

        """
        return self._logger

    @property
    def msgbus(self) -> MessageBus:
        """
        Return the kernels message bus.

        Returns
        -------
        MessageBus

        """
        return self._msgbus

    @property
    def cache(self) -> CacheFacade:
        """
        Return the kernels read-only cache instance.

        Returns
        -------
        CacheFacade

        """
        return self._cache

    @property
    def portfolio(self) -> PortfolioFacade:
        """
        Return the kernels read-only portfolio instance.

        Returns
        -------
        PortfolioFacade

        """
        return self._portfolio

    @property
    def data_engine(self) -> DataEngine:
        """
        Return the kernels data engine.

        Returns
        -------
        DataEngine

        """
        return self._data_engine

    @property
    def risk_engine(self) -> RiskEngine:
        """
        Return the kernels risk engine.

        Returns
        -------
        RiskEngine

        """
        return self._risk_engine

    @property
    def exec_engine(self) -> ExecutionEngine:
        """
        Return the kernels execution engine.

        Returns
        -------
        ExecutionEngine

        """
        return self._exec_engine

    @property
    def emulator(self) -> OrderEmulator:
        """
        Return the kernels order emulator.

        Returns
        -------
        OrderEmulator

        """
        return self._emulator

    @property
    def trader(self) -> Trader:
        """
        Return the kernels trader instance.

        Returns
        -------
        Trader

        """
        return self._trader

    @property
    def writer(self) -> StreamingFeatherWriter | None:
        """
        Return the kernels writer.

        Returns
        -------
        StreamingFeatherWriter or ``None``

        """
        return self._writer

    @property
    def catalog(self) -> ParquetDataCatalog | None:
        """
        Return the kernels data catalog.

        Returns
        -------
        ParquetDataCatalog or ``None``

        """
        return self._catalog

    def start(self) -> None:
        """
        Start the Nautilus system kernel.
        """
        self._log.info("STARTING...")

        self._start_engines()
        self._connect_clients()
        self._emulator.start()
        self._initialize_portfolio()
        self._trader.start()

        if self._controller:
            self._controller.start()

    async def start_async(self) -> None:
        """
        Start the Nautilus system kernel in an asynchronous context with an event loop.

        Raises
        ------
        RuntimeError
            If no event loop has been assigned to the kernel.

        """
        if self.loop is None:
            raise RuntimeError("no event loop has been assigned to the kernel")

        self._log.info("STARTING...")

        self._register_executor()
        self._start_engines()
        self._connect_clients()

        if not await self._await_engines_connected():
            return

        if not await self._await_execution_reconciliation():
            return

        self._emulator.start()
        self._initialize_portfolio()

        if not await self._await_portfolio_initialization():
            return

        self._trader.start()

        if self._controller:
            self._controller.start()

    async def stop(self) -> None:
        """
        Stop the Nautilus system kernel.
        """
        self.log.info("STOPPING...")

        if self._controller:
            self._controller.stop()

        if self._trader.is_running:
            self._trader.stop()

        if self.save_state:
            self._trader.save()

        self._disconnect_clients()

        self._stop_engines()
        self._cancel_timers()
        self._flush_writer()

        self._log.info("STOPPED.")

    async def stop_async(self) -> None:
        """
        Stop the Nautilus system kernel asynchronously.

        After a specified delay the internal `Trader` residual state will be checked.

        If save strategy is configured, then strategy states will be saved.

        Raises
        ------
        RuntimeError
            If no event loop has been assigned to the kernel.

        """
        if self.loop is None:
            raise RuntimeError("no event loop has been assigned to the kernel")

        self.log.info("STOPPING...")

        if self._trader.is_running:
            self._trader.stop()
            await self._await_trader_residuals()

        if self.save_state:
            self._trader.save()

        self._disconnect_clients()

        await self._await_engines_disconnected()

        self._stop_engines()
        self._cancel_timers()
        self._flush_writer()

        self._log.info("STOPPED.")

    def dispose(self) -> None:
        """
        Dispose of the Nautilus kernel, releasing system resources.

        Calling this method multiple times has the same effect as calling it once (it is
        idempotent). Once called, it cannot be reversed, and no other methods should be
        called on this instance.

        """
        self._stop_engines()

        # Dispose all engines
        if not self.data_engine.is_disposed:
            self.data_engine.dispose()
        if not self.risk_engine.is_disposed:
            self.risk_engine.dispose()
        if not self.exec_engine.is_disposed:
            self.exec_engine.dispose()

        if not self.trader.is_disposed:
            self.trader.dispose()

        if self._writer:
            self._writer.close()

    def cancel_all_tasks(self) -> None:
        """
        Cancel all tasks currently running for the Nautilus kernel.

        Raises
        ------
        RuntimeError
            If no event loop has been assigned to the kernel.

        """
        if self.loop is None:
            raise RuntimeError("no event loop has been assigned to the kernel")

        to_cancel = asyncio.tasks.all_tasks(self.loop)
        if not to_cancel:
            self.log.info("All tasks canceled.")
            return

        for task in to_cancel:
            self.log.warning(f"Canceling pending task {task}")
            task.cancel()

        if self.loop and self.loop.is_running():
            self.log.warning("Event loop still running during `cancel_all_tasks`.")
            return

        finish_all_tasks: asyncio.Future = asyncio.tasks.gather(*to_cancel)
        self.loop.run_until_complete(finish_all_tasks)

        self.log.debug(f"{finish_all_tasks}")

        for task in to_cancel:  # pragma: no cover
            if task.cancelled():
                continue
            if task.exception() is not None:
                self.loop.call_exception_handler(
                    {
                        "message": "unhandled exception during `asyncio.run()` shutdown",
                        "exception": task.exception(),
                        "task": task,
                    },
                )

    def _register_executor(self) -> None:
        for actor in self.trader.actors():
            actor.register_executor(self._loop, self._executor)
        for strategy in self.trader.strategies():
            strategy.register_executor(self._loop, self._executor)
        for exec_algorithm in self.trader.exec_algorithms():
            exec_algorithm.register_executor(self._loop, self._executor)

    def _start_engines(self) -> None:
        if self._config.cache_database is not None and self._config.cache_database.flush_on_start:
            self._cache.flush_db()

        self._data_engine.start()
        self._risk_engine.start()
        self._exec_engine.start()

    def _stop_engines(self) -> None:
        if self._data_engine.is_running:
            self._data_engine.stop()
        if self._risk_engine.is_running:
            self._risk_engine.stop()
        if self._exec_engine.is_running:
            self._exec_engine.stop()
        if self._emulator.is_running:
            self._emulator.stop()

    def _connect_clients(self) -> None:
        self._data_engine.connect()
        self._exec_engine.connect()

    def _disconnect_clients(self) -> None:
        self._data_engine.disconnect()
        self._exec_engine.disconnect()

    def _initialize_portfolio(self) -> None:
        self._portfolio.initialize_orders()
        self._portfolio.initialize_positions()

    async def _await_engines_connected(self) -> bool:
        self._log.info(
            f"Awaiting engine connections and initializations "
            f"({self._config.timeout_connection}s timeout)...",
            color=LogColor.BLUE,
        )
        if not await self._check_engines_connected():
            self._log.warning(
                f"Timed out ({self._config.timeout_connection}s) waiting for engines to connect and initialize."
                f"\nStatus"
                f"\n------"
                f"\nDataEngine.check_connected() == {self._data_engine.check_connected()}"
                f"\nExecEngine.check_connected() == {self._exec_engine.check_connected()}",
            )
            return False

        return True

    async def _await_engines_disconnected(self) -> None:
        self._log.info(
            f"Awaiting engine disconnections "
            f"({self._config.timeout_disconnection}s timeout)...",
            color=LogColor.BLUE,
        )
        if not await self._check_engines_disconnected():
            self._log.error(
                f"Timed out ({self._config.timeout_disconnection}s) waiting for engines to disconnect."
                f"\nStatus"
                f"\n------"
                f"\nDataEngine.check_disconnected() == {self._data_engine.check_disconnected()}"
                f"\nExecEngine.check_disconnected() == {self._exec_engine.check_disconnected()}",
            )

    async def _await_execution_reconciliation(self) -> bool:
        self._log.info(
            f"Awaiting execution state reconciliation "
            f"({self._config.timeout_reconciliation}s timeout)...",
            color=LogColor.BLUE,
        )
        if not await self._exec_engine.reconcile_state(
            timeout_secs=self._config.timeout_reconciliation,
        ):
            self._log.error("Execution state could not be reconciled.")
            return False

        self._log.info("Execution state reconciled.", color=LogColor.GREEN)
        return True

    async def _await_portfolio_initialization(self) -> bool:
        self._log.info(
            "Awaiting portfolio initialization " f"({self._config.timeout_portfolio}s timeout)...",
            color=LogColor.BLUE,
        )
        if not await self._check_portfolio_initialized():
            self._log.warning(
                f"Timed out ({self._config.timeout_portfolio}s) waiting for portfolio to initialize."
                f"\nStatus"
                f"\n------"
                f"\nPortfolio.initialized == {self._portfolio.initialized}",
            )
            return False

        self._log.info("Portfolio initialized.", color=LogColor.GREEN)
        return True

    async def _await_trader_residuals(self) -> None:
        self._log.info(
            f"Awaiting post stop ({self._config.timeout_post_stop}s timeout)...",
            color=LogColor.BLUE,
        )
        await asyncio.sleep(self._config.timeout_post_stop)
        self._trader.check_residuals()

    async def _check_engines_connected(self) -> bool:
        # - The data engine clients will be set connected when all
        # instruments are received and updated with the data engine.
        # - The execution engine clients will be set connected when all
        # accounts are updated and the current order and position status is
        # reconciled.
        # Thus any delay here will be due to blocking network I/O.
        seconds = self._config.timeout_connection
        timeout: timedelta = self.clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self.clock.utc_now() >= timeout:
                return False
            if not self._data_engine.check_connected():
                continue
            if not self._exec_engine.check_connected():
                continue
            break

        return True

    async def _check_engines_disconnected(self) -> bool:
        seconds = self._config.timeout_disconnection
        timeout: timedelta = self._clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self._clock.utc_now() >= timeout:
                return False
            if not self._data_engine.check_disconnected():
                continue
            if not self._exec_engine.check_disconnected():
                continue
            break

        return True

    async def _check_portfolio_initialized(self) -> bool:
        # - The portfolio will be set initialized when all margin and unrealized
        # PnL calculations are completed (maybe waiting on first quotes).
        # Thus any delay here will be due to blocking network I/O.
        seconds = self._config.timeout_portfolio
        timeout: timedelta = self._clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self._clock.utc_now() >= timeout:
                return False
            if not self._portfolio.initialized:
                continue
            break

        return True

    def _cancel_timers(self) -> None:
        timer_names = self._clock.timer_names
        self._clock.cancel_timers()

        for name in timer_names:
            self._log.info(f"Canceled Timer(name={name}).")

    def _flush_writer(self) -> None:
        if self._writer is not None:
            self._writer.flush()
