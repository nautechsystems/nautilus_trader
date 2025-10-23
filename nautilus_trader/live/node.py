# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from collections.abc import Callable
from datetime import timedelta

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.functions import get_event_loop
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core import nautilus_pyo3
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

        if loop is None:
            try:
                loop = asyncio.get_event_loop()
            except RuntimeError:
                loop = get_event_loop()

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

        has_cache_backing = bool(config.cache and config.cache.database)
        has_msgbus_backing = bool(config.message_bus and config.message_bus.database)
        self.kernel.logger.info(f"{has_cache_backing=}", LogColor.BLUE)
        self.kernel.logger.info(f"{has_msgbus_backing=}", LogColor.BLUE)

        self._stream_processors: list[Callable] = []

        # Async tasks
        self._task_streaming: asyncio.Future | None = None

        # State flags
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

    def is_running(self) -> bool:
        """
        Return whether the trading node is running.

        Returns
        -------
        bool

        """
        return self.kernel.is_running()

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

    def add_stream_processor(self, callback: Callable) -> None:
        """
        Add the given stream processor callback.

        Parameters
        ----------
        callback : Callable
            The callback to add.

        """
        self._stream_processors.append(callback)

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

    def run(self, raise_exception: bool = False) -> None:
        """
        Start and run the trading node.

        Parameters
        ----------
        raise_exception : bool, default False
            If runtime exceptions should be re-raised as well as being logged.

        """
        try:
            if self.kernel.loop.is_running():
                task = self.kernel.loop.create_task(self.run_async())
                task.add_done_callback(self._handle_run_task_result)
            else:
                self.kernel.loop.run_until_complete(self.run_async())
        except RuntimeError as e:
            self.kernel.logger.exception("Error on run", e)

            if raise_exception:
                raise e

    def publish_bus_message(self, bus_msg: nautilus_pyo3.BusMessage) -> None:
        """
        Publish bus message on the internal message bus.

        Note the message will not be published externally.

        Parameters
        ----------
        bus_msg : nautilus_pyo3.BusMessage
            The bus message to publish.

        """
        try:
            msg = self.kernel.msgbus_serializer.deserialize(bus_msg.payload)
        except Exception as e:
            self.kernel.logger.error(f"Failed to deserialize bus message: {e}")
            return

        try:
            for processor in self._stream_processors:
                processor(msg)

            if not self.kernel.msgbus.is_streaming_type(type(msg)):
                return  # Type has not been registered for message streaming
        except Exception as e:
            self.kernel.logger.error(f"Failed to process bus message: {e}")
            return

        try:
            self.kernel.msgbus.publish(bus_msg.topic, msg, external_pub=False)
        except Exception as e:
            self.kernel.logger.error(f"Failed to publish bus message: {e}")

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

            await self.kernel.start_async()

            if self.kernel.loop.is_running():
                self.kernel.logger.info("RUNNING")
            else:
                self.kernel.logger.warning("Event loop is not running")

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

            if self._config.message_bus and self._config.message_bus.external_streams:
                streams = self._config.message_bus.external_streams
                self.kernel.logger.info("Starting task: external message streaming", LogColor.BLUE)
                self.kernel.logger.info(f"Listening to streams: {streams}", LogColor.BLUE)
                self._task_streaming = asyncio.ensure_future(
                    self.kernel.msgbus_database.stream(self.publish_bus_message),
                )
                self._task_streaming.add_done_callback(self._handle_streaming_exception)

            await asyncio.gather(*tasks)
        except asyncio.CancelledError as e:
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
        await self.kernel.stop_async()

    def dispose(self) -> None:
        """
        Dispose of the trading node.

        Gracefully shuts down the executor and event loop.

        """
        try:
            timeout = self.kernel.clock.utc_now() + timedelta(
                seconds=self._config.timeout_disconnection,
            )

            while self.kernel.is_running():
                time.sleep(0.1)

                if self.kernel.clock.utc_now() >= timeout:
                    self.kernel.logger.warning(
                        f"Timed out ({self._config.timeout_disconnection}s) waiting for node to stop"
                        f"\nStatus"
                        f"\n------"
                        f"\nDataEngine.check_disconnected() == {self.kernel.data_engine.check_disconnected()}"
                        f"\nExecEngine.check_disconnected() == {self.kernel.exec_engine.check_disconnected()}",
                    )
                    break

            self.kernel.logger.debug("DISPOSING")

            if self._task_streaming:
                self.kernel.logger.info("Canceling task 'streaming'")
                self._task_streaming.cancel()
                self._task_streaming = None

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
                self.kernel.logger.info("Shutting down executor")
                self.kernel.executor.shutdown(wait=True, cancel_futures=True)

            loop = self.kernel.loop

            if not loop.is_closed():
                if loop.is_running():
                    self.kernel.logger.info("Stopping event loop")
                    self.kernel.cancel_all_tasks()
                    loop.stop()
                else:
                    self.kernel.logger.info("Closing event loop")
                    loop.close()
            else:
                self.kernel.logger.info("Event loop already closed (normal with asyncio.run)")
        except (asyncio.CancelledError, RuntimeError) as e:
            self.kernel.logger.exception("Error on dispose", e)
        finally:
            self.kernel.logger.info(f"loop.is_running={self.kernel.loop.is_running()}")
            self.kernel.logger.info(f"loop.is_closed={self.kernel.loop.is_closed()}")
            self.kernel.logger.info("DISPOSED")

    def _handle_run_task_result(self, task: asyncio.Task) -> None:
        try:
            task.result()
        except asyncio.CancelledError:
            return  # Normal control flow
        except BaseException as e:
            self.kernel.logger.exception("Error in run_async task", e)

    def _handle_streaming_exception(self, task: asyncio.Future) -> None:
        try:
            task.result()
        except asyncio.CancelledError:
            return  # Normal control flow
        except BaseException as e:
            self.kernel.logger.exception("Error in external message streaming task", e)

    def _loop_sig_handler(self, sig: signal.Signals) -> None:
        self.kernel.logger.warning(f"Received {sig.name}, shutting down")
        self.stop()
