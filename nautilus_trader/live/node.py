# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
import signal
import time
from datetime import timedelta

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
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
    loop : asyncio.AbstractEventLoop, optional
        The event loop for the node.
        If ``None`` then will get the running event loop internally.

    """

    def __init__(
        self,
        config: TradingNodeConfig | None = None,
        loop: asyncio.AbstractEventLoop | None = None,
    ) -> None:
        if config is None:
            config = TradingNodeConfig()
        PyCondition.not_none(config, "config")
        PyCondition.type(config, TradingNodeConfig, "config")

        self._config: TradingNodeConfig = config

        loop = loop or asyncio.get_event_loop()

        self.kernel = NautilusKernel(
            name=type(self).__name__,
            config=config,
            loop=loop,
            loop_sig_callback=self._loop_sig_handler,
        )

        self._builder = TradingNodeBuilder(
            loop=loop,
            data_engine=self.kernel.data_engine,
            exec_engine=self.kernel.exec_engine,
            portfolio=self.kernel.portfolio,
            msgbus=self.kernel.msgbus,
            cache=self.kernel.cache,
            clock=self.kernel.clock,
            logger=self.kernel.logger,
        )

        # Operation flags
        self._is_built = False
        self._is_running = False
        self._has_cache_backing = config.cache and config.cache.database
        self._has_msgbus_backing = config.message_bus and config.message_bus.database

        self.kernel.logger.info(f"{self._has_cache_backing=}", LogColor.BLUE)
        self.kernel.logger.info(f"{self._has_msgbus_backing=}", LogColor.BLUE)

        # Async tasks
        self._task_heartbeats: asyncio.Task | None = None
        self._task_position_snapshots: asyncio.Task | None = None

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

    def get_event_loop(self) -> asyncio.AbstractEventLoop | None:
        """
        Return the event loop of the trading node.

        Returns
        -------
        asyncio.AbstractEventLoop or ``None``

        """
        return self.kernel.loop

    def get_logger(self) -> Logger:
        """
        Return the logger for the trading node.

        Returns
        -------
        Logger

        """
        return self.kernel.logger

    def add_data_client_factory(self, name: str, factory: type[LiveDataClientFactory]) -> None:
        """
        Add the given data client factory to the node.

        Parameters
        ----------
        name : str
            The name of the client factory.
        factory : type[LiveDataClientFactory]
            The factory class to add.

        Raises
        ------
        ValueError
            If `name` is not a valid string.
        KeyError
            If `name` has already been added.

        """
        self._builder.add_data_client_factory(name, factory)

    def add_exec_client_factory(self, name: str, factory: type[LiveExecClientFactory]) -> None:
        """
        Add the given execution client factory to the node.

        Parameters
        ----------
        name : str
            The name of the client factory.
        factory : type[LiveExecutionClientFactory]
            The factory class to add.

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

    def run(self) -> None:
        """
        Start and run the trading node.
        """
        try:
            if self.kernel.loop.is_running():
                self.kernel.loop.create_task(self.run_async())
            else:
                self.kernel.loop.run_until_complete(self.run_async())
        except RuntimeError as e:
            self.kernel.logger.exception("Error on run", e)

    async def run_async(self) -> None:
        """
        Start and run the trading node asynchronously.
        """
        try:
            if not self._is_built:
                raise RuntimeError(
                    "The trading nodes clients have not been built. "
                    "Run `node.build()` prior to start.",
                )

            self._is_running = True
            await self.kernel.start_async()

            if self.kernel.loop.is_running():
                self.kernel.logger.info("RUNNING.")
            else:
                self.kernel.logger.warning("Event loop is not running.")

            # Continue to run while engines are running...
            tasks: list[asyncio.Task] = [
                self.kernel.data_engine.get_cmd_queue_task(),
                self.kernel.data_engine.get_req_queue_task(),
                self.kernel.data_engine.get_res_queue_task(),
                self.kernel.data_engine.get_data_queue_task(),
                self.kernel.risk_engine.get_cmd_queue_task(),
                self.kernel.risk_engine.get_evt_queue_task(),
                self.kernel.exec_engine.get_cmd_queue_task(),
                self.kernel.exec_engine.get_evt_queue_task(),
            ]

            if self._config.heartbeat_interval:
                self._task_heartbeats = asyncio.create_task(
                    self.maintain_heartbeat(self._config.heartbeat_interval),
                )
            if self._config.snapshot_positions_interval:
                self._task_position_snapshots = asyncio.create_task(
                    self.snapshot_open_positions(self._config.snapshot_positions_interval),
                )

            await asyncio.gather(*tasks)
        except asyncio.CancelledError as e:
            self.kernel.logger.error(str(e))

    async def maintain_heartbeat(self, interval: float) -> None:
        """
        Maintain heartbeats at the given `interval` while the node is running.

        Parameters
        ----------
        interval : float
            The interval (seconds) between heartbeats.

        """
        self.kernel.logger.info(
            f"Starting heartbeats at {interval}s intervals...",
            LogColor.BLUE,
        )
        try:
            while True:
                await asyncio.sleep(interval)
                msg = self.kernel.clock.utc_now()
                if self._has_cache_backing:
                    self.cache.heartbeat(msg)
                if self._has_msgbus_backing:
                    self.kernel.msgbus.publish(topic="health:heartbeat", msg=str(msg))
        except asyncio.CancelledError:
            pass
        except Exception as e:
            # Catch-all exceptions for development purposes (unexpected errors)
            self.kernel.logger.error(str(e))

    async def snapshot_open_positions(self, interval: float) -> None:
        """
        Snapshot the state of all open positions at the configured interval.

        Parameters
        ----------
        interval : float
            The interval (seconds) between open position state snapshotting.

        """
        self.kernel.logger.info(
            f"Starting open position snapshots at {interval}s intervals...",
            LogColor.BLUE,
        )
        try:
            while True:
                await asyncio.sleep(interval)
                open_positions = self.kernel.cache.positions_open()
                for position in open_positions:
                    if self._has_cache_backing:
                        self.cache.snapshot_position_state(
                            position=position,
                            ts_snapshot=self.kernel.clock.timestamp_ns(),
                        )
                    if self._has_msgbus_backing:
                        #  TODO: Consolidate this with the cache
                        position_state = position.to_dict()
                        unrealized_pnl = self.kernel.cache.calculate_unrealized_pnl(position)
                        if unrealized_pnl is not None:
                            position_state["unrealized_pnl"] = unrealized_pnl.to_str()
                        self.kernel.msgbus.publish(
                            topic=f"snapshots:positions:{position.id}",
                            msg=self.kernel.msgbus.serializer.serialize(position_state),
                        )
        except asyncio.CancelledError:
            pass
        except Exception as e:
            # Catch-all exceptions for development purposes (unexpected errors)
            self.kernel.logger.error(str(e))

    def stop(self) -> None:
        """
        Stop the trading node gracefully.

        After a specified delay the internal `Trader` residual state will be checked.

        If save strategy is configured, then strategy states will be saved.

        """
        try:
            if self.kernel.loop.is_running():
                self.kernel.loop.create_task(self.stop_async())
            else:
                self.kernel.loop.run_until_complete(self.stop_async())
        except RuntimeError as e:
            self.kernel.logger.exception("Error on stop", e)

    async def stop_async(self) -> None:
        """
        Stop the trading node gracefully, asynchronously.

        After a specified delay the internal `Trader` residual state will be checked.

        If save strategy is configured, then strategy states will be saved.

        """
        if self._task_heartbeats:
            self.kernel.logger.info("Cancelling `task_heartbeats` task...")
            self._task_heartbeats.cancel()
            self._task_heartbeats = None

        if self._task_position_snapshots:
            self.kernel.logger.info("Cancelling `task_position_snapshots` task...")
            self._task_position_snapshots.cancel()
            self._task_position_snapshots = None

        await self.kernel.stop_async()

        self._is_running = False

    def dispose(self) -> None:
        """
        Dispose of the trading node.

        Gracefully shuts down the executor and event loop.

        """
        try:
            timeout = self.kernel.clock.utc_now() + timedelta(
                seconds=self._config.timeout_disconnection,
            )
            while self._is_running:
                time.sleep(0.1)
                if self.kernel.clock.utc_now() >= timeout:
                    self.kernel.logger.warning(
                        f"Timed out ({self._config.timeout_disconnection}s) waiting for node to stop."
                        f"\nStatus"
                        f"\n------"
                        f"\nDataEngine.check_disconnected() == {self.kernel.data_engine.check_disconnected()}"
                        f"\nExecEngine.check_disconnected() == {self.kernel.exec_engine.check_disconnected()}",
                    )
                    break

            self.kernel.logger.debug("DISPOSING...")

            self.kernel.logger.debug(str(self.kernel.data_engine.get_cmd_queue_task()))
            self.kernel.logger.debug(str(self.kernel.data_engine.get_req_queue_task()))
            self.kernel.logger.debug(str(self.kernel.data_engine.get_res_queue_task()))
            self.kernel.logger.debug(str(self.kernel.data_engine.get_data_queue_task()))
            self.kernel.logger.debug(str(self.kernel.exec_engine.get_cmd_queue_task()))
            self.kernel.logger.debug(str(self.kernel.exec_engine.get_evt_queue_task()))
            self.kernel.logger.debug(str(self.kernel.risk_engine.get_cmd_queue_task()))
            self.kernel.logger.debug(str(self.kernel.risk_engine.get_evt_queue_task()))

            self.kernel.dispose()

            if self.kernel.executor:
                self.kernel.logger.info("Shutting down executor...")
                self.kernel.executor.shutdown(wait=True, cancel_futures=True)

            self.kernel.logger.info("Stopping event loop...")
            self.kernel.cancel_all_tasks()
            self.kernel.loop.stop()
        except (asyncio.CancelledError, RuntimeError) as e:
            self.kernel.logger.exception("Error on dispose", e)
        finally:
            if self.kernel.loop.is_running():
                self.kernel.logger.warning("Cannot close a running event loop.")
            else:
                self.kernel.logger.info("Closing event loop...")
                self.kernel.loop.close()

            # Check and log if event loop is running
            if self.kernel.loop.is_running():
                self.kernel.logger.warning(f"loop.is_running={self.kernel.loop.is_running()}")
            else:
                self.kernel.logger.info(f"loop.is_running={self.kernel.loop.is_running()}")

            # Check and log if event loop is closed
            if not self.kernel.loop.is_closed():
                self.kernel.logger.warning(f"loop.is_closed={self.kernel.loop.is_closed()}")
            else:
                self.kernel.logger.info(f"loop.is_closed={self.kernel.loop.is_closed()}")

            self.kernel.logger.info("DISPOSED.")

    def _loop_sig_handler(self, sig: signal.Signals) -> None:
        self.kernel.logger.warning(f"Received {sig!s}, shutting down...")
        self.stop()
