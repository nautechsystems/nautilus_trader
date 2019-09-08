# -------------------------------------------------------------------------------------------------
# <copyright file="node.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import json
import pymongo
import redis
import msgpack
import zmq

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.account_type cimport account_type_from_string
from nautilus_trader.model.identifiers cimport Venue, AccountId, TraderId
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.common.execution cimport InMemoryExecutionDatabase, ExecutionDatabase
from nautilus_trader.common.logger import LogLevel
from nautilus_trader.common.logger cimport LoggerAdapter, nautilus_header
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.trade.trader cimport Trader
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer, MsgPackEventSerializer
from nautilus_trader.live.logger cimport LogStore, LiveLogger
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.live.execution cimport RedisExecutionDatabase, LiveExecutionEngine, LiveExecClient

from test_kit.stubs import TestStubs

cdef class TradingNode:
    """
    Provides a trading node to control a live Trader instance with a WebSocket API.
    """
    cdef LiveClock _clock
    cdef LiveGuidFactory _guid_factory
    cdef LiveLogger _logger
    cdef LogStore _log_store
    cdef LoggerAdapter _log
    cdef object _zmq_context
    cdef ExecutionDatabase _exec_db
    cdef LiveExecutionEngine _exec_engine
    cdef LiveDataClient _data_client
    cdef LiveExecClient _exec_client

    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly Portfolio portfolio
    cdef readonly Trader trader

    def __init__(
            self,
            str config_path='config.json',
            list strategies=[]):
        """
        Initializes a new instance of the TradingNode class.

        :param config_path: The path to the config file.
        :param strategies: The list of strategies for the internal Trader.
        :raises ConditionFailed: If the config_path is not a valid string.
        """
        Condition.valid_string(config_path, 'config_path')

        self._clock = LiveClock()
        self._guid_factory = LiveGuidFactory()
        self._zmq_context = zmq.Context()

        # Load the configuration from the file specified in config_path
        with open(config_path, 'r') as config_file:
            config = json.load(config_file)
        config_trader = config['trader']
        config_log = config['logging']
        config_data = config['data_client']
        config_account = config['account']
        config_exec_db = config['exec_database']
        config_exec_client = config['exec_client']

        self.trader_id = TraderId(
            name=config_trader['name'],
            order_id_tag=config_trader['order_id_tag'])

        self.account_id = AccountId(
            config_account['broker'],
            config_account['account_number'],
            account_type_from_string(config_account['account_type']))

        self._log_store = LogStore(
            trader_id=self.trader_id,
            host=config_log['redis_host'],
            port=config_log['redis_port'])
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

        self._data_client = LiveDataClient(
            zmq_context=self._zmq_context,
            venue=Venue(config_data['venue']),
            service_name=config_data['service_name'],
            service_address=config_data['service_address'],
            tick_rep_port=config_data['tick_rep_port'],
            tick_pub_port=config_data['tick_pub_port'],
            bar_rep_port=config_data['bar_rep_port'],
            bar_pub_port=config_data['bar_pub_port'],
            inst_rep_port=config_data['inst_rep_port'],
            inst_pub_port=config_data['inst_pub_port'],
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        self.portfolio = Portfolio(
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        if config_exec_db['type'] == 'redis':
            self._exec_db = RedisExecutionDatabase(
                trader_id=self.trader_id,
                host=config_exec_db['redis_host'],
                port=config_exec_db['redis_port'],
                command_serializer=MsgPackCommandSerializer(),
                event_serializer=MsgPackEventSerializer(),
                logger=self._logger)
        else:
            self._exec_db = InMemoryExecutionDatabase(
                trader_id=self.trader_id,
                logger=self._logger)

        self._exec_engine = LiveExecutionEngine(
            database=self._exec_db,
            portfolio=self.portfolio,
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        self._exec_engine.handle_event(TestStubs.account_event(self.account_id))

        self._exec_client = LiveExecClient(
            exec_engine=self._exec_engine,
            zmq_context=self._zmq_context,
            service_name=config_exec_client['service_name'],
            service_address=config_exec_client['service_address'],
            events_topic=config_exec_client['events_topic'],
            commands_port=config_exec_client['commands_port'],
            events_port=config_exec_client['events_port'],
            logger=self._logger)

        self._exec_engine.register_client(self._exec_client)

        self.trader = Trader(
            trader_id=self.trader_id,
            strategies=strategies,
            data_client=self._data_client,
            exec_engine=self._exec_engine,
            clock=self._clock,
            logger=self._logger)

        Condition.equal(self.trader_id, self.trader.id)

        self._log.info("Initialized.")

    cpdef void load_strategies(self, list strategies):
        """
        Load the given strategies into the trading nodes trader.
        """
        self.trader.initialize_strategies(strategies)

    cpdef void connect(self):
        """
        Connect the trading node to its services.
        """
        self._data_client.connect()
        self._exec_client.connect()
        self._data_client.update_instruments()

        account_inquiry = AccountInquiry(
            account_id=self.account_id,
            command_id=self._guid_factory.generate(),
            command_timestamp=self._clock.time_now())
        self._exec_client.account_inquiry(account_inquiry)

    cpdef void start(self):
        """
        Start the trading nodes trader.
        """
        self.trader.start()

    cpdef void stop(self):
        """
        Stop the trading nodes trader.
        """
        self.trader.stop()

    cpdef void disconnect(self):
        """
        Disconnect the trading node from its services.
        """
        self._data_client.disconnect()
        self._exec_client.disconnect()

    cpdef void dispose(self):
        """
        Dispose of the trading node.
        """
        self.trader.dispose()
        self._data_client.dispose()
        self._exec_client.dispose()

    cdef void _log_header(self):
        nautilus_header(self._log)
        self._log.info(f"redis v{redis.__version__}")
        self._log.info(f"pymongo v{pymongo.__version__}")
        self._log.info(f"pyzmq v{zmq.pyzmq_version()}")
        self._log.info(f"msgpack v{msgpack.version[0]}.{msgpack.version[1]}.{msgpack.version[2]}")
        self._log.info("#---------------------------------------------------------------#")
        self._log.info("")
