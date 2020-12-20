# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
from asyncio import tasks
import signal
import time
from typing import Dict, List

import msgpack
import redis

from nautilus_trader.adapters.binance.data import BinanceDataClient
from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import nautilus_header
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.live.data import LiveDataEngine
from nautilus_trader.live.execution import LiveExecutionEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.redis.execution import RedisExecutionDatabase
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.trading.trader import Trader


try:
    import uvloop
    uvloop_version = uvloop.__version__
except ImportError:
    uvloop_version = None


class TradingNode:
    """
    Provides an asynchronous network node for live trading.
    """

    def __init__(
        self,
        strategies: List[TradingStrategy],
        config: Dict[str, object],
    ):
        """
        Initialize a new instance of the TradingNode class.

        Parameters
        ----------
        strategies : list[TradingStrategy]
            The list of strategies to run on the trading node.
        config : dict[str, object]
            The configuration for the trading node.

        """
        if strategies is None:
            strategies = []

        config_trader = config.get("trader", {})
        config_log = config.get("logging", {})
        config_exec_db = config.get("exec_database", {})
        config_strategy = config.get("strategy", {})
        config_data_clients = config.get("data_clients", {})

        self._clock = LiveClock()
        self._uuid_factory = UUIDFactory()
        self._loop = asyncio.get_event_loop()
        # self._loop.set_debug(True)  # TODO: Development

        # Setup identifiers
        self.trader_id = TraderId(
            name=config_trader["name"],
            tag=config_trader["id_tag"],
        )

        # Setup logging
        logger = LiveLogger(
            clock=self._clock,
            name=self.trader_id.value,
            level_console=LogLevelParser.from_str_py(config_log.get("log_level_console")),
            level_file=LogLevelParser.from_str_py(config_log.get("log_level_file")),
            level_store=LogLevelParser.from_str_py(config_log.get("log_level_store")),
            log_thread=config_log.get("log_thread_id", True),
            log_to_file=config_log.get("log_to_file", False),
            log_file_path=config_log.get("log_file_path", ""),
        )

        self._log = LoggerAdapter(component_name=self.__class__.__name__, logger=logger)
        self._log_header()
        self._log.info("Building...")

        self.portfolio = Portfolio(
            clock=self._clock,
            logger=logger,
        )

        self._data_engine = LiveDataEngine(
            loop=self._loop,
            portfolio=self.portfolio,
            clock=self._clock,
            logger=logger,
        )

        self.portfolio.register_cache(self._data_engine.cache)
        self.analyzer = PerformanceAnalyzer()

        if config_exec_db["type"] == "redis":
            exec_db = RedisExecutionDatabase(
                trader_id=self.trader_id,
                logger=logger,
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                config={
                    "host": config_exec_db["host"],
                    "port": config_exec_db["port"],
                }
            )
        else:
            exec_db = BypassExecutionDatabase(
                trader_id=self.trader_id,
                logger=logger,
            )

        self._exec_engine = LiveExecutionEngine(
            loop=self._loop,
            database=exec_db,
            portfolio=self.portfolio,
            clock=self._clock,
            logger=logger,
        )

        self._exec_engine.load_cache()
        self._setup_data_clients(config_data_clients, logger)

        self.trader = Trader(
            trader_id=self.trader_id,
            strategies=strategies,
            data_engine=self._data_engine,
            exec_engine=self._exec_engine,
            clock=self._clock,
            logger=logger,
        )

        self._check_residuals_delay = 2.0  # Hard coded delay (refactor)
        self._load_strategy_state = config_strategy.get("load_state", True)
        self._save_strategy_state = config_strategy.get("save_state", True)

        if self._load_strategy_state:
            self.trader.load()

        self._setup_loop()
        self._log.info("state=INITIALIZED.")

    def start(self):
        """
        Start the trading node.
        """
        self._loop.create_task(self._run())
        self._loop.run_forever()

    def stop(self):
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

    def dispose(self):
        """
        Dispose of the trading node.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.

        """
        if self._loop.is_running():
            self._loop.create_task(self._dispose())
        else:
            self._loop.run_until_complete(self._dispose())

    def _log_header(self):
        nautilus_header(self._log)
        self._log.info(f"redis {redis.__version__}")
        self._log.info(f"msgpack {msgpack.version[0]}.{msgpack.version[1]}.{msgpack.version[2]}")
        if uvloop_version:
            self._log.info(f"uvloop {uvloop_version}")
        self._log.info("=================================================================")

    def _setup_data_clients(self, config, logger):
        # TODO: DataClientFactory
        for key, value in config.items():
            credentials = {
                "api_key": config.get("api_key"),
                "api_secret": config.get("api_secret"),
            }
            client = BinanceDataClient(
                credentials=credentials,
                engine=self._data_engine,
                clock=self._clock,
                logger=logger,
            )

            self._data_engine.register_client(client)

    def _setup_loop(self):
        signal.signal(signal.SIGINT, signal.SIG_DFL)
        signals = (signal.SIGTERM, signal.SIGINT)
        for sig in signals:
            self._loop.add_signal_handler(sig, self._loop_sig_handler, sig)
        self._log.debug(f"Event loop {signals} handling setup.")

    def _loop_sig_handler(self, sig):
        self._loop.stop()
        self._log.warning(f"Received {sig!s}, shutting down...")

        # Remove signal handler so shutdown sequence is triggered only once
        self._loop.remove_signal_handler(signal.SIGTERM)
        self._loop.add_signal_handler(signal.SIGINT, lambda: None)

        self.stop()
        self.dispose()

    async def _run(self):
        self._log.info("state=STARTING...")

        self._data_engine.start()
        self._exec_engine.start()

        # Allow engines time to spool up
        await asyncio.sleep(0.5)
        self.trader.start()

        if self._loop.is_running():
            self._log.info("state=RUNNING.")
        else:
            self._log.warning("Event loop is not running.")

        # Continue to run while engines are running
        await asyncio.gather(
            self._data_engine.get_run_task(),
            self._exec_engine.get_run_task(),
        )

    async def _stop(self):
        self._log.info("state=STOPPING...")
        self.trader.stop()

        self._log.info("Awaiting residual state...")
        time.sleep(self._check_residuals_delay)

        # TODO: Refactor shutdown - check completely flat before stopping engines
        self.trader.check_residuals()

        if self._save_strategy_state:
            self.trader.save()

        self._data_engine.stop()
        self._exec_engine.stop()

        self._log.info("state=STOPPED.")

    async def _dispose(self):
        self._log.info("state=DISPOSING...")

        self.trader.dispose()
        self._data_engine.dispose()
        self._exec_engine.dispose()

        try:
            self._loop.call_soon(self._loop.stop)
            self._loop.call_soon(self._cancel_all_tasks)
        except RuntimeError as ex:
            self._log.exception(ex)
        finally:
            await self._loop.shutdown_asyncgens()

            if self._loop.is_running():
                self._log.warning("Cannot close running event loop.")
            else:
                self._log.info("Closing event loop...")
                self._loop.close()

            self._log.info(f"loop.is_closed={self._loop.is_closed()}")
            self._log.info("state=DISPOSED.")

    def _cancel_all_tasks(self):
        to_cancel = tasks.all_tasks(self._loop)
        if not to_cancel:
            return

        for task in to_cancel:
            task.cancel()

        self._loop.run_until_complete(
            tasks.gather(*to_cancel, loop=self._loop, return_exceptions=True))

        for task in to_cancel:
            if task.cancelled():
                continue
            if task.exception() is not None:
                self._loop.call_exception_handler({
                    'message': 'unhandled exception during asyncio.run() shutdown',
                    'exception': task.exception(),
                    'task': task,
                })

        self._log.info("Cancelled all tasks.")
