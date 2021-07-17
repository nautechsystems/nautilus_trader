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
from datetime import timedelta
import platform
import signal
import sys
import time
from typing import Dict, List
import warnings

import msgpack
import redis

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import nautilus_header
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.infrastructure.cache import RedisCacheDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.node_builder import TradingNodeBuilder
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.serialization.msgpack.serializer import MsgPackCommandSerializer
from nautilus_trader.serialization.msgpack.serializer import MsgPackEventSerializer
from nautilus_trader.serialization.msgpack.serializer import MsgPackInstrumentSerializer
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.trading.trader import Trader


try:
    import uvloop

    asyncio.set_event_loop_policy(uvloop.EventLoopPolicy())
    uvloop_version = uvloop.__version__
except ImportError:
    uvloop_version = None
    warnings.warn("uvloop is not available.")


class TradingNode:
    """
    Provides an asynchronous network node for live trading.
    """

    def __init__(
        self,
        strategies: List[TradingStrategy],
        config: Dict[str, Dict[str, object]],
    ):
        """
        Initialize a new instance of the TradingNode class.

        Parameters
        ----------
        strategies : list[TradingStrategy]
            The list of strategies to run on the trading node.
        config : dict[str, dict[str, object]]
            The configuration for the trading node.

        Raises
        ------
        ValueError
            If strategies is None or empty.
        ValueError
            If config is None or empty.

        """
        PyCondition.not_none(strategies, "strategies")
        PyCondition.not_none(config, "config")
        PyCondition.not_empty(strategies, "strategies")
        PyCondition.not_empty(config, "config")

        self._config = config

        # Extract configs
        config_trader = config.get("trader", {})
        config_system = config.get("system", {})
        config_log = config.get("logging", {})
        config_db = config.get("database", {})
        config_cache = config.get("cache", {})
        config_data = config.get("data_engine", {})
        config_risk = config.get("risk_engine", {})
        config_exec = config.get("exec_engine", {})
        config_strategy = config.get("strategy", {})

        # System config
        self._timeout_connection: float = config_system.get("timeout_connection", 5.0)  # type: ignore
        self._timeout_reconciliation: float = config_system.get("timeout_reconciliation", 10.0)  # type: ignore
        self._timeout_portfolio: float = config_system.get("timeout_portfolio", 10.0)  # type: ignore
        self._timeout_disconnection: float = config_system.get("timeout_disconnection", 5.0)  # type: ignore
        self._check_residuals_delay: float = config_system.get("check_residuals_delay", 5.0)  # type: ignore
        self._load_strategy_state: bool = config_strategy.get("load_state", True)  # type: ignore
        self._save_strategy_state: bool = config_strategy.get("save_state", True)  # type: ignore

        # Setup loop
        self._loop = asyncio.get_event_loop()
        self._executor = concurrent.futures.ThreadPoolExecutor()
        self._loop.set_default_executor(self._executor)
        self._loop.set_debug(bool(config_system.get("loop_debug", False)))

        # Components
        self._clock = LiveClock(loop=self._loop)
        self._uuid_factory = UUIDFactory()
        self.system_id = self._uuid_factory.generate()
        self.created_time = self._clock.utc_now()
        self._is_running = False

        # Setup identifiers
        self.trader_id = TraderId(
            f"{config_trader['name']}-{config_trader['id_tag']}",
        )

        # Setup logging
        level_stdout = LogLevelParser.from_str_py(config_log.get("level_stdout"))

        self._logger = LiveLogger(
            loop=self._loop,
            clock=self._clock,
            trader_id=self.trader_id,
            system_id=self.system_id,
            level_stdout=level_stdout,
        )

        self._log = LoggerAdapter(
            component=self.__class__.__name__,
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
        if config_db["type"] == "in-memory":
            cache_db = None
        elif config_db["type"] == "redis":
            cache_db = RedisCacheDatabase(
                trader_id=self.trader_id,
                logger=self._logger,
                instrument_serializer=MsgPackInstrumentSerializer(),
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                config={
                    "host": config_db["host"],
                    "port": config_db["port"],
                },
            )
        else:
            raise ValueError(
                "The cache_db_type in the configuration is unrecognized, "
                "can one of {{'in-memory', 'redis'}}.",
            )

        self._msgbus = MessageBus(
            clock=self._clock,
            logger=self._logger,
        )

        cache = Cache(
            database=cache_db,
            logger=self._logger,
            config=config_cache,
        )

        self.portfolio = Portfolio(
            msgbus=self._msgbus,
            cache=cache,
            clock=self._clock,
            logger=self._logger,
        )

        self._data_engine = LiveDataEngine(
            loop=self._loop,
            portfolio=self.portfolio,
            cache=cache,
            clock=self._clock,
            logger=self._logger,
            config=config_data,
        )

        self._exec_engine = LiveExecutionEngine(
            loop=self._loop,
            trader_id=self.trader_id,
            msgbus=self._msgbus,
            cache=cache,
            clock=self._clock,
            logger=self._logger,
            config=config_exec,
        )
        self._exec_engine.load_cache()

        self._risk_engine = LiveRiskEngine(
            loop=self._loop,
            exec_engine=self._exec_engine,
            msgbus=self._msgbus,
            cache=cache,
            clock=self._clock,
            logger=self._logger,
            config=config_risk,
        )

        self.trader = Trader(
            trader_id=self.trader_id,
            strategies=strategies,
            msgbus=self._msgbus,
            portfolio=self.portfolio,
            data_engine=self._data_engine,
            risk_engine=self._risk_engine,
            exec_engine=self._exec_engine,
            clock=self._clock,
            logger=self._logger,
        )

        if self._load_strategy_state:
            self.trader.load()

        self._builder = TradingNodeBuilder(
            data_engine=self._data_engine,
            exec_engine=self._exec_engine,
            clock=self._clock,
            logger=self._logger,
            log=self._log,
        )

        self._log.info("state=INITIALIZED.")
        self.time_to_initialize = self._clock.delta(self.created_time)
        self._log.info(f"Initialized in {self.time_to_initialize.total_seconds():.3f}s.")

        self._is_built = False

    @property
    def is_running(self) -> bool:
        """
        If the trading node is running.

        Returns
        -------
        bool
            True if running, else False.

        """
        return self._is_running

    @property
    def is_built(self) -> bool:
        """
        If the trading node clients are built.

        Returns
        -------
        bool
            True if built, else False.

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

    def add_data_client_factory(self, name, factory):
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

    def add_exec_client_factory(self, name, factory):
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

        self._builder.build_data_clients(self._config.get("data_clients"))
        self._builder.build_exec_clients(self._config.get("exec_clients"))
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
            timeout = self._clock.utc_now() + timedelta(seconds=self._timeout_disconnection)
            while self._is_running:
                time.sleep(0.1)
                if self._clock.utc_now() >= timeout:
                    self._log.warning(
                        f"Timed out ({self._timeout_disconnection}s) waiting for node to stop."
                        f"\nStatus"
                        f"\n------"
                        f"\nDataEngine.check_disconnected() == {self._data_engine.check_disconnected()}"
                        f"\nExecEngine.check_disconnected() == {self._exec_engine.check_disconnected()}"
                    )
                    break

            self._log.info("state=DISPOSING...")

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

            self._log.info("state=DISPOSED.")

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
            self._log.info("state=STARTING...")
            self._is_running = True

            # Start system
            self._logger.start()
            self._data_engine.start()
            self._exec_engine.start()
            self._risk_engine.start()

            # Await engine connection and initialization
            self._log.info(
                f"Waiting for engines to connect and initialize "
                f"({self._timeout_connection}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._await_engines_connected():
                self._log.warning(
                    f"Timed out ({self._timeout_connection}s) waiting for engines to connect and initialize."
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
                f"({self._timeout_reconciliation}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._exec_engine.reconcile_state(
                timeout_secs=self._timeout_reconciliation,
            ):
                self._log.warning(
                    f"Timed out ({self._timeout_reconciliation}s) waiting for "
                    f"execution state to reconcile."
                )
                return
            self._log.info("State reconciled.", color=LogColor.GREEN)

            # Initialize portfolio
            self.portfolio.initialize_orders()
            self.portfolio.initialize_positions()

            # Await portfolio initialization
            self._log.info(
                "Waiting for portfolio to initialize " f"({self._timeout_portfolio}s timeout)...",
                color=LogColor.BLUE,
            )
            if not await self._await_portfolio_initialized():
                self._log.warning(
                    f"Timed out ({self._timeout_portfolio}s) waiting for portfolio to initialize."
                    f"\nStatus"
                    f"\n------"
                    f"\nPortfolio.initialized == {self.portfolio.initialized}"
                )
                return
            self._log.info("Portfolio initialized.", color=LogColor.GREEN)

            # Update portfolio
            for account in self._exec_engine.cache.accounts():
                self.portfolio.register_account(account)

            # Start trader and strategies
            self.trader.start()

            if self._loop.is_running():
                self._log.info("state=RUNNING.")
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
        seconds = self._timeout_connection
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
        seconds = self._timeout_portfolio
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
        self._log.info("state=STOPPING...")

        if self.trader.state == ComponentState.RUNNING:
            self.trader.stop()
            self._log.info(
                f"Awaiting residual state ({self._check_residuals_delay}s delay)...",
                color=LogColor.BLUE,
            )
            await asyncio.sleep(self._check_residuals_delay)
            self.trader.check_residuals()

        if self._save_strategy_state:
            self.trader.save()

        if self._data_engine.state == ComponentState.RUNNING:
            self._data_engine.stop()
        if self._exec_engine.state == ComponentState.RUNNING:
            self._exec_engine.stop()
        if self._risk_engine.state == ComponentState.RUNNING:
            self._risk_engine.stop()

        self._log.info(
            f"Waiting for engines to disconnect " f"({self._timeout_disconnection}s timeout)...",
            color=LogColor.BLUE,
        )
        if not await self._await_engines_disconnected():
            self._log.error(
                f"Timed out ({self._timeout_disconnection}s) waiting for engines to disconnect."
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

        self._log.info("state=STOPPED.")
        self._logger.stop()
        self._is_running = False

    async def _await_engines_disconnected(self) -> bool:
        seconds = self._timeout_disconnection
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
            self._log.warning(f"Cancelling pending task {task}")
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

        for task in to_cancel:
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
