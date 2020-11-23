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

import time
import msgpack
import redis

from nautilus_trader.core.correctness cimport Condition
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
from nautilus_trader.serialization.serializers cimport MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers cimport MsgPackEventSerializer
from nautilus_trader.trading.trader cimport Trader
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class TradingNode:
    """
    Provides an asynchronous network node for live trading.
    """
    cdef LiveClock _clock
    cdef UUIDFactory _uuid_factory
    cdef LiveLogger _logger
    cdef LoggerAdapter _log

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
            list strategies not None,
            dict config not None,
    ):
        """
        Initialize a new instance of the TradingNode class.

        Parameters
        ----------
        strategies : list[TradingStrategy]
            The list of strategies for the internal `Trader`.
        config : dict
            The configuration for the trading node.

        """
        if strategies is None:
            strategies = []

        config_trader = config["trader"]
        config_log = config["logging"]
        config_exec_db = config["exec_database"]
        config_strategy = config["strategy"]

        self._clock = LiveClock()
        self._uuid_factory = UUIDFactory()

        # Setup identifiers
        self.trader_id = TraderId(
            name=config_trader["name"],
            tag=config_trader["id_tag"],
        )

        # Setup logging
        self._logger = LiveLogger(
            clock=self._clock,
            name=self.trader_id.value,
            level_console=LogLevelParser.from_string(config_log.get("log_level_console")),
            level_file=LogLevelParser.from_string(config_log.get("log_level_file")),
            level_store=LogLevelParser.from_string(config_log.get("log_level_store")),
            log_thread=config_log.get("log_thread_id", True),
            log_to_file=config_log.get("log_to_file", False),
            log_file_path=config_log.get("log_file_path", ""),
        )

        self._log = LoggerAdapter(component_name=self.__class__.__name__, logger=self._logger)
        self._log_header()
        self._log.info("Starting...")

        # Serializers
        command_serializer = MsgPackCommandSerializer()
        event_serializer = MsgPackEventSerializer()
        header_serializer = MsgPackDictionarySerializer()

        self.portfolio = Portfolio(
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self._data_engine = LiveDataEngine(
            portfolio=self.portfolio,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self.analyzer = PerformanceAnalyzer()

        if config_exec_db["type"] == "redis":
            exec_db = RedisExecutionDatabase(
                trader_id=self.trader_id,
                logger=self._logger,
                command_serializer=command_serializer,
                event_serializer=event_serializer,
                config={
                    "host": config_exec_db["host"],
                    "port": config_exec_db["port"],
                }
            )
        else:
            exec_db = BypassExecutionDatabase(
                trader_id=self.trader_id,
                logger=self._logger,
            )

        self._exec_engine = LiveExecutionEngine(
            database=exec_db,
            portfolio=self.portfolio,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self.trader = Trader(
            trader_id=self.trader_id,
            strategies=strategies,
            data_engine=self._data_engine,
            exec_engine=self._exec_engine,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self._check_residuals_delay = 2.0  # Hard coded delay to await system spool up (refactor)
        self._load_strategy_state = config_strategy.get("load_state", True)
        self._save_strategy_state = config_strategy.get("save_state", True)

        if self._load_strategy_state:
            self.trader.load()

        self._log.info("Initialized.")

    cpdef void load_strategies(self, list strategies) except *:
        """
        Load the given strategies into the trading nodes trader.
        """
        Condition.not_empty(strategies, "strategies")

        self.trader.initialize_strategies(strategies)

    cpdef void connect(self) except *:
        """
        Connect the trading node to its services.
        """
        pass

    cpdef void start(self) except *:
        """
        Start the trading nodes trader.
        """
        self._data_engine.start()
        self._exec_engine.start()
        self.trader.start()

    cpdef void stop(self) except *:
        """
        Stop the trading node.

        After the specified check residuals delay the internal `Trader`
        residuals will be checked. If save strategy is specified then strategy
        states will then be saved.

        """
        self.trader.stop()

        self._log.info("Awaiting residual state...")
        time.sleep(self._check_residuals_delay)
        self.trader.check_residuals()

        if self._save_strategy_state:
            self.trader.save()

        self._data_engine.stop()
        self._exec_engine.stop()

    cpdef void disconnect(self) except *:
        """
        Disconnect the trading node from its services.
        """
        pass

    cpdef void dispose(self) except *:
        """
        Dispose of the trading node.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.
        """
        self._log.info("Disposing resources...")

        # time.sleep(1.0)  # Hard coded delay to await graceful disconnection (refactor)

        self.trader.dispose()
        self._data_engine.dispose()
        self._exec_engine.dispose()

        self._log.info("Disposed.")

    cdef void _log_header(self) except *:
        nautilus_header(self._log)
        self._log.info(f"redis {redis.__version__}")
        self._log.info(f"msgpack {msgpack.version[0]}.{msgpack.version[1]}.{msgpack.version[2]}")
        self._log.info("=================================================================")
