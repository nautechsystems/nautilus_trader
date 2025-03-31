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
from asyncio import Queue
from typing import Final

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.message import Command
from nautilus_trader.core.message import Event
from nautilus_trader.live.enqueue import ThrottledEnqueuer
from nautilus_trader.portfolio.base import PortfolioFacade
from nautilus_trader.risk.engine import RiskEngine


class LiveRiskEngine(RiskEngine):
    """
    Provides a high-performance asynchronous live risk engine.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the engine.
    portfolio : PortfolioFacade
        The portfolio for the engine.
    msgbus : MessageBus
        The message bus for the engine.
    cache : CacheFacade
        The read-only cache for the engine.
    clock : LiveClock
        The clock for the engine.
    config : LiveRiskEngineConfig
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `LiveRiskEngineConfig`.

    """

    _sentinel: Final[None] = None

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: LiveClock,
        config: LiveRiskEngineConfig | None = None,
    ) -> None:
        if config is None:
            config = LiveRiskEngineConfig()
        PyCondition.type(config, LiveRiskEngineConfig, "config")
        super().__init__(
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self._loop: asyncio.AbstractEventLoop = loop
        self._cmd_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._evt_queue: asyncio.Queue = Queue(maxsize=config.qsize)

        self._cmd_enqueuer: ThrottledEnqueuer[Command] = ThrottledEnqueuer(
            qname="cmd_queue",
            queue=self._cmd_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )
        self._evt_enqueuer: ThrottledEnqueuer[Event] = ThrottledEnqueuer(
            qname="evt_queue",
            queue=self._evt_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )

        # Async tasks
        self._cmd_queue_task: asyncio.Task | None = None
        self._evt_queue_task: asyncio.Task | None = None
        self._kill: bool = False

    def get_cmd_queue_task(self) -> asyncio.Task | None:
        """
        Return the internal command queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._cmd_queue_task

    def get_evt_queue_task(self) -> asyncio.Task | None:
        """
        Return the internal event queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._evt_queue_task

    def cmd_qsize(self) -> int:
        """
        Return the number of `Command` messages buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._cmd_queue.qsize()

    def evt_qsize(self) -> int:
        """
        Return the number of `Event` messages buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._evt_queue.qsize()

    # -- COMMANDS -------------------------------------------------------------------------------------

    def kill(self) -> None:
        """
        Kill the engine by abruptly canceling the queue task and calling stop.
        """
        self._log.warning("Killing engine")
        self._kill = True
        self.stop()
        if self._cmd_queue_task:
            self._log.debug(f"Canceling task '{self._cmd_queue_task.get_name()}'")
            self._cmd_queue_task.cancel()
            self._cmd_queue_task = None
        if self._evt_queue_task:
            self._log.debug(f"Canceling task '{self._evt_queue_task.get_name()}'")
            self._evt_queue_task.cancel()
            self._evt_queue_task = None

    def execute(self, command: Command) -> None:
        """
        Execute the given command.

        If the internal queue is at or near capacity, it logs a warning (throttled)
        and schedules an asynchronous `put()` operation. This ensures all messages are
        eventually enqueued and processed without blocking the caller when the queue is full.

        Parameters
        ----------
        command : Command
            The command to execute.

        Warnings
        --------
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        self._cmd_enqueuer.enqueue(command)

    def process(self, event: Event) -> None:
        """
        Process the given event.

        If the internal queue is at or near capacity, it logs a warning (throttled)
        and schedules an asynchronous `put()` operation. This ensures all messages are
        eventually enqueued and processed without blocking the caller when the queue is full.

        Parameters
        ----------
        event : Event
            The event to process.

        """
        self._evt_enqueuer.enqueue(event)

    # -- INTERNAL -------------------------------------------------------------------------------------

    def _enqueue_sentinel(self) -> None:
        self._loop.call_soon_threadsafe(self._cmd_queue.put_nowait, self._sentinel)
        self._loop.call_soon_threadsafe(self._evt_queue.put_nowait, self._sentinel)
        self._log.debug("Sentinel messages placed on queues")

    def _on_start(self) -> None:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running")

        self._cmd_queue_task = self._loop.create_task(self._run_cmd_queue(), name="cmd_queue")
        self._evt_queue_task = self._loop.create_task(self._run_evt_queue(), name="evt_queue")

        self._log.debug(f"Scheduled task '{self._cmd_queue_task.get_name()}'")
        self._log.debug(f"Scheduled task '{self._evt_queue_task.get_name()}'")

    def _on_stop(self) -> None:
        if self._kill:
            return  # Avoids queuing redundant sentinel messages
        # This will stop the queues processing as soon as they see the sentinel message
        self._enqueue_sentinel()

    async def _run_cmd_queue(self) -> None:
        self._log.debug(
            f"Command message queue processing (qsize={self.cmd_qsize()})",
        )
        try:
            while True:
                command: Command | None = await self._cmd_queue.get()
                if command is self._sentinel:
                    break
                self._execute_command(command)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'run_cmd_queue'")
        except Exception as e:
            self._log.exception(f"{e!r}", e)
        finally:
            stopped_msg = "Command message queue stopped"
            if not self._cmd_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.cmd_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)

    async def _run_evt_queue(self) -> None:
        self._log.debug(
            f"Event message queue processing starting (qsize={self.evt_qsize()})",
        )
        try:
            while True:
                event: Event | None = await self._evt_queue.get()
                if event is self._sentinel:
                    break
                self._handle_event(event)
        except asyncio.CancelledError:
            self._log.warning("Canceled task 'run_evt_queue'")
        except Exception as e:
            self._log.exception(f"{e!r}", e)
        finally:
            stopped_msg = "Event message queue stopped"
            if not self._evt_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.evt_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)
