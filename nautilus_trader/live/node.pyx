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

import json
import os
import time

import lz4
import msgpack
import pymongo
import redis
import zmq

from nautilus_trader.common.execution_database cimport ExecutionDatabase
from nautilus_trader.common.execution_database cimport InMemoryExecutionDatabase
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.account_type cimport account_type_from_string
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue

from nautilus_trader.common.logging import LogLevel  # import for parsing config

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport nautilus_header
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_client cimport LiveExecClient
from nautilus_trader.live.execution_database cimport RedisExecutionDatabase
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.live.factories cimport LiveUUIDFactory
from nautilus_trader.live.logging cimport LiveLogger
from nautilus_trader.live.logging cimport LogStore
from nautilus_trader.network.compression cimport BypassCompressor
from nautilus_trader.network.compression cimport LZ4Compressor
from nautilus_trader.network.encryption cimport EncryptionSettings
from nautilus_trader.serialization.data cimport BsonDataSerializer
from nautilus_trader.serialization.data cimport BsonInstrumentSerializer
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer
from nautilus_trader.serialization.serializers cimport MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers cimport MsgPackEventSerializer
from nautilus_trader.serialization.serializers cimport MsgPackRequestSerializer
from nautilus_trader.serialization.serializers cimport MsgPackResponseSerializer
from nautilus_trader.trading.trader cimport Trader


cdef class TradingNode:
    """
    Provides a trading node to host a live trader instance.
    """
    cdef LiveClock _clock
    cdef LiveUUIDFactory _uuid_factory
    cdef LiveLogger _logger
    cdef LogStore _log_store
    cdef LoggerAdapter _log
    cdef Venue _venue
    cdef object _zmq_context
    cdef ExecutionDatabase _exec_db
    cdef LiveExecutionEngine _exec_engine
    cdef LiveDataEngine _data_engine
    cdef LiveExecClient _exec_client

    cdef double _check_residuals_delay
    cdef bint _load_strategy_state
    cdef bint _save_strategy_state

    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly Portfolio portfolio
    cdef readonly PerformanceAnalyzer analyzer
    cdef readonly Trader trader

    def __init__(
            self,
            str config_path="config.json",
            list strategies=None,
    ):
        """
        Initialize a new instance of the TradingNode class.

        :param config_path: The path to the config file.
        :param strategies: The list of strategies for the internal Trader.
        :raises ValueError: If the config_path is not a valid string.
        """
        if strategies is None:
            strategies = []
        Condition.valid_string(config_path, "config_path")

        # Load the configuration from the file specified in config_path
        with open(config_path, "r") as config_file:
            config = json.load(config_file)

        config_trader = config["trader"]
        config_account = config["account"]
        config_log = config["logging"]
        config_strategy = config["strategy"]
        config_messaging = config["messaging"]
        config_data = config["data_client"]
        config_exec_db = config["exec_database"]
        config_exec_client = config["exec_client"]

        self._clock = LiveClock()
        self._uuid_factory = LiveUUIDFactory()
        self._zmq_context = zmq.Context(io_threads=int(config_messaging["zmq_threads"]))

        # Setup identifiers
        self.trader_id = TraderId(
            name=config_trader["name"],
            order_id_tag=config_trader["id_tag"],
        )

        self.account_id = AccountId(
            broker=config_account["broker"],
            account_number=config_account["account_number"],
            account_type=account_type_from_string(config_account["account_type"]),
        )

        # Setup logging
        self._log_store = LogStore(trader_id=self.trader_id)

        self._logger = LiveLogger(
            clock=self._clock,
            name=self.trader_id.value,
            level_console=LogLevel[config_log["log_level_console"]],
            level_file=LogLevel[config_log["log_level_file"]],
            level_store=LogLevel[config_log["log_level_store"]],
            log_thread=True,
            log_to_file=config_log["log_to_file"],
            log_file_path=config_log["log_file_path"],
            store=self._log_store,
        )

        self._log = LoggerAdapter(component_name=self.__class__.__name__, logger=self._logger)
        self._log_header()
        self._log.info("Starting...")

        # Setup compressor
        compressor_type = config_messaging["compression"]
        if compressor_type in ("", "none"):
            compressor = BypassCompressor()
        elif compressor_type == "lz4":
            compressor = LZ4Compressor()
        else:
            raise RuntimeError(f"Compressor type {compressor_type} not recognized. "
                               f"Must be either 'none', or 'lz4'.")

        # Setup encryption
        working_directory = os.getcwd()
        keys_dir = os.path.join(working_directory, config_messaging["keys_dir"])
        encryption = EncryptionSettings(
            algorithm=config_messaging["encryption"],
            keys_dir=keys_dir)

        # Serializers
        command_serializer = MsgPackCommandSerializer()
        event_serializer = MsgPackEventSerializer()
        header_serializer = MsgPackDictionarySerializer()
        request_serializer = MsgPackRequestSerializer()
        response_serializer = MsgPackResponseSerializer()
        data_serializer = BsonDataSerializer()
        instrument_serializer = BsonInstrumentSerializer()

        self._venue = Venue(config_data["venue"])

        self._data_engine = LiveDataEngine(
            trader_id=self.trader_id,
            host=config_data["host"],
            data_req_port=config_data["data_req_port"],
            data_res_port=config_data["data_res_port"],
            data_pub_port=config_data["data_pub_port"],
            tick_pub_port=config_data["tick_pub_port"],
            compressor=compressor,
            encryption=encryption,
            header_serializer=header_serializer,
            request_serializer=request_serializer,
            response_serializer=response_serializer,
            data_serializer=data_serializer,
            instrument_serializer=instrument_serializer,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self.portfolio = Portfolio(
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self.analyzer = PerformanceAnalyzer()

        if config_exec_db["type"] == "redis":
            self._exec_db = RedisExecutionDatabase(
                trader_id=self.trader_id,
                host=config_exec_db["host"],
                port=config_exec_db["port"],
                command_serializer=command_serializer,
                event_serializer=event_serializer,
                logger=self._logger,
            )
        else:
            self._exec_db = InMemoryExecutionDatabase(
                trader_id=self.trader_id,
                logger=self._logger,
            )

        self._exec_engine = LiveExecutionEngine(
            trader_id=self.trader_id,
            account_id=self.account_id,
            database=self._exec_db,
            portfolio=self.portfolio,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self._exec_client = LiveExecClient(
            exec_engine=self._exec_engine,
            host=config_exec_client["host"],
            command_req_port=config_exec_client["command_req_port"],
            command_res_port=config_exec_client["command_res_port"],
            event_pub_port=config_exec_client["event_pub_port"],
            compressor=compressor,
            encryption=encryption,
            command_serializer=command_serializer,
            header_serializer=header_serializer,
            request_serializer=request_serializer,
            response_serializer=response_serializer,
            event_serializer=event_serializer,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger,
        )

        self._exec_engine.register_client(self._exec_client)

        self.trader = Trader(
            trader_id=self.trader_id,
            account_id=self.account_id,
            strategies=strategies,
            data_client=self._data_engine,
            exec_engine=self._exec_engine,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
            logger=self._logger)

        Condition.equal(self.trader_id, self.trader.id, "trader_id", "trader.id")

        self._check_residuals_delay = 2.0  # Hard coded delay to await system spool up (refactor)
        self._load_strategy_state = config_strategy["load_state"]
        self._save_strategy_state = config_strategy["save_state"]

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
        self._data_engine.connect()
        self._exec_client.connect()
        self._data_engine.update_instruments(self._venue)

        account_inquiry = AccountInquiry(
            trader_id=self.trader_id,
            account_id=self.account_id,
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._exec_client.account_inquiry(account_inquiry)
        time.sleep(0.5)  # Hard coded delay to await instruments and account updates (refactor)

    cpdef void start(self) except *:
        """
        Start the trading nodes trader.
        """
        self.trader.start()

    cpdef void stop(self) except *:
        """
        Stop the trading nodes trader. After the specified check residuals delay
        the traders residuals will be checked. If save strategy is specified
        then strategy states will then be saved.
        """
        self.trader.stop()

        time.sleep(self._check_residuals_delay)
        self.trader.check_residuals()

        if self._save_strategy_state:
            self.trader.save()

    cpdef void disconnect(self) except *:
        """
        Disconnect the trading node from its services.
        """
        self._data_engine.disconnect()
        self._exec_client.disconnect()

    cpdef void dispose(self) except *:
        """
        Dispose of the trading node.
        """
        self._log.info("Disposing resources...")

        time.sleep(1.0)  # Hard coded delay to await graceful disconnection (refactor)

        self.trader.dispose()
        self._data_engine.dispose()
        self._exec_client.dispose()

        self._log.info("Disposed.")

    cdef void _log_header(self) except *:
        nautilus_header(self._log)
        self._log.info(f"redis {redis.__version__}")
        self._log.info(f"pymongo {pymongo.__version__}")
        self._log.info(f"pyzmq {zmq.pyzmq_version()}")
        self._log.info(f"msgpack {msgpack.version[0]}.{msgpack.version[1]}.{msgpack.version[2]}")
        self._log.info(f"lz4 {lz4.__version__}")
        self._log.info("=================================================================")
