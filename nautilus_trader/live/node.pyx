# -------------------------------------------------------------------------------------------------
# <copyright file="node.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import json
import logging
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
from nautilus_trader.common.logger cimport LoggerAdapter, nautilus_header
from nautilus_trader.model.objects cimport Venue
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.trade.trader cimport Trader
from nautilus_trader.trade.portfolio cimport Portfolio
from nautilus_trader.live.logger cimport LiveLogger
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.live.execution cimport LiveExecClient
from nautilus_trader.live.stores cimport LogStore, EventStore


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
    cdef LiveDataClient _data_client
    cdef LiveExecClient _exec_client

    cdef readonly GUID id
    cdef readonly EventStore event_store
    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef readonly Trader trader

    def __init__(
            self,
            str config_path='config.json',
            list strategies=None):
        """
        Initializes a new instance of the TradingNode class.

        :param config_path: The path to the config file.
        :raises ValueError: If the config_path is not a valid string.
        """
        Condition.valid_string(config_path, 'config_path')

        # Load the configuration from the config.json file
        with open(config_path, 'r') as config_file:
            config = json.load(config_file)

        self._clock = LiveClock()
        self._guid_factory = LiveGuidFactory()
        self._zmq_context = zmq.Context()
        self.id = GUID(uuid.uuid4())

        trader_id = TraderId(config['trader']['idTag'])
        database_config = config['database']
        self._log_store = LogStore(trader_id=trader_id, redis_port=database_config['log_store_port'])

        log_config = config['logging']
        self._logger = LiveLogger(
            name=log_config['log_name'],
            level_console=getattr(logging, log_config['log_level_console']),
            level_file=getattr(logging, log_config['log_level_file']),
            level_store=getattr(logging, log_config['log_level_store']),
            log_thread=log_config['log_thread'],
            log_to_file=log_config['log_to_file'],
            log_file_path=log_config['log_file_path'],
            clock=self._clock,
            store=self._log_store)
        self._log = LoggerAdapter(component_name=self.__class__.__name__, logger=self._logger)
        self._log_header()
        self._log.info("Starting...")

        self.account = Account()
        self.portfolio = Portfolio(
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        self.event_store = EventStore(trader_id=trader_id, redis_port=database_config['event_store_port'])

        data_config = config['dataClient']
        self._data_client = LiveDataClient(
            zmq_context=self._zmq_context,
            venue=Venue(data_config['venue']),
            service_name=data_config['serviceName'],
            service_address=data_config['serviceAddress'],
            tick_req_port=data_config['tickReqPort'],
            tick_sub_port=data_config['tickSubPort'],
            bar_req_port=data_config['barReqPort'],
            bar_sub_port=data_config['barSubPort'],
            inst_req_port=data_config['instReqPort'],
            inst_sub_port=data_config['instSubPort'],
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger)

        exec_config = config['execClient']
        self._exec_client = LiveExecClient(
            zmq_context=self._zmq_context,
            service_name=exec_config['serviceName'],
            service_address=exec_config['serviceAddress'],
            events_topic=exec_config['eventsTopic'],
            commands_port=exec_config['commandsPort'],
            events_port=exec_config['eventsPort'],
            account=self.account,
            portfolio=self.portfolio,
            clock=self._clock,
            guid_factory=self._guid_factory,
            logger=self._logger,
            store=self.event_store)

        self.trader = Trader(
            id_tag_trader=trader_id.value,
            strategies=[],
            data_client=self._data_client,
            exec_client=self._exec_client,
            account=self.account,
            portfolio=self.portfolio,
            clock=self._clock,
            logger=self._logger)

        self._log.info("Initialized.")

    cpdef void connect(self):
        """
        Connect the trading node to its services.
        """
        self._data_client.connect()
        self._exec_client.connect()
        self._data_client.update_instruments()

    cpdef void start(self):
        """
        Start the trader.
        """
        self.trader.start()

    cpdef void stop(self):
        """
        Stop the trader.
        """
        self.trader.stop()

    cpdef void disconnect(self):
        """
        Disconnect the trading node to its services.
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
