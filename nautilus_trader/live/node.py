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

import asyncio
import concurrent.futures
import platform
import signal
import socket
import sys
import time
import warnings
from datetime import timedelta
from typing import Any, Callable, Dict, Optional

import msgpack
import pydantic
import redis
from pydantic import PositiveFloat

from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.cache import CacheConfig
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.common.logging import nautilus_header
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.infrastructure.cache import CacheDatabaseConfig
from nautilus_trader.infrastructure.cache import RedisCacheDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.data_engine import LiveDataEngineConfig
from nautilus_trader.live.execution_engine import LiveExecEngineConfig
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.node_builder import TradingNodeBuilder
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.live.risk_engine import LiveRiskEngineConfig
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.serialization.msgpack.serializer import MsgPackSerializer
from nautilus_trader.trading.trader import Trader


try:
    import uvloop

    asyncio.set_event_loop_policy(uvloop.EventLoopPolicy())
    uvloop_version = uvloop.__version__
except ImportError:  # pragma: no cover
    uvloop_version = None
    warnings.warn("uvloop is not available.")


class TradingNodeConfig(pydantic.BaseModel):
    """
    Configuration for ``TradingNode`` instances.

    trader_id : str, default="TRADER-000"
        The trader ID for the node (must be a name and ID tag separated by a hyphen)
    log_level : str, default="INFO"
        The stdout log level for the node.
    cache : CacheConfig, optional
        The cache configuration.
    cache_database : CacheDatabaseConfig, optional
        The cache database configuration.
    data_engine : LiveDataEngineConfig, optional
        The live data engine configuration.
    risk_engine : LiveRiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : LiveExecEngineConfig, optional
        The live execution engine configuration.
    loop_debug : bool, default=False
        If the asyncio event loop should be in debug mode.
    load_strategy_state : bool, default=True
        If trading strategy state should be loaded from the database on start.
    save_strategy_state : bool, default=True
        If trading strategy state should be saved to the database on stop.
    timeout_connection : PositiveFloat (seconds)
        The timeout for all clients to connect and initialize.
    timeout_reconciliation : PositiveFloat (seconds)
        The timeout for execution state to reconcile.
    timeout_portfolio : PositiveFloat (seconds)
        The timeout for portfolio to initialize margins and unrealized PnLs.
    timeout_disconnection : PositiveFloat (seconds)
        The timeout for all engine clients to disconnect.
    check_residuals_delay : PositiveFloat (seconds)
        The delay after stopping the node to check residual state before final shutdown.
    data_clients : Dict[str, Dict[str, Any]], optional
        The data client configurations.
    exec_clients : Dict[str, Dict[str, Any]], optional
        The execution client configurations.
    """

    trader_id: str = "TRADER-000"
    log_level: str = "INFO"
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: Optional[LiveDataEngineConfig] = None
    risk_engine: Optional[LiveRiskEngineConfig] = None
    exec_engine: Optional[LiveExecEngineConfig] = None
    loop_debug: bool = False
    load_strategy_state: bool = True
    save_strategy_state: bool = True
    timeout_connection: PositiveFloat = 10.0
    timeout_reconciliation: PositiveFloat = 10.0
    timeout_portfolio: PositiveFloat = 10.0
    timeout_disconnection: PositiveFloat = 10.0
    check_residuals_delay: PositiveFloat = 10.0
    data_clients: Dict[str, Dict[str, Any]] = {}
    exec_clients: Dict[str, Dict[str, Any]] = {}


class TradingNode:
    """
    Provides an asynchronous network node for live trading.
    """

    def __init__(self, config: Optional[TradingNodeConfig] = None):
        """
        Initialize a new instance of the TradingNode class.

        Parameters
        ----------
        config : TradingNodeConfig, optional
            The configuration for the instance.

        Raises
        ------
        TypeError
            If config is not of type `TradingNodeConfig`.

        """
        if config is None:
            config = TradingNodeConfig()
        PyCondition.not_none(config, "config")
        PyCondition.type(config, TradingNodeConfig, "config")

        # Configuration
        self._config = config

        # Setup loop
        self._loop = asyncio.get_event_loop()
        self._executor = concurrent.futures.ThreadPoolExecutor()
        self._loop.set_default_executor(self._executor)
        self._loop.set_debug(config.loop_debug)

        # Components
        self._clock = LiveClock(loop=self._loop)
        self._uuid_factory = UUIDFactory()
        self.created_time = self._clock.utc_now()
        self._is_running = False

        # Identifiers
        self.trader_id = TraderId(config.trader_id)
        self.machine_id = socket.gethostname()
        self.instance_id = self._uuid_factory.generate()

        # Setup logging
        self._logger = LiveLogger(
            loop=self._loop,
            clock=self._clock,
            trader_id=self.trader_id,
            machine_id=self.machine_id,
            instance_id=self.instance_id,
            level_stdout=LogLevelParser.from_str_py(config.log_level.upper()),
        )

        self._log = LoggerAdapter(
            component_name=type(self).__name__,
            logger=self._logger,
        )

        self._log_header()
        self._log.info("Building...")

        if platform.system() != "Windows":
            # Windows does not support signal handling
            # https://stackoverflow.com/questions/45987985/asyncio-loops-add-signal-handler-in-windows
            self._setup_loop()

        ########################################################################
        # Build platform
        ########################################################################
        if config.cache_database is None or config.cache_database.type == "in-memory":
            cache_db = None
        elif config.cache_database.type == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=self.trader_id,
                logger=self._logger,
                serializer=MsgPackSerializer(timestamps_as_str=True),
                config=config.cache_database,
            )
        else:  # pragma: no cover (design-time error)
            raise ValueError(
                "The cache_db_type in the configuration is unrecognized, "
                "can one of {{'in-memory', 'redis'}}.",
            )

        self._msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self._clock,
            logger=self._logger,
        )

        self._cache = Cache(
            database=cache_db,
            logger=self._logger,
            config=config.cache,
        )

        self.portfolio = Portfolio(
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
        )

        self._data_engine = LiveDataEngine(
            loop=self._loop,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
            config=config.data_engine,
        )

        self._exec_engine = LiveExecutionEngine(
            loop=self._loop,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
            config=config.exec_engine,
        )
        self._exec_engine.load_cache()

        self._risk_engine = LiveRiskEngine(
            loop=self._loop,
            portfolio=self.portfolio,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
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
            clock=self._clock,
            logger=self._logger,
        )

        if config.load_strategy_state:
            self.trader.load()

        self._builder = TradingNodeBuilder(
            loop=self._loop,
            data_engine=self._data_engine,
            exec_engine=self._exec_engine,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            logger=self._logger,
            log=self._log,
        )

        self._log.info("INITIALIZED.")
        self.time_to_initialize = self._clock.delta(self.created_time)
        self._log.info(f"Initialized in {int(self.time_to_initialize.total_seconds() * 1000)}ms.")

        self._is_built = False

    @property
    def is_running(self) -> bool:
        """
        If the trading node is running.

        Returns
        -------
        bool

        """
        return self._is_running

    @property
    def is_built(self) -> bool:
        """
        If the trading node clients are built.

        Returns
        -------
        bool

        """
        return self._is_built

    def get_event_loop(self) -> asyncio.AbstractEventLoop:
        """
        Return the event loop of the trading node.

        Returns
        -------
        asyncio.AbstractEventLoop

        """
        return self._loop

    def get_logger(self) -> LiveLogger:
        """
        Return the logger for the trading node.

        Returns
        -------
        LiveLogger

        """
        return self._logger

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
            If handler already registered.

        """
        self._logger.register_sink(handler=handler)

    def add_data_client_factory(self, name: str, factory):
        """
        Add the given data client factory to the node.

        Parameters
        ----------
        name : str
            The name of the client factory.
        factory : LiveDataClientFactory or LiveExecutionClientFactory
            The factory to add.

        Raises
        ------
        ValueError
            If name is not a valid string.
        KeyError
            If name has already been added.

        """
        self._builder.add_data_client_factory(name, factory)

    def add_exec_client_factory(self, name: str, factory):
        """
        Add the given execution client factory to the node.

        Parameters
        ----------
        name : str
            The name of the client factory.
        factory : LiveDataClientFactory or LiveExecutionClientFactory
            The factory to add.

        Raises
        ------
        ValueError
            If name is not a valid string.
        KeyError
            If name has already been added.

        """
        self._builder.add_exec_client_factory(name, factory)

    def build(self) -> None:
        """
        Build the nodes clients.
        """
        if self._is_built:
            raise RuntimeError("the trading nodes clients are already built.")

        self._builder.build_data_clients(self._config.data_clients)
        self._builder.build_exec_clients(self._config.exec_clients)
        self._is_built = True

    def start(self) -> None:
        """
        Start the trading node.
        """
        if not self._is_built:
            raise RuntimeError(
                "The trading nodes clients have not been built. "
                "Please run `node.build()` prior to start."
            )

        try:
            if self._loop.is_running():
                self._loop.create_task(self._run())
            else:
                self._loop.run_until_complete(self._run())

        except RuntimeError as ex:
            self._log.exception(ex)

    def stop(self) -> None:
        """
        Stop the trading node gracefully.

        After a specified delay the internal `Trader` residuals will be checked.

        If save strategy is specified then strategy states will then be saved.

        """
        try:
            if self._loop.is_running():
                self._loop.create_task(self._stop())
            else:
                self._loop.run_until_complete(self._stop())

        except RuntimeError as ex:
            self._log.exception(ex)

    def dispose(self) -> None:
        """
        Dispose of the trading node.

        Gracefully shuts down the executor and event loop.

        """
        try:
            timeout = self._clock.utc_now() + timedelta(seconds=self._config.timeout_disconnection)
            while self._is_running:
                time.sleep(0.1)
                if self._clock.utc_now() >= timeout:
                    self._log.warning(
                        f"Timed out ({self._config.timeout_disconnection}s) waiting for node to stop."
                        f"\nStatus"
                        f"\n------"
                        f"\nDataEngine.check_disconnected() == {self._data_engine.check_disconnected()}"
                        f"\nExecEngine.check_disconnected() == {self._exec_engine.check_disconnected()}"
                    )
                    break

            self._log.info("DISPOSING...")

            self._log.debug(f"{self._data_engine.get_run_queue_task()}")
            self._log.debug(f"{self._exec_engine.get_run_queue_task()}")
            self._log.debug(f"{self._risk_engine.get_run_queue_task()}")

            self.trader.dispose()
            self._data_engine.dispose()
            self._exec_engine.dispose()
            self._risk_engine.dispose()

            self._log.info("Shutting down executor...")
            if sys.version_info >= (3, 9):
                # cancel_futures added in Python 3.9
                self._executor.shutdown(wait=True, cancel_futures=True)
            else:
                self._executor.shutdown(wait=True)

            self._log.info("Stopping event loop...")
            self._cancel_all_tasks()
            self._loop.stop()
        except RuntimeError as ex:
            self._log.exception(ex)
        finally:
            if self._loop.is_running():
                self._log.warning("Cannot close a running event loop.")
            else:
                self._log.info("Closing event loop...")
                self._loop.close()

            # Check and log if event loop is running
            if self._loop.is_running():
                self._log.warning(f"loop.is_running={self._loop.is_running()}")
            else:
                self._log.info(f"loop.is_running={self._loop.is_running()}")

            # Check and log if event loop is closed
            if not self._loop.is_closed():
                self._log.warning(f"loop.is_closed={self._loop.is_closed()}")
            else:
                self._log.info(f"loop.is_closed={self._loop.is_closed()}")

            self._log.info("DISPOSED.")

    def _log_header(self) -> None:
        nautilus_header(self._log)
        self._log.info(f"redis {redis.__version__}")  # type: ignore
        self._log.info(f"msgpack {msgpack.version[0]}.{msgpack.version[1]}.{msgpack.version[2]}")
        if uvloop_version:
            self._log.info(f"uvloop {uvloop_version}")
        self._log.info("=================================================================")

    def _setup_loop(self) -> None:
        if self._loop.is_closed():
            self._log.error("Cannot setup signal handling (event loop was closed).")
            return

        signal.signal(signal.SIGINT, signal.SIG_DFL)
        signals = (signal.SIGTERM, signal.SIGINT, signal.SIGABRT)
        for sig in signals:
            self._loop.add_signal_handler(sig, self._loop_sig_handler, sig)
        self._log.debug(f"Event loop {signals} handling setup.")

    def _loop_sig_handler(self, sig) -> None:
        self._loop.remove_signal_handler(signal.SIGTERM)
        self._loop.add_signal_handler(signal.SIGINT, lambda: None)

        self._log.warning(f"Received {sig!s}, shutting down...")
        self.stop()

    async def _run(self) -> None:
        try:
            self._log.info("STARTING...")
            self._is_running = True

            # Start system
            self._logger.start()
            self._data_engine.start()
            self._exec_engine.start()
            self._risk_engine.start()

            # Await engine connection and initialization
            self._log.info(
                f"Waiting for engines to connect and initialize "
                f"({self._config.timeout_connection}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._await_engines_connected():
                self._log.warning(
                    f"Timed out ({self._config.timeout_connection}s) waiting for engines to connect and initialize."
                    f"\nStatus"
                    f"\n------"
                    f"\nDataEngine.check_connected() == {self._data_engine.check_connected()}"
                    f"\nExecEngine.check_connected() == {self._exec_engine.check_connected()}"
                )
                return
            self._log.info("Engines connected.", color=LogColor.GREEN)

            # Await execution state reconciliation
            self._log.info(
                f"Waiting for execution state to reconcile "
                f"({self._config.timeout_reconciliation}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._exec_engine.reconcile_state(
                timeout_secs=self._config.timeout_reconciliation,
            ):
                self._log.warning(
                    f"Timed out ({self._config.timeout_reconciliation}s) waiting for "
                    f"execution state to reconcile."
                )
                return
            self._log.info("State reconciled.", color=LogColor.GREEN)

            # Initialize portfolio
            self.portfolio.initialize_orders()
            self.portfolio.initialize_positions()

            # Await portfolio initialization
            self._log.info(
                "Waiting for portfolio to initialize "
                f"({self._config.timeout_portfolio}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._await_portfolio_initialized():
                self._log.warning(
                    f"Timed out ({self._config.timeout_portfolio}s) waiting for portfolio to initialize."
                    f"\nStatus"
                    f"\n------"
                    f"\nPortfolio.initialized == {self.portfolio.initialized}"
                )
                return
            self._log.info("Portfolio initialized.", color=LogColor.GREEN)

            # Start trader and strategies
            self.trader.start()

            if self._loop.is_running():
                self._log.info("RUNNING.")
            else:
                self._log.warning("Event loop is not running.")

            # Continue to run while engines are running...
            await self._data_engine.get_run_queue_task()
            await self._exec_engine.get_run_queue_task()
            await self._risk_engine.get_run_queue_task()
        except asyncio.CancelledError as ex:
            self._log.error(str(ex))

    async def _await_engines_connected(self) -> bool:
        # - The data engine clients will be set connected when all
        # instruments are received and updated with the data engine.
        # - The execution engine clients will be set connected when all
        # accounts are updated and the current order and position status is
        # reconciled.
        # Thus any delay here will be due to blocking network IO.
        seconds = self._config.timeout_connection
        timeout: timedelta = self._clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self._clock.utc_now() >= timeout:
                return False
            if not self._data_engine.check_connected():
                continue
            if not self._exec_engine.check_connected():
                continue
            break

        return True  # Engines connected

    async def _await_portfolio_initialized(self) -> bool:
        # - The portfolio will be set initialized when all margin and unrealized
        # PnL calculations are completed (may be waiting on first quote).
        # Thus any delay here will be due to blocking network IO.
        seconds = self._config.timeout_portfolio
        timeout: timedelta = self._clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self._clock.utc_now() >= timeout:
                return False
            if not self.portfolio.initialized:
                continue
            break

        return True  # Portfolio initialized

    async def _stop(self) -> None:
        self._is_stopping = True
        self._log.info("STOPPING...")

        if self.trader.state == ComponentState.RUNNING:
            self.trader.stop()
            self._log.info(
                f"Awaiting residual state ({self._config.check_residuals_delay}s delay)...",
                color=LogColor.BLUE,
            )
            await asyncio.sleep(self._config.check_residuals_delay)
            self.trader.check_residuals()

        if self._config.save_strategy_state:
            self.trader.save()

        if self._data_engine.state == ComponentState.RUNNING:
            self._data_engine.stop()
        if self._exec_engine.state == ComponentState.RUNNING:
            self._exec_engine.stop()
        if self._risk_engine.state == ComponentState.RUNNING:
            self._risk_engine.stop()

        self._log.info(
            f"Waiting for engines to disconnect "
            f"({self._config.timeout_disconnection}s timeout)...",
            color=LogColor.BLUE,
        )
        if not await self._await_engines_disconnected():
            self._log.error(
                f"Timed out ({self._config.timeout_disconnection}s) waiting for engines to disconnect."
                f"\nStatus"
                f"\n------"
                f"\nDataEngine.check_disconnected() == {self._data_engine.check_disconnected()}"
                f"\nExecEngine.check_disconnected() == {self._exec_engine.check_disconnected()}"
            )

        # Clean up remaining timers
        timer_names = self._clock.timer_names()
        self._clock.cancel_timers()

        for name in timer_names:
            self._log.info(f"Cancelled Timer(name={name}).")

        self._log.info("STOPPED.")
        self._logger.stop()
        self._is_running = False

    async def _await_engines_disconnected(self) -> bool:
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

        return True  # Engines disconnected

    def _cancel_all_tasks(self) -> None:
        to_cancel = asyncio.tasks.all_tasks(self._loop)
        if not to_cancel:
            self._log.info("All tasks finished.")
            return

        for task in to_cancel:
            self._log.warning(f"Canceling pending task {task}")
            task.cancel()

        if self._loop.is_running():
            self._log.warning("Event loop still running during `cancel_all_tasks`.")
            return

        finish_all_tasks: asyncio.Future = asyncio.tasks.gather(
            *to_cancel,
            loop=self._loop,
            return_exceptions=True,
        )
        self._loop.run_until_complete(finish_all_tasks)

        self._log.debug(f"{finish_all_tasks}")

        for task in to_cancel:  # pragma: no cover
            if task.cancelled():
                continue
            if task.exception() is not None:
                self._loop.call_exception_handler(
                    {
                        "message": "unhandled exception during asyncio.run() shutdown",
                        "exception": task.exception(),
                        "task": task,
                    }
                )
