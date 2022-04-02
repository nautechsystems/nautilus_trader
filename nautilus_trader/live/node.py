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
import platform
import signal
import socket
import sys
import time
from datetime import timedelta
from functools import partial
from typing import Any, Callable, Dict, List, Optional

import orjson

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.infrastructure.config import CacheDatabaseConfig
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.config import TradingNodeConfig
from nautilus_trader.live.node_builder import TradingNodeBuilder
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.persistence.config import PersistenceConfig
from nautilus_trader.persistence.streaming import FeatherWriter
from nautilus_trader.portfolio.base import PortfolioFacade
from nautilus_trader.trading.config import StrategyFactory
from nautilus_trader.trading.kernel import NautilusKernel
from nautilus_trader.trading.trader import Trader


class TradingNode:
    """
    Provides an asynchronous network node for live trading.

    Parameters
    ----------
    config : TradingNodeConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `TradingNodeConfig`.
    """

    def __init__(self, config: Optional[TradingNodeConfig] = None):
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
        self._is_running = False

        # Components
        clock = LiveClock(loop=self._loop)
        uuid_factory = UUIDFactory()

        # Identifiers
        trader_id = TraderId(config.trader_id)
        machine_id = socket.gethostname()
        instance_id = uuid_factory.generate()

        # Setup logging
        self._logger = LiveLogger(
            loop=self._loop,
            clock=clock,
            trader_id=trader_id,
            machine_id=machine_id,
            instance_id=instance_id,
            level_stdout=LogLevelParser.from_str_py(config.log_level.upper()),
        )

        self.kernel = NautilusKernel(
            name=type(self).__name__,
            trader_id=trader_id,
            machine_id=machine_id,
            instance_id=instance_id,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=self._logger,
            cache_config=config.cache or CacheConfig(),
            cache_database_config=config.cache_database or CacheDatabaseConfig(),
            data_config=config.data_engine or LiveDataEngineConfig(),
            risk_config=config.risk_engine or LiveRiskEngineConfig(),
            exec_config=config.exec_engine or LiveExecEngineConfig(),
            loop=self._loop,
        )

        if platform.system() != "Windows":
            # Windows does not support signal handling
            # https://stackoverflow.com/questions/45987985/asyncio-loops-add-signal-handler-in-windows
            self._setup_loop()

        if config.load_strategy_state:
            self.kernel.trader.load()

        # Setup persistence (requires trader)
        self.persistence_writers: List[Any] = []
        if config.persistence:
            self._setup_persistence(config=config.persistence)

        self._builder = TradingNodeBuilder(
            loop=self._loop,
            data_engine=self.kernel.data_engine,
            exec_engine=self.kernel.exec_engine,
            msgbus=self.kernel.msgbus,
            cache=self.kernel.cache,
            clock=self.kernel.clock,
            logger=self._logger,
            log=self.kernel.log,
        )

        for strategy_config in self._config.strategies:
            strategy = StrategyFactory.create(strategy_config)  # type: ignore
            self.trader.add_strategy(strategy)  # type: ignore

        self._is_built = False

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
        Return the nodes internal trader.

        Returns
        -------
        Trader

        """
        return self.kernel.trader

    @property
    def cache(self) -> CacheFacade:
        """
        Return the nodes internal read-only cache.

        Returns
        -------
        CacheFacade

        """
        return self.kernel.cache

    @property
    def portfolio(self) -> PortfolioFacade:
        """
        Return the nodes internal read-only portfolio.

        Returns
        -------
        PortfolioFacade

        """
        return self.kernel.portfolio

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
            If `handler` already registered.

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
            If `name` is not a valid string.
        KeyError
            If `name` has already been added.

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
            If `name` is not a valid string.
        KeyError
            If `name` has already been added.

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

    def start(self) -> Optional[asyncio.Task]:
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
                return self._loop.create_task(self._run())
            else:
                self._loop.run_until_complete(self._run())
                return None
        except RuntimeError as ex:
            self.kernel.log.exception("Error on run", ex)
            return None

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
            self.kernel.log.exception("Error on stop", ex)

    def dispose(self) -> None:
        """
        Dispose of the trading node.

        Gracefully shuts down the executor and event loop.

        """
        try:
            timeout = self.kernel.clock.utc_now() + timedelta(
                seconds=self._config.timeout_disconnection
            )
            while self._is_running:
                time.sleep(0.1)
                if self.kernel.clock.utc_now() >= timeout:
                    self.kernel.log.warning(
                        f"Timed out ({self._config.timeout_disconnection}s) waiting for node to stop."
                        f"\nStatus"
                        f"\n------"
                        f"\nDataEngine.check_disconnected() == {self.kernel.data_engine.check_disconnected()}"
                        f"\nExecEngine.check_disconnected() == {self.kernel.exec_engine.check_disconnected()}"
                    )
                    break

            self.kernel.log.info("DISPOSING...")

            self.kernel.log.debug(f"{self.kernel.data_engine.get_run_queue_task()}")
            self.kernel.log.debug(f"{self.kernel.exec_engine.get_run_queue_task()}")
            self.kernel.log.debug(f"{self.kernel.risk_engine.get_run_queue_task()}")

            self.kernel.trader.dispose()
            self.kernel.data_engine.dispose()
            self.kernel.exec_engine.dispose()
            self.kernel.risk_engine.dispose()

            self.kernel.log.info("Shutting down executor...")
            if sys.version_info >= (3, 9):
                # cancel_futures added in Python 3.9
                self._executor.shutdown(wait=True, cancel_futures=True)
            else:
                self._executor.shutdown(wait=True)

            self.kernel.log.info("Stopping event loop...")
            self._cancel_all_tasks()
            self._loop.stop()
        except RuntimeError as ex:
            self.kernel.log.exception("Error on dispose", ex)
        finally:
            if self._loop.is_running():
                self.kernel.log.warning("Cannot close a running event loop.")
            else:
                self.kernel.log.info("Closing event loop...")
                self._loop.close()

            # Check and log if event loop is running
            if self._loop.is_running():
                self.kernel.log.warning(f"loop.is_running={self._loop.is_running()}")
            else:
                self.kernel.log.info(f"loop.is_running={self._loop.is_running()}")

            # Check and log if event loop is closed
            if not self._loop.is_closed():
                self.kernel.log.warning(f"loop.is_closed={self._loop.is_closed()}")
            else:
                self.kernel.log.info(f"loop.is_closed={self._loop.is_closed()}")

            self.kernel.log.info("DISPOSED.")

    def _setup_loop(self) -> None:
        if self._loop.is_closed():
            self.kernel.log.error("Cannot setup signal handling (event loop was closed).")
            return

        signal.signal(signal.SIGINT, signal.SIG_DFL)
        signals = (signal.SIGTERM, signal.SIGINT, signal.SIGABRT)
        for sig in signals:
            self._loop.add_signal_handler(sig, self._loop_sig_handler, sig)
        self.kernel.log.debug(f"Event loop signal handling setup for {signals}.")

    def _setup_persistence(self, config: PersistenceConfig) -> None:
        # Setup persistence
        path = f"{config.catalog_path}/live/{self.kernel.instance_id}.feather"
        writer = FeatherWriter(
            path=path,
            fs_protocol=config.fs_protocol,
            flush_interval=config.flush_interval,
        )
        self.persistence_writers.append(writer)
        self.kernel.trader.subscribe("*", writer.write)
        self.kernel.log.info(f"Persisting data & events to {path=}")

        # Setup logging
        if config.persist_logs:

            def sink(record, f):
                f.write(orjson.dumps(record) + b"\n")

            path = f"{config.catalog_path}/logs/{self.kernel.instance_id}.log"
            log_sink = open(path, "wb")
            self.persistence_writers.append(log_sink)
            self._logger.register_sink(partial(sink, f=log_sink))
            self.kernel.log.info(f"Persisting logs to {path=}")

    def _loop_sig_handler(self, sig) -> None:
        self._loop.remove_signal_handler(signal.SIGTERM)
        self._loop.add_signal_handler(signal.SIGINT, lambda: None)

        self.kernel.log.warning(f"Received {sig!s}, shutting down...")
        self.stop()

    async def _run(self) -> None:
        try:
            self.kernel.log.info("STARTING...")
            self._is_running = True

            # Start system
            self._logger.start()
            self.kernel.data_engine.start()
            self.kernel.exec_engine.start()
            self.kernel.risk_engine.start()

            # Connect all clients
            self.kernel.data_engine.connect()
            self.kernel.exec_engine.connect()

            # Await engine connection and initialization
            self.kernel.log.info(
                f"Waiting for engines to connect and initialize "
                f"({self._config.timeout_connection}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._await_engines_connected():
                self.kernel.log.warning(
                    f"Timed out ({self._config.timeout_connection}s) waiting for engines to connect and initialize."
                    f"\nStatus"
                    f"\n------"
                    f"\nDataEngine.check_connected() == {self.kernel.data_engine.check_connected()}"
                    f"\nExecEngine.check_connected() == {self.kernel.exec_engine.check_connected()}"
                )
                return
            self.kernel.log.info("Engines connected.", color=LogColor.GREEN)

            # Await execution state reconciliation
            self.kernel.log.info(
                f"Waiting for execution state to reconcile "
                f"({self._config.timeout_reconciliation}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self.kernel.exec_engine.reconcile_state(
                timeout_secs=self._config.timeout_reconciliation,
            ):
                self.kernel.log.error("Execution state could not be reconciled.")
                return
            self.kernel.log.info("State reconciled.", color=LogColor.GREEN)

            # Initialize portfolio
            self.kernel.portfolio.initialize_orders()
            self.kernel.portfolio.initialize_positions()

            # Await portfolio initialization
            self.kernel.log.info(
                "Waiting for portfolio to initialize "
                f"({self._config.timeout_portfolio}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._await_portfolio_initialized():
                self.kernel.log.warning(
                    f"Timed out ({self._config.timeout_portfolio}s) waiting for portfolio to initialize."
                    f"\nStatus"
                    f"\n------"
                    f"\nPortfolio.initialized == {self.kernel.portfolio.initialized}"
                )
                return
            self.kernel.log.info("Portfolio initialized.", color=LogColor.GREEN)

            # Start trader and strategies
            self.kernel.trader.start()

            if self._loop.is_running():
                self.kernel.log.info("RUNNING.")
            else:
                self.kernel.log.warning("Event loop is not running.")

            # Continue to run while engines are running...
            await self.kernel.data_engine.get_run_queue_task()
            await self.kernel.exec_engine.get_run_queue_task()
            await self.kernel.risk_engine.get_run_queue_task()
        except asyncio.CancelledError as ex:
            self.kernel.log.error(str(ex))

    async def _await_engines_connected(self) -> bool:
        # - The data engine clients will be set connected when all
        # instruments are received and updated with the data engine.
        # - The execution engine clients will be set connected when all
        # accounts are updated and the current order and position status is
        # reconciled.
        # Thus any delay here will be due to blocking network I/O.
        seconds = self._config.timeout_connection
        timeout: timedelta = self.kernel.clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self.kernel.clock.utc_now() >= timeout:
                return False
            if not self.kernel.data_engine.check_connected():
                continue
            if not self.kernel.exec_engine.check_connected():
                continue
            break

        return True  # Engines connected

    async def _await_portfolio_initialized(self) -> bool:
        # - The portfolio will be set initialized when all margin and unrealized
        # PnL calculations are completed (maybe waiting on first quotes).
        # Thus any delay here will be due to blocking network I/O.
        seconds = self._config.timeout_portfolio
        timeout: timedelta = self.kernel.clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self.kernel.clock.utc_now() >= timeout:
                return False
            if not self.kernel.portfolio.initialized:
                continue
            break

        return True  # Portfolio initialized

    async def _stop(self) -> None:
        self._is_stopping = True
        self.kernel.log.info("STOPPING...")

        if self.kernel.trader.is_running:
            self.kernel.trader.stop()
            self.kernel.log.info(
                f"Awaiting residual state ({self._config.check_residuals_delay}s delay)...",
                color=LogColor.BLUE,
            )
            await asyncio.sleep(self._config.check_residuals_delay)
            self.kernel.trader.check_residuals()

        if self._config.save_strategy_state:
            self.kernel.trader.save()

        # Disconnect all clients
        self.kernel.data_engine.disconnect()
        self.kernel.exec_engine.disconnect()

        if self.kernel.data_engine.is_running:
            self.kernel.data_engine.stop()
        if self.kernel.exec_engine.is_running:
            self.kernel.exec_engine.stop()
        if self.kernel.risk_engine.is_running:
            self.kernel.risk_engine.stop()

        self.kernel.log.info(
            f"Waiting for engines to disconnect "
            f"({self._config.timeout_disconnection}s timeout)...",
            color=LogColor.BLUE,
        )
        if not await self._await_engines_disconnected():
            self.kernel.log.error(
                f"Timed out ({self._config.timeout_disconnection}s) waiting for engines to disconnect."
                f"\nStatus"
                f"\n------"
                f"\nDataEngine.check_disconnected() == {self.kernel.data_engine.check_disconnected()}"
                f"\nExecEngine.check_disconnected() == {self.kernel.exec_engine.check_disconnected()}"
            )

        # Clean up remaining timers
        timer_names = self.kernel.clock.timer_names()
        self.kernel.clock.cancel_timers()

        for name in timer_names:
            self.kernel.log.info(f"Cancelled Timer(name={name}).")

        # Clean up persistence
        for writer in self.persistence_writers:
            writer.close()

        self.kernel.log.info("STOPPED.")
        self._logger.stop()
        self._is_running = False

    async def _await_engines_disconnected(self) -> bool:
        seconds = self._config.timeout_disconnection
        timeout: timedelta = self.kernel.clock.utc_now() + timedelta(seconds=seconds)
        while True:
            await asyncio.sleep(0)
            if self.kernel.clock.utc_now() >= timeout:
                return False
            if not self.kernel.data_engine.check_disconnected():
                continue
            if not self.kernel.exec_engine.check_disconnected():
                continue
            break

        return True  # Engines disconnected

    def _cancel_all_tasks(self) -> None:
        to_cancel = asyncio.tasks.all_tasks(self._loop)
        if not to_cancel:
            self.kernel.log.info("All tasks canceled.")
            return

        for task in to_cancel:
            self.kernel.log.warning(f"Canceling pending task {task}")
            task.cancel()

        if self._loop.is_running():
            self.kernel.log.warning("Event loop still running during `cancel_all_tasks`.")
            return

        finish_all_tasks: asyncio.Future = asyncio.tasks.gather(  # type: ignore
            *to_cancel,
            loop=self._loop,
            return_exceptions=True,
        )
        self._loop.run_until_complete(finish_all_tasks)

        self.kernel.log.debug(f"{finish_all_tasks}")

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
