# -------------------------------------------------------------------------------------------------
# <copyright file="node.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import time
import json
import pymongo
import redis
import msgpack
import zmq

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.account_type cimport account_type_from_string
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.identifiers cimport Venue, AccountId, TraderId
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.common.execution cimport InMemoryExecutionDatabase, ExecutionDatabase
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.logging cimport LoggerAdapter, nautilus_header
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.network.compression cimport CompressorBypass, SnappyCompressor
from nautilus_trader.network.encryption cimport EncryptionSettings
from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.trading.trader cimport Trader
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer, MsgPackEventSerializer
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.live.guid cimport LiveGuidFactory
from nautilus_trader.live.logging cimport LogStore, LiveLogger
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.live.execution_engine cimport RedisExecutionDatabase, LiveExecutionEngine
from nautilus_trader.live.execution_client cimport LiveExecClient


cdef class TradingNode:
    """
    Provides a trading node to control a live Trader instance with a WebSocket API.
    """
    cdef LiveClock _clock
    cdef LiveGuidFactory _guid_factory
    cdef LiveLogger _logger
    cdef LogStore _log_store
    cdef LoggerAdapter _log
    cdef Venue _venue
    cdef object _zmq_context
    cdef ExecutionDatabase _exec_db
    cdef LiveExecutionEngine _exec_engine
    cdef LiveDataClient _data_client
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
            str config_path='config.json',
            list strategies=None):
        """
        Initializes a new instance of the TradingNode class.

        :param config_path: The path to the config file.
        :param strategies: The list of strategies for the internal Trader.
        :raises ValueError: If the config_path is not a valid string.
        """
        if strategies is None:
            strategies = []
        Condition.valid_string(config_path, 'config_path')

        self._clock = LiveClock()
        self._guid_factory = LiveGuidFactory()
        self._zmq_context = zmq.Context(4)

        # Load the configuration from the file specified in config_path
        with open(config_path, 'r') as config_file:
            config = json.load(config_file)

        config_trader = config['trader']
        config_account = config['account']
        config_log = config['logging']
        config_strategy = config['strategy']
        config_messaging = config['messaging']
        config_data = config['data_client']
        config_exec_db = config['exec_database']
        config_exec_client = config['exec_client']

        # Setup identifiers
        self.trader_id = TraderId(
            name=config_trader['name'],
            order_id_tag=config_trader['order_id_tag'])

        self.account_id = AccountId(
            broker=config_account['broker'],
            account_number=config_account['account_number'],
            account_type=account_type_from_string(config_account['account_type']))

        # Setup logging
        self._log_store = LogStore(
            trader_id=self.trader_id,
            host=config_log['host'],
            port=config_log['port'])
        self._logger = LiveLogger(
            name=self.trader_id.value,
            level_console=LogLevel[config_log['log_level_console']],
            level_file=LogLevel[config_log['log_level_file']],
            level_store=LogLevel[config_log['log_level_store']],
            log_thread=True,
            log_to_file=config_log['log_to_file'],
            log_file_path=config_log['log_file_path'],
            clock=self._clock,
            store=self._log_store)
        self._log = LoggerAdapter(component_name=self.__class__.__name__, logger=self._logger)
        self._log_header()
        self._log.info("Starting...")

        # Setup compressor
        compressor_type = config_messaging['compression']
        if compressor_type in ('', 'none'):
            compressor = CompressorBypass()
        elif compressor_type == 'snappy':
            compressor = SnappyCompressor()
        else:
            raise RuntimeError(f"Compressor type {compressor_type} not recognized. "
                               f"Must be either 'none', or 'snappy'.")

        # Setup encryption
        working_directory = os.getcwd()
        keys_dir = os.path.join(working_directory, config_messaging['keys_dir'])
        encryption = EncryptionSettings(
            algorithm=config_messaging['encryption'],
            keys_dir=keys_dir)

        self._venue = Venue(config_data['venue'])
        self._data_client = LiveDataClient(
            trader_id=self.trader_id,
            host=config_data['host'],
            data_server_req_port=config_data['data_server_req_port'],
            data_server_rep_port=config_data['data_server_rep_port'],
            data_server_pub_port=config_data['data_server_pub_port'],
            tick_server_pub_port=config_data['tick_server_pub_port'],
            compressor=compressor,
            encryption=encryption,
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        # TODO: Portfolio currency?
        self.portfolio = Portfolio(
            currency=Currency.USD,
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        self.analyzer = PerformanceAnalyzer()

        if config_exec_db['type'] == 'redis':
            self._exec_db = RedisExecutionDatabase(
                trader_id=self.trader_id,
                host=config_exec_db['host'],
                port=config_exec_db['port'],
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                logger=self._logger)
        else:
            self._exec_db = InMemoryExecutionDatabase(
                trader_id=self.trader_id,
                logger=self._logger)

        self._exec_engine = LiveExecutionEngine(
            trader_id=self.trader_id,
            account_id=self.account_id,
            database=self._exec_db,
            portfolio=self.portfolio,
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        self._exec_client = LiveExecClient(
            exec_engine=self._exec_engine,
            host=config_exec_client['host'],
            command_req_port=config_exec_client['command_req_port'],
            command_req_port=config_exec_client['command_req_port'],
            event_pub_port=config_exec_client['event_pub_port'],
            compressor=compressor,
            encryption=encryption,
            logger=self._logger)

        self._exec_engine.register_client(self._exec_client)

        self.trader = Trader(
            trader_id=self.trader_id,
            account_id=self.account_id,
            strategies=strategies,
            data_client=self._data_client,
            exec_engine=self._exec_engine,
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        Condition.equal(self.trader_id, self.trader.id, 'trader_id', 'trader.id')

        self._check_residuals_delay = config_trader['check_residuals_delay']
        self._load_strategy_state = config_strategy['load_state']
        self._save_strategy_state = config_strategy['save_state']

        if self._load_strategy_state:
            self.trader.load()

        self._log.info("Initialized.")

    cpdef void load_strategies(self, list strategies) except *:
        """
        Load the given strategies into the trading nodes trader.
        """
        Condition.not_empty(strategies, 'strategies')

        self.trader.initialize_strategies(strategies)

    cpdef void connect(self) except *:
        """
        Connect the trading node to its services.
        """
        self._data_client.connect()
        self._exec_client.connect()
        self._data_client.update_instruments(self._venue)

        account_inquiry = AccountInquiry(
            trader_id=self.trader_id,
            account_id=self.account_id,
            command_id=self._guid_factory.generate(),
            command_timestamp=self._clock.time_now())
        self._exec_client.account_inquiry(account_inquiry)

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
        self._data_client.disconnect()
        self._exec_client.disconnect()

    cpdef void dispose(self) except *:
        """
        Dispose of the trading node.
        """
        self.trader.dispose()
        self._data_client.dispose()
        self._exec_client.dispose()

    cdef void _log_header(self) except *:
        nautilus_header(self._log)
        self._log.info(f"redis {redis.__version__}")
        self._log.info(f"pymongo {pymongo.__version__}")
        self._log.info(f"pyzmq {zmq.pyzmq_version()}")
        self._log.info(f"msgpack {msgpack.version[0]}.{msgpack.version[1]}.{msgpack.version[2]}")
        self._log.info("=================================================================")
