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
import sys
import time
from datetime import timedelta
from typing import Optional

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common import Environment
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.node_builder import TradingNodeBuilder
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.portfolio.base import PortfolioFacade
from nautilus_trader.system.kernel import NautilusKernel
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
        loop = asyncio.get_event_loop()

        # Build core system kernel
        self.kernel = NautilusKernel(
            environment=Environment.LIVE,
            name=type(self).__name__,
            trader_id=TraderId(config.trader_id),
            cache_config=config.cache or CacheConfig(),
            cache_database_config=config.cache_database or CacheDatabaseConfig(),
            data_config=config.data_engine or LiveDataEngineConfig(),
            risk_config=config.risk_engine or LiveRiskEngineConfig(),
            exec_config=config.exec_engine or LiveExecEngineConfig(),
            streaming_config=config.streaming,
            actor_configs=config.actors,
            strategy_configs=config.strategies,
            loop=loop,
            loop_debug=config.loop_debug,
            loop_sig_callback=self._loop_sig_handler,
            log_level=LogLevelParser.from_str_py(config.log_level.upper()),
        )

        self._builder = TradingNodeBuilder(
            loop=loop,
            data_engine=self.kernel.data_engine,
            exec_engine=self.kernel.exec_engine,
            msgbus=self.kernel.msgbus,
            cache=self.kernel.cache,
            clock=self.kernel.clock,
            logger=self.kernel.logger,
            log=self.kernel.log,
        )

        # Operation flags
        self._is_built = False
        self._is_running = False

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
        Return whether the trading node is running.

        Returns
        -------
        bool

        """
        return self._is_running

    @property
    def is_built(self) -> bool:
        """
        Return whether the trading node clients are built.

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
        return self.kernel.loop

    def get_logger(self) -> LiveLogger:
        """
        Return the logger for the trading node.

        Returns
        -------
        LiveLogger

        """
        return self.kernel.logger

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
            if self.kernel.loop.is_running():
                self.kernel.loop.create_task(self._run())
            else:
                self.kernel.loop.run_until_complete(self._run())
        except RuntimeError as e:
            self.kernel.log.exception("Error on run", e)

    def stop(self) -> None:
        """
        Stop the trading node gracefully.

        After a specified delay the internal `Trader` residuals will be checked.

        If save strategy is specified then strategy states will then be saved.

        """
        try:
            if self.kernel.loop.is_running():
                self.kernel.loop.create_task(self._stop())
            else:
                self.kernel.loop.run_until_complete(self._stop())
        except RuntimeError as e:
            self.kernel.log.exception("Error on stop", e)

    def dispose(self) -> None:  # noqa C901 'TradingNode.dispose' is too complex (11)
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

            if self.kernel.trader.is_running:
                self.kernel.trader.stop()
            if self.kernel.data_engine.is_running:
                self.kernel.data_engine.stop()
            if self.kernel.exec_engine.is_running:
                self.kernel.exec_engine.stop()
            if self.kernel.risk_engine.is_running:
                self.kernel.risk_engine.stop()

            self.kernel.trader.dispose()
            self.kernel.data_engine.dispose()
            self.kernel.exec_engine.dispose()
            self.kernel.risk_engine.dispose()

            # Cleanup writer
            if self.kernel.writer is not None:
                self.kernel.writer.close()

            self.kernel.log.info("Shutting down executor...")
            if sys.version_info >= (3, 9):
                # cancel_futures added in Python 3.9
                self.kernel.executor.shutdown(wait=True, cancel_futures=True)
            else:
                self.kernel.executor.shutdown(wait=True)

            self.kernel.log.info("Stopping event loop...")
            self.kernel.cancel_all_tasks()
            self.kernel.loop.stop()
        except (asyncio.CancelledError, RuntimeError) as e:
            self.kernel.log.exception("Error on dispose", e)
        finally:
            if self.kernel.loop.is_running():
                self.kernel.log.warning("Cannot close a running event loop.")
            else:
                self.kernel.log.info("Closing event loop...")
                self.kernel.loop.close()

            # Check and log if event loop is running
            if self.kernel.loop.is_running():
                self.kernel.log.warning(f"loop.is_running={self.kernel.loop.is_running()}")
            else:
                self.kernel.log.info(f"loop.is_running={self.kernel.loop.is_running()}")

            # Check and log if event loop is closed
            if not self.kernel.loop.is_closed():
                self.kernel.log.warning(f"loop.is_closed={self.kernel.loop.is_closed()}")
            else:
                self.kernel.log.info(f"loop.is_closed={self.kernel.loop.is_closed()}")

            self.kernel.log.info("DISPOSED.")

    def _loop_sig_handler(self, sig) -> None:
        self.kernel.log.warning(f"Received {sig!s}, shutting down...")
        self.stop()

    async def _run(self) -> None:
        try:
            self.kernel.log.info("STARTING...")
            self._is_running = True

            # Start system
            self.kernel.logger.start()
            self.kernel.data_engine.start()
            self.kernel.exec_engine.start()
            self.kernel.risk_engine.start()

            # Connect all clients
            self.kernel.data_engine.connect()
            self.kernel.exec_engine.connect()

            # Await engine connection and initialization
            self.kernel.log.info(
                f"Awaiting engine connections and initializations "
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
                f"Awaiting execution state reconciliation "
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
                "Awaiting portfolio initialization "
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

            if self.kernel.loop.is_running():
                self.kernel.log.info("RUNNING.")
            else:
                self.kernel.log.warning("Event loop is not running.")

            # Continue to run while engines are running...
            await self.kernel.data_engine.get_run_queue_task()
            await self.kernel.exec_engine.get_run_queue_task()
            await self.kernel.risk_engine.get_run_queue_task()
        except asyncio.CancelledError as e:
            self.kernel.log.error(str(e))

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
                f"Awaiting post stop ({self._config.timeout_post_stop}s timeout)...",
                color=LogColor.BLUE,
            )
            await asyncio.sleep(self._config.timeout_post_stop)
            self.kernel.trader.check_residuals()

        if self._config.save_state:
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
            f"Awaiting engine disconnections "
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
        timer_names = self.kernel.clock.timer_names
        self.kernel.clock.cancel_timers()

        for name in timer_names:
            self.kernel.log.info(f"Cancelled Timer(name={name}).")

        # Flush writer
        if self.kernel.writer is not None:
            self.kernel.writer.flush()

        self.kernel.log.info("STOPPED.")
        self.kernel.logger.stop()
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
