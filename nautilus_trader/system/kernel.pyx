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
import warnings
from asyncio import AbstractEventLoop
from typing import Callable, Dict, List, Optional, Union

from nautilus_trader.common import Environment
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
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StrategyFactory
from nautilus_trader.config import StreamingConfig
from nautilus_trader.persistence.catalog import resolve_path
from nautilus_trader.persistence.streaming import StreamingFeatherWriter

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport nanos_to_millis
from nautilus_trader.core.rust.core cimport unix_timestamp_ns
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.infrastructure.cache cimport RedisCacheDatabase
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.live.risk_engine cimport LiveRiskEngine
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.portfolio cimport Portfolio
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.serialization.msgpack.serializer cimport MsgPackSerializer
from nautilus_trader.trading.strategy cimport Strategy
from nautilus_trader.trading.trader cimport Trader


try:
    import uvloop
    asyncio.set_event_loop_policy(uvloop.EventLoopPolicy())
except ImportError:  # pragma: no cover
    uvloop = None


cdef class NautilusKernel:
    """
    Provides the core Nautilus system kernel

    The kernel is common between backtest, sandbox and live system types.

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
    streaming_config : StreamingConfig, optional
        The configuration for streaming to feather files.
    actor_configs : List[ImportableActorConfig], optional
        The list of importable actor configs.
    strategy_configs : List[ImportableStrategyConfig], optional
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

    def __init__(
        self,
        environment not None: Environment,
        str name not None,
        TraderId trader_id not None,
        cache_config not None: CacheConfig,
        cache_database_config not None: CacheDatabaseConfig,
        data_config not None: Union[DataEngineConfig, LiveDataEngineConfig],
        risk_config not None: Union[RiskEngineConfig, LiveRiskEngineConfig],
        exec_config not None: Union[ExecEngineConfig, LiveExecEngineConfig],
        streaming_config: Optional[StreamingConfig] = None,
        actor_configs: Optional[List[ImportableActorConfig]] = None,
        strategy_configs: Optional[List[ImportableStrategyConfig]] = None,
        loop: Optional[AbstractEventLoop] = None,
        loop_sig_callback: Optional[Callable] = None,
        loop_debug: bool = False,
        load_state: bool = False,
        save_state: bool = False,
        LogLevel log_level = LogLevel.INFO,
        bypass_logging: bool = False,
    ):
        if uvloop is None:
            warnings.warn("uvloop is not available.")
        if actor_configs is None:
            actor_configs = []
        if strategy_configs is None:
            strategy_configs = []
        Condition.type(environment, Environment, "environment")
        Condition.valid_string(name, "name")
        Condition.type(cache_config, CacheConfig, "cache_config")
        Condition.type(cache_database_config, CacheDatabaseConfig, "cache_database_config")
        Condition.true(isinstance(data_config, (DataEngineConfig, LiveDataEngineConfig)), "data_config was unrecognized type", ex_type=TypeError)
        Condition.true(isinstance(risk_config, (RiskEngineConfig, LiveRiskEngineConfig)), "risk_config was unrecognized type", ex_type=TypeError)
        Condition.true(isinstance(exec_config, (ExecEngineConfig, LiveExecEngineConfig)), "exec_config was unrecognized type", ex_type=TypeError)
        Condition.type_or_none(streaming_config, StreamingConfig, "streaming_config")

        self.environment = environment

        # Identifiers
        self.name = name
        self.trader_id = trader_id
        self.machine_id = socket.gethostname()
        self.instance_id = UUID4()
        self.ts_created = unix_timestamp_ns()

        # Components
        if self.environment == Environment.BACKTEST:
            self.clock = TestClock()
            self.logger = Logger(
                clock=LiveClock(loop=loop),
                trader_id=self.trader_id,
                machine_id=self.machine_id,
                instance_id=self.instance_id,
                level_stdout=log_level,
                bypass=bypass_logging,
            )
        elif self.environment in (Environment.SANDBOX, Environment.LIVE):
            self.clock = LiveClock(loop=loop)
            self.logger = LiveLogger(
                loop=loop,
                clock=self.clock,
                trader_id=self.trader_id,
                machine_id=self.machine_id,
                instance_id=self.instance_id,
                level_stdout=log_level,
            )
        else:
            raise NotImplementedError(  # pragma: no cover (design-time error)
                f"environment {environment} not recognized",
            )

        # Setup logging
        self.log = LoggerAdapter(
            component_name=name,
            logger=self.logger,
        )

        nautilus_header(self.log)
        self.log.info("Building system kernel...")

        # Setup loop
        self.loop = loop
        if self.loop is not None:
            self.executor = concurrent.futures.ThreadPoolExecutor()
            self.loop.set_default_executor(self.executor)
            self.loop.set_debug(loop_debug)
            self.loop_sig_callback = loop_sig_callback
            if platform.system() != "Windows":
                # Windows does not support signal handling
                # https://stackoverflow.com/questions/45987985/asyncio-loops-add-signal-handler-in-windows
                self._setup_loop()

        if cache_database_config is None or cache_database_config.type == "in-memory":
            cache_db = None
        elif cache_database_config.type == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=self.trader_id,
                logger=self.logger,
                serializer=MsgPackSerializer(timestamps_as_str=True),
                config=cache_database_config,
            )
        else:
            raise ValueError(
                f"The `cache_db_config.type` is unrecognized. "
                f"Please use one of {{\'in-memory\', \'redis\'}}.",
            )

        ########################################################################
        # Core components
        ########################################################################
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = Cache(
            database=cache_db,
            logger=self.logger,
            config=cache_config,
        )

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        ########################################################################
        # Data Engine
        ########################################################################
        if isinstance(data_config, LiveDataEngineConfig):
            self.data_engine = LiveDataEngine(
                loop=loop,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=data_config,
            )
        elif isinstance(data_config, DataEngineConfig):
            self.data_engine = DataEngine(
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=data_config,
            )

        ########################################################################
        # Risk Engine
        ########################################################################
        if isinstance(risk_config, LiveRiskEngineConfig):
            self.risk_engine = LiveRiskEngine(
                loop=loop,
                portfolio=self.portfolio,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=risk_config,
            )
        elif isinstance(risk_config, RiskEngineConfig):
            self.risk_engine = RiskEngine(
                portfolio=self.portfolio,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=risk_config,
            )

        ########################################################################
        # Execution Engine
        ########################################################################
        if isinstance(exec_config, LiveExecEngineConfig):
            self.exec_engine = LiveExecutionEngine(
                loop=loop,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=exec_config,
            )
        elif isinstance(exec_config, ExecEngineConfig):
            self.exec_engine = ExecutionEngine(
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
                config=exec_config,
            )

        if exec_config.load_cache:
            self.exec_engine.load_cache()

        ########################################################################
        # Trader
        ########################################################################
        self.trader = Trader(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            exec_engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
            loop=self.loop,
        )

        if load_state:
            self.trader.load()

        # Setup writer
        self.writer: Optional[StreamingFeatherWriter] = None
        if streaming_config:
            self._setup_streaming(config=streaming_config)

        # Create importable actors
        for config in actor_configs:
            actor: Actor = ActorFactory.create(config)
            self.trader.add_actor(actor)

        # Create importable strategies
        for config in strategy_configs:
            strategy: Strategy = StrategyFactory.create(config)
            self.trader.add_strategy(strategy)

        cdef uint64_t build_time_ms = nanos_to_millis(unix_timestamp_ns() - self.ts_created)
        self.log.info(f"Initialized in {build_time_ms}ms.")

    def _setup_loop(self) -> None:
        if self.loop.is_closed():
            self.log.error("Cannot setup signal handling (event loop was closed).")
            return

        signal.signal(signal.SIGINT, signal.SIG_DFL)
        signals = (signal.SIGTERM, signal.SIGINT, signal.SIGABRT)
        for sig in signals:
            self.loop.add_signal_handler(sig, self._loop_sig_handler, sig)
        self.log.debug(f"Event loop signal handling setup for {signals}.")

    def _loop_sig_handler(self, sig) -> None:
        self.loop.remove_signal_handler(signal.SIGTERM)
        self.loop.add_signal_handler(signal.SIGINT, lambda: None)
        if self.loop_sig_callback:
            self.loop_sig_callback(sig)

    def _setup_streaming(self, config: StreamingConfig) -> None:
        # Setup persistence
        catalog = config.as_catalog()
        persistence_dir = pathlib.Path(config.catalog_path) / self.environment.value
        parent_path = resolve_path(persistence_dir, fs=config.fs)
        if not catalog.fs.exists(parent_path):
            catalog.fs.mkdir(parent_path)

        path = resolve_path(persistence_dir / f"{self.instance_id}.feather", fs=config.fs)
        self.writer = StreamingFeatherWriter(
            path=path,
            fs_protocol=config.fs_protocol,
            flush_interval_ms=config.flush_interval_ms,
            include_types=config.include_types,
            logger=self.log
        )
        self.trader.subscribe("*", self.writer.write)
        self.log.info(f"Writing data & events to {path}")

    def add_log_sink(self, handler: Callable[[Dict], None]):
        """
        Register the given sink handler with the nodes logger.

        Parameters
        ----------
        handler : Callable[[Dict], None]
            The sink handler to register.

        Raises
        ------
        KeyError
            If `handler` already registered.

        """
        self.logger.register_sink(handler=handler)

    def cancel_all_tasks(self) -> None:
        Condition.not_none(self.loop, "self.loop")

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
                    }
                )
