# -------------------------------------------------------------------------------------------------
# <copyright file="node.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import json
import uuid
import pymongo
import redis
import msgpack
import zmq

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.common.execution cimport ExecutionDatabase
from nautilus_trader.common.logger import LogLevel
from nautilus_trader.common.logger cimport LoggerAdapter, nautilus_header
from nautilus_trader.model.objects cimport Venue
from nautilus_trader.model.identifiers cimport IdTag, TraderId
from nautilus_trader.trade.trader cimport Trader
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.live.logger cimport LogStore, LiveLogger
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.live.execution cimport RedisExecutionDatabase, LiveExecutionEngine, LiveExecClient


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

    cdef readonly GUID id
    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef readonly Trader trader

    def __init__(
            self,
            str config_path='config.json',
            list strategies=[]):
        """
        Initializes a new instance of the TradingNode class.

        :param config_path: The path to the config file.
        :raises ConditionFailed: If the config_path is not a valid string.
        """
        Condition.valid_string(config_path, 'config_path')

        self._clock = LiveClock()
        self._guid_factory = LiveGuidFactory()
        self._zmq_context = zmq.Context()
        self.id = GUID(uuid.uuid4())

        # Load the configuration from the file specified in config_path
        with open(config_path, 'r') as config_file:
            config = json.load(config_file)

        trader_id = TraderId(config['trader']['trader_id'])

        log_config = config['logging']
        self._log_store = LogStore(trader_id=trader_id, port=log_config['redis_port'])
        self._logger = LiveLogger(
            name=trader_id.value,
            level_console=LogLevel[log_config['log_level_console']],
            level_file=LogLevel[log_config['log_level_file']],
            level_store=LogLevel[log_config['log_level_store']],
            log_thread=True,
            log_to_file=log_config['log_to_file'],
            log_file_path=log_config['log_file_path'],
            clock=self._clock,
            store=self._log_store)
        self._log = LoggerAdapter(component_name=self.__class__.__name__, logger=self._logger)
        self._log_header()
        self._log.info("Starting...")

        data_config = config['dataClient']
        self._data_client = LiveDataClient(
            zmq_context=self._zmq_context,
            venue=Venue(data_config['venue']),
            service_name=data_config['service_name'],
            service_address=data_config['service_address'],
            tick_req_port=data_config['tick_req_port'],
            tick_sub_port=data_config['tick_sub_port'],
            bar_req_port=data_config['bar_req_port'],
            bar_sub_port=data_config['bar_sub_port'],
            inst_req_port=data_config['inst_req_port'],
            inst_sub_port=data_config['inst_sub_port'],
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        self.account = Account()
        self.portfolio = Portfolio(
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        exec_config = config['execClient']

        self._exec_db = RedisExecutionDatabase(trader_id=trader_id, port=exec_config['redis_port'])
        self._exec_engine = LiveExecutionEngine(
            trader_id=trader_id,
            account=self.account,
            portfolio=self.portfolio,
            exec_db=self._exec_db,
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        self._exec_client = LiveExecClient(
            zmq_context=self._zmq_context,
            service_name=exec_config['service_name'],
            service_address=exec_config['service_address'],
            events_topic=exec_config['events_topic'],
            commands_port=exec_config['commands_port'],
            events_port=exec_config['events_port'],
            engine=self._exec_engine,
            logger=self._logger)

        id_tag_trader= IdTag(config['trader']['id_tag_trader'])
        self.trader = Trader(
            trader_id=trader_id,
            id_tag_trader=id_tag_trader,
            strategies=strategies,
            data_client=self._data_client,
            exec_client=self._exec_client,
            account=self.account,
            portfolio=self.portfolio,
            clock=self._clock,
            logger=self._logger)

        self._log.info("Initialized.")

    cpdef void load_strategies(self, list strategies):
        """
        Load the given strategies into the trading nodes trader.
        """
        self.trader.load_strategies(strategies)

    cpdef void connect(self):
        """
        Connect the trading node to its services.
        """
        self._data_client.connect()
        self._exec_client.connect()
        self._data_client.update_instruments()

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
