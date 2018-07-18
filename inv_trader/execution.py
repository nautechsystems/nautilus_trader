#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import time
import uuid
import iso8601

from redis import StrictRedis, ConnectionError
from datetime import datetime
from decimal import Decimal
from typing import Dict
from pika import ConnectionParameters, BlockingConnection

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide, OrderType, OrderStatus
from inv_trader.model.objects import Symbol, BarType, Bar
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.strategy import TradeStrategy
from inv_trader.messaging import MsgPackEventSerializer

StrategyId = str
OrderId = str

UTF8 = 'utf-8'
ORDER_EVENT_BUS = 'order_events'
ORDER_CHANNEL = 'order_channel'


class ExecutionClient:
    """
    The abstract base class for all execution clients.
    """

    __metaclass__ = abc.ABCMeta

    @typechecking
    def __init__(self):
        """
        Initializes a new instance of the ExecutionClient class.
        """
        self._registered_strategies = {}  # type: Dict[StrategyId, callable]
        self._order_index = {}            # type: Dict[OrderId, StrategyId]

    @typechecking
    def register_strategy(self, strategy: TradeStrategy):
        """
        Register the given strategy with the execution client.
        """
        strategy_id = str(strategy)

        if strategy_id in self._registered_strategies.keys():
            raise ValueError("The strategy must have a unique name and label.")

        self._registered_strategies[strategy_id] = strategy._update_events
        strategy._register_execution_client(self)

    @abc.abstractmethod
    def connect(self):
        """
        Connect to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    @abc.abstractmethod
    def disconnect(self):
        """
        Disconnect from the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    @abc.abstractmethod
    def submit_order(
            self,
            order: Order,
            strategy_id: StrategyId):
        """
        Send a submit order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    @abc.abstractmethod
    def cancel_order(self, order: Order):
        """
        Send a cancel order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    @abc.abstractmethod
    def modify_order(self, order: Order, new_price: Decimal):
        """
        Send a modify order request to the execution service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the execution client.")

    @typechecking
    def _register_order(
            self,
            order: Order,
            strategy_id: StrategyId):
        """
        Register the given order with the execution client.

        :param order: The order to register.
        :param strategy_id: The strategy id to register with the order.
        """
        if order.id in self._order_index.keys():
            raise ValueError(f"The order does not have a unique id.")

        self._order_index[order.id] = strategy_id

    @typechecking
    def _on_event(self, event: Event):
        """
        Handle events received from the execution service.
        """
        # Order event
        if isinstance(event, OrderEvent):
            order_id = event.order_id
            if order_id not in self._order_index.keys():
                self._log(
                    f"[Warning]: The given event order id not contained in order index {order_id}")
                return

            strategy_id = self._order_index[order_id]
            self._registered_strategies[strategy_id](event)

        # Account event
        # TODO

    @staticmethod
    @typechecking
    def _log(message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"ExecClient: {message}")


class LiveExecClient(ExecutionClient):
    """
    Provides a live execution client for trading strategies.
    """

    @typechecking
    def __init__(
            self,
            redis_host: str='localhost',
            redis_port: int=6379,
            amqp_host: str='localhost',
            amqp_port: int=5672):
        """
        Initializes a new instance of the LiveExecClient class.
        The host and port parameters are for the order event subscription
        channel.

        :param redis_host: The redis host IP address (default=127.0.0.1).
        :param redis_port: The redis host port (default=6379).
        :param amqp_host: The AMQP host IP address (default=127.0.0.1).
        :param amqp_port: The AMQP host port (default=5672).
        """
        super().__init__()
        self._redis_host = redis_host
        self._redis_port = redis_port
        self._amqp_host = amqp_host
        self._amqp_port = amqp_port
        self._pubsub_client = None
        self._pubsub = None
        self._pubsub_thread = None
        self._amqp_client = None
        self._amqp_channel = None

    @property
    def is_connected(self) -> bool:
        """
        :return: True if the client is connected, otherwise false.
        """
        if self._pubsub_client is None or self._amqp_client is None:
            return False

        try:
            self._pubsub_client.ping()
        except ConnectionError:
            return False

        # TODO: Check AMQP connection.

        return True

    def connect(self):
        """
        Connect to the execution service and create a pub/sub server.
        """
        self._pubsub_client = StrictRedis(host=self._redis_host,
                                          port=self._redis_port,
                                          db=0)
        self._pubsub = self._pubsub_client.pubsub()
        self._pubsub.subscribe(**{ORDER_EVENT_BUS: self._order_event_handler})

        self._log((f"Connected to execution service publisher at "
                   f"{self._redis_host}:{self._redis_port}."))

        connection_params = ConnectionParameters(self._amqp_host)
        self._amqp_client = BlockingConnection(connection_params)
        self._amqp_channel = self._amqp_client.channel()
        self._amqp_channel.queue_declare('orders')

        self._log((f"Connected to execution service orders channel at "
                   f"{self._amqp_host}:{self._amqp_port}."))

    def disconnect(self):
        """
        Disconnect from the local pub/sub server and the execution service.
        """
        if self._pubsub is not None:
            self._pubsub.unsubscribe(ORDER_EVENT_BUS)

        if self._pubsub_thread is not None:
            self._pubsub_thread.stop()
            time.sleep(0.100)  # Allows thread to stop.
            self._log(f"Stopped PubSub thread {self._pubsub_thread}.")

        if self._pubsub_client is not None:
            self._pubsub_client.connection_pool.disconnect()
            self._log((f"Disconnected from execution service "
                       f"at {self._redis_host}:{self._redis_port}."))
        else:
            self._log("Disconnected (the client was already disconnected).")

        self._pubsub_client = None
        self._pubsub = None
        self._pubsub_thread = None

    @typechecking
    def submit_order(
            self,
            order: Order,
            strategy_id: StrategyId):
        """
        Send a submit order request to the execution service.

        :param: order: The order to submit.
        :param: strategy_id: The strategy id to register the order with.
        """
        self._check_connection()
        super()._register_order(order, strategy_id)

        # TODO
        self._amqp_channel.basic_publish(exchange='',
                                         routing_key=ORDER_CHANNEL,
                                         body='submit_order:')

    @typechecking
    def cancel_order(self, order: Order):
        """
        Send a cancel order request to the execution service.

        :param: order: The order to cancel.
        """
        self._check_connection()

        # TODO
        self._amqp_channel.basic_publish(exchange='',
                                         routing_key=ORDER_CHANNEL,
                                         body='cancel_order:')

    @typechecking
    def modify_order(
            self,
            order: Order,
            new_price: Decimal):
        """
        Send a modify order request to the execution service.

        :param: order: The order to modify.
        :param: new_price: The new modified price for the order.
        """
        self._check_connection()

        # TODO
        self._amqp_channel.basic_publish(exchange='',
                                         routing_key=ORDER_CHANNEL,
                                         body='modify_order:')

    def _check_connection(self):
        """
        Check the connection with the live database.

        :raises: ConnectionError if the client is not connected.
        """
        if self._pubsub_client is None:
            raise ConnectionError(("No connection has been established to the execution service "
                                   "(please connect first)."))
        if not self.is_connected:
            raise ConnectionError("No connection is established with the execution service.")

    @typechecking
    def _deserialize_order_event(self, body: bytearray) -> OrderEvent:
        """
        Deserialize the given message body.

        :param body: The body to deserialize.
        :return: The deserialized order event.
        """
        return MsgPackEventSerializer.deserialize_order_event(body)

    @typechecking
    def _order_event_handler(self, body: bytearray):
        """"
        Handle the order event message by parsing to an OrderEvent and sending
        to the registered strategy.

        :param body: The order event message body.
        """
        order_event = self._deserialize_order_event(body)

        # If no registered strategies then print message to console.
        if len(self._registered_strategies) == 0:
            print(f"Received order event from queue: {order_event}")

        self._on_event(order_event)
