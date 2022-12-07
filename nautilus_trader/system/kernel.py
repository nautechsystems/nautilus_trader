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

import asyncio
import concurrent.futures
import pathlib
import platform
import signal
import socket
import time
from asyncio import AbstractEventLoop
from concurrent.futures import ThreadPoolExecutor
from typing import Callable, Optional, Union

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common import Environment
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import Clock
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.logging import nautilus_header
from nautilus_trader.config import ActorFactory
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.config import OrderEmulatorConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StrategyFactory
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import nanos_to_millis
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.infrastructure.cache import RedisCacheDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.persistence.catalog import resolve_path
from nautilus_trader.persistence.streaming import StreamingFeatherWriter
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
    environment : Environment { ``BACKTEST``, ``SANDBOX``, ``LIVE`` }
        The environment context for the kernel.
    name : str
        The name for the kernel (will prepend all log messages).
    trader_id : str
        The trader ID for the kernel (must be a name and ID tag separated by a hyphen).
    cache_config : CacheConfig
        The cache configuration for the kernel.
    cache_database_config : CacheDatabaseConfig
        The cache database configuration for the kernel.
    data_config : Union[DataEngineConfig, LiveDataEngineConfig]
        The live data engine configuration for the kernel.
    risk_config : Union[RiskEngineConfig, LiveRiskEngineConfig]
        The risk engine configuration for the kernel.
    exec_config : Union[ExecEngineConfig, LiveExecEngineConfig]
        The execution engine configuration for the kernel.
    emulator_config : Union[ExecEngineConfig, LiveExecEngineConfig]
        The order emulator configuration for the kernel.
    streaming_config : StreamingConfig, optional
        The configuration for streaming to feather files.
    actor_configs : list[ImportableActorConfig], optional
        The list of importable actor configs.
    strategy_configs : list[ImportableStrategyConfig], optional
        The list of importable strategy configs.
    loop : AbstractEventLoop, optional
        The event loop for the kernel.
    loop_sig_callback : Callable, optional
        The callback for the signal handler.
    loop_debug : bool, default False
        If the event loop should run in debug mode.
    load_state : bool, default False
        If strategy state should be loaded on start.
    save_state : bool, default False
        If strategy state should be saved on stop.
    log_level : LogLevel, default LogLevel.INFO
        The log level for the kernels logger.
    bypass_logging : bool, default False
        If logging to stdout should be bypassed.

    Raises
    ------
    TypeError
        If `environment` is not of type `Environment`.
    ValueError
        If `name` is not a valid string.
    TypeError
        If any configuration object is not of the expected type.

    """

    def __init__(  # noqa (too complex)
        self,
        environment: Environment,
        name: str,
        trader_id: TraderId,
        cache_config: CacheConfig,
        cache_database_config: CacheDatabaseConfig,
        data_config: Union[DataEngineConfig, LiveDataEngineConfig],
        risk_config: Union[RiskEngineConfig, LiveRiskEngineConfig],
        exec_config: Union[ExecEngineConfig, LiveExecEngineConfig],
        instance_id: Optional[UUID4] = None,
        emulator_config: Optional[OrderEmulatorConfig] = None,
        streaming_config: Optional[StreamingConfig] = None,
        actor_configs: Optional[list[ImportableActorConfig]] = None,
        strategy_configs: Optional[list[ImportableStrategyConfig]] = None,
        loop: Optional[AbstractEventLoop] = None,
        loop_sig_callback: Optional[Callable] = None,
        loop_debug: bool = False,
        load_state: bool = False,
        save_state: bool = False,
        log_level: LogLevel = LogLevel.INFO,
        bypass_logging: bool = False,
    ):
        PyCondition.not_none(environment, "environment")
        PyCondition.not_none(name, "name")
        PyCondition.not_none(trader_id, "trader_id")
        PyCondition.not_none(cache_config, "cache_config")
        PyCondition.not_none(cache_database_config, "cache_database_config")
        PyCondition.not_none(data_config, "data_config")
        PyCondition.not_none(risk_config, "risk_config")
        PyCondition.not_none(exec_config, "exec_config")
        if actor_configs is None:
            actor_configs = []
        if strategy_configs is None:
            strategy_configs = []
        PyCondition.type(environment, Environment, "environment")
        PyCondition.valid_string(name, "name")
        PyCondition.type(cache_config, CacheConfig, "cache_config")
        PyCondition.type(cache_database_config, CacheDatabaseConfig, "cache_database_config")
        PyCondition.true(
            isinstance(data_config, (DataEngineConfig, LiveDataEngineConfig)),
            "data_config was unrecognized type",
            ex_type=TypeError,
        )
        PyCondition.true(
            isinstance(risk_config, (RiskEngineConfig, LiveRiskEngineConfig)),
            "risk_config was unrecognized type",
            ex_type=TypeError,
        )
        PyCondition.true(
            isinstance(exec_config, (ExecEngineConfig, LiveExecEngineConfig)),
            "exec_config was unrecognized type",
            ex_type=TypeError,
        )
        PyCondition.type_or_none(streaming_config, StreamingConfig, "streaming_config")

        self._environment = environment

        # Identifiers
        self._name = name
        self._trader_id = trader_id
        self._machine_id = socket.gethostname()
        self._instance_id = UUID4(instance_id) if instance_id is not None else UUID4()
        self._ts_created = time.time_ns()

        # Components
        if self._environment == Environment.BACKTEST:
            self._clock = TestClock()
            self._logger = Logger(
                clock=LiveClock(loop=loop),
                trader_id=self._trader_id,
                machine_id=self._machine_id,
                instance_id=self._instance_id,
                level_stdout=log_level,
                bypass=bypass_logging,
            )
        elif self.environment in (Environment.SANDBOX, Environment.LIVE):
            self._clock = LiveClock(loop=loop)
            self._logger = LiveLogger(
                loop=loop,
                clock=self._clock,
                trader_id=self._trader_id,
                machine_id=self._machine_id,
                instance_id=self._instance_id,
                level_stdout=log_level,
            )
        else:
            raise NotImplementedError(  # pragma: no cover (design-time error)
                f"environment {environment} not recognized",  # pragma: no cover (design-time error)
            )

        # Setup logging
        self._log = LoggerAdapter(
            component_name=name,
            logger=self._logger,
        )

        nautilus_header(self._log)
        self.log.info("Building system kernel...")

        # Setup loop
        self._loop: asyncio.AbstractEventLoop = loop or asyncio.get_event_loop()
        if loop is not None:
            self._executor = concurrent.futures.ThreadPoolExecutor()
            self._loop.set_default_executor(self.executor)
            self._loop.set_debug(loop_debug)
            self._loop_sig_callback = loop_sig_callback
            if platform.system() != "Windows":
                # Windows does not support signal handling
                # https://stackoverflow.com/questions/45987985/asyncio-loops-add-signal-handler-in-windows
                self._setup_loop()

        if cache_database_config is None or cache_database_config.type == "in-memory":
            cache_db = None
        elif cache_database_config.type == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=self._trader_id,
                logger=self._logger,
                serializer=MsgPackSerializer(timestamps_as_str=True),
                config=cache_database_config,
            )
        else:
            raise ValueError(
                "The `cache_db_config.type` is unrecognized. "
                "Please use one of {{'in-memory', 'redis'}}.",
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
            config=cache_config,
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
        if isinstance(data_config, LiveDataEngineConfig):
            self._data_engine = LiveDataEngine(
                loop=loop,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=data_config,
            )
        elif isinstance(data_config, DataEngineConfig):
            self._data_engine = DataEngine(
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=data_config,
            )

        ########################################################################
        # Risk components
        ########################################################################
        if isinstance(risk_config, LiveRiskEngineConfig):
            self._risk_engine = LiveRiskEngine(
                loop=loop,
                portfolio=self._portfolio,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=risk_config,
            )
        elif isinstance(risk_config, RiskEngineConfig):
            self._risk_engine = RiskEngine(
                portfolio=self._portfolio,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=risk_config,
            )

        ########################################################################
        # Execution components
        ########################################################################
        if isinstance(exec_config, LiveExecEngineConfig):
            self._exec_engine = LiveExecutionEngine(
                loop=loop,
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=exec_config,
            )
        elif isinstance(exec_config, ExecEngineConfig):
            self._exec_engine = ExecutionEngine(
                msgbus=self._msgbus,
                cache=self._cache,
                clock=self._clock,
                logger=self._logger,
                config=exec_config,
            )

        if exec_config.load_cache:
            self.exec_engine.load_cache()

        self._emulator = OrderEmulator(
            trader_id=self._trader_id,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
            config=emulator_config,
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

        if load_state:
            self._trader.load()

        # Setup writer
        self._writer: Optional[StreamingFeatherWriter] = None
        if streaming_config:
            self._setup_streaming(config=streaming_config)

        # Create importable actors
        for actor_config in actor_configs:
            actor: Actor = ActorFactory.create(actor_config)
            self._trader.add_actor(actor)

        # Create importable strategies
        for strategy_config in strategy_configs:
            strategy: Strategy = StrategyFactory.create(strategy_config)
            self._trader.add_strategy(strategy)

        build_time_ms = nanos_to_millis(time.time_ns() - self.ts_created)
        self.log.info(f"Initialized in {build_time_ms}ms.")

    def __del__(self) -> None:
        if hasattr(self, "_writer") and self._writer and not self._writer.closed:
            self._writer.close()

    def _setup_loop(self) -> None:
        if self._loop.is_closed():
            self.log.error("Cannot setup signal handling (event loop was closed).")
            return

        signal.signal(signal.SIGINT, signal.SIG_DFL)
        signals = (signal.SIGTERM, signal.SIGINT, signal.SIGABRT)
        for sig in signals:
            self._loop.add_signal_handler(sig, self._loop_sig_handler, sig)
        self.log.debug(f"Event loop signal handling setup for {signals}.")

    def _loop_sig_handler(self, sig) -> None:
        self._loop.remove_signal_handler(signal.SIGTERM)
        self._loop.add_signal_handler(signal.SIGINT, lambda: None)
        if self._loop_sig_callback:
            self.loop_sig_callback(sig)

    def _setup_streaming(self, config: StreamingConfig) -> None:
        # Setup persistence
        catalog = config.as_catalog()
        persistence_dir = pathlib.Path(config.catalog_path) / self._environment.value
        parent_path = resolve_path(persistence_dir, fs=config.fs)
        if not catalog.fs.exists(parent_path):
            catalog.fs.mkdir(parent_path)

        path = resolve_path(persistence_dir / f"{self.instance_id}.feather", fs=config.fs)
        self._writer = StreamingFeatherWriter(
            path=path,
            fs_protocol=config.fs_protocol,
            flush_interval_ms=config.flush_interval_ms,
            include_types=config.include_types,  # type: ignore  # TODO(cs)
            logger=self.log,
        )
        self._trader.subscribe("*", self.writer.write)
        self.log.info(f"Writing data & events to {path}")

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
    def loop(self) -> AbstractEventLoop:
        """
        Return the kernels event loop.

        Returns
        -------
        AbstractEventLoop

        """
        return self._loop

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

    def dispose(self) -> None:
        """
        Dispose of the kernel releasing system resources.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.

        """
        self.trader.dispose()

        if self.data_engine.is_running:
            self.data_engine.stop()
        if self.risk_engine.is_running:
            self.risk_engine.stop()
        if self.exec_engine.is_running:
            self.exec_engine.stop()

        self.data_engine.dispose()
        self.risk_engine.dispose()
        self.exec_engine.dispose()

        if self._writer:
            self._writer.close()

    def add_log_sink(self, handler: Callable[[dict], None]):
        """
        Register the given sink handler with the nodes logger.

        Parameters
        ----------
        handler : Callable[[dict], None]
            The sink handler to register.

        Raises
        ------
        KeyError
            If `handler` already registered.

        """
        self.logger.register_sink(handler=handler)

    def cancel_all_tasks(self) -> None:
        PyCondition.not_none(self.loop, "self.loop")

        to_cancel = asyncio.tasks.all_tasks(self.loop)
        if not to_cancel:
            self.log.info("All tasks canceled.")
            return

        for task in to_cancel:
            self.log.warning(f"Canceling pending task {task}")
            task.cancel()

        if self.loop.is_running():
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
