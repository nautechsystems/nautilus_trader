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

import asyncio
import concurrent.futures
import platform
import signal
import socket
import time
from concurrent.futures import ThreadPoolExecutor
from typing import Callable, Optional

import msgspec

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common import Environment
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import Clock
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
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
from nautilus_trader.config.common import ExecAlgorithmFactory
from nautilus_trader.config.common import LoggingConfig
from nautilus_trader.config.common import NautilusKernelConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import nanos_to_millis
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
from nautilus_trader.persistence.streaming.writer import StreamingFeatherWriter
from nautilus_trader.portfolio.base import PortfolioFacade
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.serialization.msgpack.serializer import MsgPackSerializer
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

    The kernel is common between backtest, sandbox and live environment context types.

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
    """

    def __init__(  # noqa (too complex)
        self,
        name: str,
        config: NautilusKernelConfig,
        loop: Optional[asyncio.AbstractEventLoop] = None,
        loop_sig_callback: Optional[Callable] = None,
    ):
        PyCondition.valid_string(name, "name")
        PyCondition.type(config, NautilusKernelConfig, "config")

        self._config = config
        self._environment = config.environment
        self._load_state = config.load_state
        self._save_state = config.save_state

        # Identifiers
        self._name = name
        self._trader_id = TraderId(config.trader_id)
        self._machine_id = socket.gethostname()
        self._instance_id = UUID4(config.instance_id) if config.instance_id is not None else UUID4()
        self._ts_created = time.time_ns()

        # Components
        if self._environment == Environment.BACKTEST:
            self._clock = TestClock()
        elif self.environment in (Environment.SANDBOX, Environment.LIVE):
            self._clock = LiveClock(loop=loop)
        else:
            raise NotImplementedError(  # pragma: no cover (design-time error)
                f"environment {self._environment} not recognized",  # pragma: no cover (design-time error)
            )

        logging: LoggingConfig = config.logging or LoggingConfig()

        # Setup the logger with a `LiveClock` initially,
        # which is later swapped out for a `TestClock` in the `BacktestEngine`.
        self._logger = Logger(
            clock=self._clock if isinstance(self._clock, LiveClock) else LiveClock(),
            trader_id=self._trader_id,
            machine_id=self._machine_id,
            instance_id=self._instance_id,
            level_stdout=log_level_from_str(logging.log_level),
            level_file=log_level_from_str(logging.log_level_file)
            if logging.log_level_file is not None
            else LogLevel.DEBUG,
            file_logging=True if logging.log_level_file is not None else False,
            directory=logging.log_directory,
            file_name=logging.log_file_name,
            file_format=logging.log_file_format,
            component_levels=logging.log_component_levels,
            rate_limit=logging.log_rate_limit,
            bypass=False if self._environment == Environment.LIVE else logging.bypass_logging,
        )

        # Setup logging
        self._log = LoggerAdapter(
            component_name=name,
            logger=self._logger,
        )

        nautilus_header(self._log)
        self.log.info("Building system kernel...")

        # Setup loop (if sandbox live)
        self._loop: Optional[asyncio.AbstractEventLoop] = None
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
                serializer=MsgPackSerializer(timestamps_as_str=True),
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
            self._data_engine = LiveDataEngine(
                loop=self.loop,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.data_engine,
            )
        elif isinstance(config.data_engine, DataEngineConfig):
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
            self._exec_engine = LiveExecutionEngine(
                loop=self.loop,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=config.exec_engine,
            )
        elif isinstance(config.exec_engine, ExecEngineConfig):
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
            config=None,  # No configuration for now
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
        )

        if self._load_state:
            self._trader.load()

        # Setup stream writer
        self._writer: Optional[StreamingFeatherWriter] = None
        if config.streaming:
            self._setup_streaming(config=config.streaming)

        # Setup data catalog
        self._catalog: Optional[ParquetDataCatalog] = None
        if config.catalog:
            self._catalog = ParquetDataCatalog(
                path=config.catalog.path,
                fs_protocol=config.catalog.fs_protocol,
                fs_storage_options=config.catalog.fs_storage_options,
            )
            self._data_engine.register_catalog(
                catalog=self._catalog,
                use_rust=config.catalog.use_rust,
            )

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
        if hasattr(self, "_writer") and self._writer and not self._writer.closed:
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

    def _loop_sig_handler(self, sig) -> None:
        if self._loop is None:
            raise RuntimeError("No event loop available for the node")

        self._loop.remove_signal_handler(signal.SIGTERM)
        self._loop.add_signal_handler(signal.SIGINT, lambda: None)
        if self._loop_sig_callback:
            self._loop_sig_callback(sig)

    def _setup_streaming(self, config: StreamingConfig) -> None:
        # Setup persistence
        path = f"{config.catalog_path}/{self._environment.value}/{self.instance_id}.feather"
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
    def loop_sig_callback(self) -> Optional[Callable]:
        """
        Return the kernels signal handling callback.

        Returns
        -------
        Callable or ``None``

        """
        return self._loop_sig_callback

    @property
    def executor(self) -> Optional[ThreadPoolExecutor]:
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
    def writer(self) -> Optional[StreamingFeatherWriter]:
        """
        Return the kernels writer.

        Returns
        -------
        StreamingFeatherWriter or ``None``

        """
        return self._writer

    @property
    def catalog(self) -> Optional[ParquetDataCatalog]:
        """
        Return the kernels data catalog.

        Returns
        -------
        ParquetDataCatalog or ``None``

        """
        return self._catalog

    def dispose(self) -> None:
        """
        Dispose of the kernel releasing system resources.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.

        """
        # Stop all engines
        if self.data_engine.is_running:
            self.data_engine.stop()
        if self.risk_engine.is_running:
            self.risk_engine.stop()
        if self.exec_engine.is_running:
            self.exec_engine.stop()

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
        PyCondition.not_none(self.loop, "self.loop")

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
                        "message": "unhandled exception during asyncio.run() shutdown",
                        "exception": task.exception(),
                        "task": task,
                    },
                )
