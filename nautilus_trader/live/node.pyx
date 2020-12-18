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
import time
import msgpack
import redis
from asyncio import AbstractEventLoop
from signal import SIGINT, SIGTERM

from nautilus_trader.adapters.binance.data cimport BinanceDataClient
from nautilus_trader.execution.database cimport BypassExecutionDatabase
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.common.logging cimport LogLevelParser
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.live.data cimport LiveDataEngine
from nautilus_trader.live.execution cimport LiveExecutionEngine
from nautilus_trader.redis.execution cimport RedisExecutionDatabase
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer
from nautilus_trader.serialization.serializers cimport MsgPackEventSerializer
from nautilus_trader.trading.trader cimport Trader
from nautilus_trader.trading.portfolio cimport Portfolio

try:
    import uvloop
    uvloop_version = uvloop.__version__
except ImportError:
    uvloop_version = None


cdef class TradingNode:
    """
    Provides an asynchronous network node for live trading.
    """
    cdef LiveClock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log

    cdef object _loop
    cdef LiveExecutionEngine _exec_engine
    cdef LiveDataEngine _data_engine

    cdef double _check_residuals_delay
    cdef bint _load_strategy_state
    cdef bint _save_strategy_state

    cdef readonly TraderId trader_id
    cdef readonly Portfolio portfolio
    cdef readonly PerformanceAnalyzer analyzer
    cdef readonly Trader trader

    def __init__(
        self,
        loop not None: AbstractEventLoop,
        list strategies not None,
        dict config not None,
    ):
        """
        Initialize a new instance of the TradingNode class.

        Parameters
        ----------
        loop : AbstractEventLoop
            The event loop for the trading node.
        strategies : list[TradingStrategy]
            The list of strategies to run on the trading node.
        config : dict
            The configuration for the trading node.

        """
        if strategies is None:
            strategies = []

        cdef dict config_trader = config["trader"]
        cdef dict config_log = config["logging"]
        cdef dict config_exec_db = config["exec_database"]
        cdef dict config_strategy = config["strategy"]
        cdef dict config_data_clients = config["data_clients"]

        self._clock = LiveClock()
        self._uuid_factory = UUIDFactory()
        self._loop = loop

        # Setup identifiers
        self.trader_id = TraderId(
            name=config_trader["name"],
            tag=config_trader["id_tag"],
        )

        # Setup logging
        cdef LiveLogger logger = LiveLogger(
            clock=self._clock,
            name=self.trader_id.value,
            level_console=LogLevelParser.from_str(config_log.get("log_level_console")),
            level_file=LogLevelParser.from_str(config_log.get("log_level_file")),
            level_store=LogLevelParser.from_str(config_log.get("log_level_store")),
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

    def _loop_sig_handler(self, sig):
        self._loop.stop()
        self._log.warning(f"Received {sig!s}, shutting down...")
        self._loop.remove_signal_handler(SIGTERM)
        self._loop.add_signal_handler(SIGINT, lambda: None)

    def _setup_loop(self):
        signals = (SIGTERM, SIGINT)
        for sig in signals:
            self._loop.add_signal_handler(sig, self._loop_sig_handler, sig)
        self._log.debug(f"Event loop {signals} handling setup.")

    def start(self):
        """
        Start the trading node.
        """
        self._log.info("state=STARTING...")

        if self._loop.is_running():
            self._loop.create_task(self._run())
        else:
            self._loop.run_until_complete(self._run())

        self._log.info("state=RUNNING.")

    def stop(self):
        """
        Stop the trading node.

        After a specified delay the internal `Trader` residuals will be checked.

        If save strategy is specified then strategy states will then be saved.

        """
        self._log.info("state=STOPPING...")

        if self._loop.is_running():
            self._loop.create_task(self._shutdown())
        else:
            self._loop.run_until_complete(self._shutdown())

        self._log.info(f"loop.is_running={self._loop.is_running()}")
        self._log.info("state=STOPPED.")

    cpdef void dispose(self) except *:
        """
        Dispose of the trading node.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.

        """
        self._log.info("state=DISPOSING...")

        self.trader.dispose()
        self._data_engine.dispose()
        self._exec_engine.dispose()

        try:
            self._loop.stop()
            self._log.info("Closing event loop...")
            time.sleep(0.1)  # Allow event loop to close (refactor)
            self._loop.close()
            self._log.info(f"loop.is_closed={self._loop.is_closed()}")
        except RuntimeError as ex:
            self._log.exception(ex)

        self._log.info("state=DISPOSED.")
        time.sleep(0.1)  # Allow final logs to print to console (refactor)

    async def _run(self):
        self._log.info(f"loop.is_running={self._loop.is_running()}")

        self._data_engine.start()
        self._exec_engine.start()

        # Allow engines time to spool up
        await asyncio.sleep(0.5)
        self.trader.start()

        # Continue to run loop while engines are running
        await asyncio.gather(
            self._data_engine.get_run_task(),
            self._exec_engine.get_run_task(),
        )

    async def _shutdown(self):
        self.trader.stop()

        await self._await_residuals_and_stop()

    async def _await_residuals_and_stop(self):
        self._log.info("Awaiting residual state...")
        await asyncio.sleep(self._check_residuals_delay)

        # TODO: Refactor shutdown - check completely flat before stopping engines
        self.trader.check_residuals()

        if self._save_strategy_state:
            self.trader.save()

        self._data_engine.stop()
        self._exec_engine.stop()

    cdef void _setup_data_clients(self, dict config, logger):
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

    cdef void _log_header(self) except *:
        nautilus_header(self._log)
        self._log.info(f"redis {redis.__version__}")
        self._log.info(f"msgpack {msgpack.version[0]}.{msgpack.version[1]}.{msgpack.version[2]}")
        if uvloop_version:
            self._log.info(f"uvloop {uvloop_version}")
        self._log.info("=================================================================")
