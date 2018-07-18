#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import json

from collections import namedtuple
from decimal import Decimal
from typing import Dict
from pika import PlainCredentials, ConnectionParameters, SelectConnection
from pika.channel import Channel
from pika.frame import Method
from pika.spec import Basic, BasicProperties, Exchange, Queue

from inv_trader.core.checks import typechecking
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderEvent
from inv_trader.strategy import TradeStrategy
from inv_trader.messaging import MsgPackEventSerializer

# Constants
UTF8 = 'utf-8'
StrategyId = str
OrderId = str

# Holder for exchange properties.
ExchangeProps = namedtuple('Exchange', 'name, type')

# Constants for needed exchanges, queues and routing keys.
ORDER_EVENTS_EXCHANGE = ExchangeProps(name='order_events', type='fanout')
ORDER_COMMANDS_EXCHANGE = ExchangeProps(name='order_commands', type='direct')
QUEUE_NAME = 'inv_trader'
ROUTING_KEY = 'inv_trader'


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
                    f"[Warning]: The given event order id was not contained in "
                    f"order index {order_id}")
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
    Provides a live execution client for trading strategies utilizing an AMQP
    (Advanced Message Queue Protocol) 0-9-1 message broker.
    """

    @typechecking
    def __init__(
            self,
            host: str= 'localhost',
            port: int=5672,
            username: str='guest',
            password: str='guest'):
        """
        Initializes a new instance of the LiveExecClient class.
        The host and port parameters are for the order event subscription
        channel.

        :param host: The execution service host IP address (default=127.0.0.1).
        :param port: The execution service host port (default=5672).
        :param username: The AMQP message broker authentication username.
        :param password: The AMQP message broker authentication password.
        """
        super().__init__()
        self._amqp_host = host
        self._amqp_port = port
        self._connection = None
        self._channel = None
        self._closing = False
        self._consumer_tag = None
        self._connection_params = ConnectionParameters(
            host=host,
            port=port,
            credentials=PlainCredentials(username, password))

    def connect(self):
        """
        Connect to the execution service and establish the messaging channels
        and queues needed for trading.
        """
        self._open_connection()

    def disconnect(self):
        """
        Disconnect from the execution service.
        """
        # TODO

    def _open_connection(self):
        """
        Open a new connection with the AMQP message broker.
        :return: The pika connection object.
        """
        self._log("Connecting...")
        self._connection = SelectConnection(self._connection_params,
                                            self._on_connection_open,
                                            stop_ioloop_on_close=False)

        self._connection.ioloop.start()
        self._log((f"Connected to execution service AMQP broker at "
                   f"{self._amqp_host}:{self._amqp_port}."))

        return self._connection

    def _on_connection_open(self, connection: SelectConnection):
        """
        Called once the connection to the AMQP message broker has been established.

        :param: connection: The pika connection object.
        """
        self._log(f'Connection opened {json.dumps(connection.server_properties, sort_keys=True, indent=2)}.')
        self._add_on_connection_close_callback()
        self._open_channel()

    def _add_on_connection_close_callback(self):
        """
        Add an on close callback which will be invoked when the AMQP message
        broker closes the connection to the execution client unexpectedly.
        """
        self._log('Adding connection close callback.')
        self._connection.add_on_close_callback(self._on_connection_closed)

    @typechecking
    def _on_connection_closed(
            self,
            connection: SelectConnection,
            reply_code: int,
            reply_text: str):
        """
        Called when the connection to the AMQP message broker is closed
        unexpectedly. Since it is unexpected, the client will reconnect..

        :param connection: The pika connection object.
        :param reply_code: The server provided reply code if given.
        :param reply_text: The server provided reply text if given.
        """
        self._channel = None
        if self._closing:
            self._connection.ioloop.stop()
        else:
            self._log(
                f'Warning: Connection closed, reopening in 5 seconds: ({reply_code}) {reply_text}')
            self._connection.add_timeout(5, self._reconnect)

    def _reconnect(self):
        """
        Called by the IOLoop timer if the connection is closed.
        (See the on_connection_closed method).
        """
        # This is the old connection IOLoop instance, stop its ioloop
        self._connection.ioloop.stop()

        if not self._closing:
            # Create a new connection
            self._connection = self._open_connection()

            # There is now a new connection, needs a new ioloop to run
            self._connection.ioloop.start()

    def _open_channel(self):
        """
        Open a new channel with the execution service by issuing the Channel.Open RPC
        command. When the execution service responds that the channel is open, the
        on_channel_open callback will be invoked.
        """
        self._log('Creating a new channel...')
        self._connection.channel(on_open_callback=self._on_channel_open)

    def _on_channel_open(self, channel: Channel):
        """
        This method is invoked by pika when the channel has been opened.
        The channel object is passed in so we can make use of it.
        Since the channel is now open, we'll declare the exchange to use.

        :param channel: The pike channel object
        """
        self._log('Channel opened.')
        self._channel = channel
        self._add_on_channel_close_callback()
        self._setup_exchange(ORDER_EVENTS_EXCHANGE)
        self._setup_exchange(ORDER_COMMANDS_EXCHANGE)

    def _add_on_channel_close_callback(self):
        """
        This method tells pika to call the on_channel_closed method if
        RabbitMQ unexpectedly closes the channel.
        """
        self._channel.add_on_close_callback(self._on_channel_closed)
        self._log('Added channel close callback.')

    @typechecking
    def _on_channel_closed(
            self,
            channel: Channel,
            reply_code: int,
            reply_text: str):
        """
        Invoked by pika when RabbitMQ unexpectedly closes the channel.
        Channels are usually closed if you attempt to do something that
        violates the protocol, such as re-declare an exchange or queue with
        different parameters. In this case, we'll close the connection
        to shutdown the object.

        :param channel: The closed channel.
        :param reply_code: The numeric reason the channel was closed.
        :param reply_text: The text reason the channel was closed.
        """
        self._log(f'Warning: Channel {channel} was closed: ({reply_code}) {reply_text}.')
        self._connection.close()

    @typechecking
    def _setup_exchange(self, exchange_info: ExchangeProps):
        """
        Setup the exchange on RabbitMQ by invoking the Exchange.Declare RPC
        command. When it is complete, the on_exchange_declare_ok method will
        be invoked by pika.

        :param exchange_info: The name and type of the exchange to declare.
        """
        self._channel.exchange_declare(self._on_exchange_declare_ok,
                                       exchange_info.name,
                                       exchange_info.type)
        self._channel.queue_declare(self._on_queue_declare_ok, QUEUE_NAME)
        self._channel.queue_bind(self._on_bind_ok,
                                 QUEUE_NAME,
                                 exchange_info.name,
                                 ROUTING_KEY)
        self._log(f'Declared exchange {exchange_info.name} (type={exchange_info.type}).')

    @typechecking
    def _on_exchange_declare_ok(self, response_frame: Method):
        """
        Invoked by pika when RabbitMQ has finished the Exchange.Declare RPC
        command.

        :param response_frame: The Exchange.DeclareOk response frame.
        """
        self._log(f'Exchange declared on channel {response_frame.channel_number}.')
        self._setup_queue(QUEUE_NAME)

    @typechecking
    def _setup_queue(self, queue_name: str):
        """
        Setup the queue on RabbitMQ by invoking the Queue.Declare RPC
        command. When it is complete, the on_queue_declare_ok method will
        be invoked by pika.

        :param queue_name: The name of the queue to declare.
        """
        self._log(f'Declaring queue {queue_name}.')
        self._channel.queue_declare(self._on_queue_declare_ok, queue_name)

    @typechecking
    def _on_queue_declare_ok(self, response_frame: Method):
        """
        Method invoked by pika when the Queue.Declare RPC call made in
        setup_queue has completed. In this method we will bind the queue
        and exchange together with the routing key by issuing the Queue.Bind
        RPC command. When this command is complete, the on_bindok method will
        be invoked by pika.

        :param response_frame: The Queue.DeclareOk frame.
        """
        self._log(f'Queue declared on channel {response_frame.channel_number}.')
        self._log('Binding {self.EXCHANGE} to {self.QUEUE} with {self.ROUTING_KEY}.')

    @typechecking
    def _on_bind_ok(self, response_frame: Method):
        """
        Invoked by pika when the Queue.Bind method has completed. At this
        point we will start consuming messages by calling start_consuming
        which will invoke the needed RPC commands to start the process.

        :param response_frame: The Queue.BindOk response frame.
        """
        self._log(f'Queue bound on channel {response_frame.channel_number}.')
        self._start_consuming()
        self._log(f'Consumption ready...')

    def _start_consuming(self):
        """
        This method sets up the consumer by first calling
        add_on_cancel_callback so that the object is notified if RabbitMQ
        cancels the consumer. It then issues the Basic.Consume RPC command
        which returns the consumer tag that is used to uniquely identify the
        consumer with RabbitMQ. We keep the value to use it when we want to
        cancel consuming. The on_message method is passed in as a callback pika
        will invoke when a message is fully received.

        """
        self._log('Issuing consumer related RPC commands.')
        self._log('Adding consumer cancellation callback')
        self._channel.add_on_cancel_callback(self._on_consumer_cancelled)
        self._consumer_tag = self._channel.basic_consume(self._on_message, QUEUE_NAME)

    @typechecking
    def _on_consumer_cancelled(self, response_frame: Method):
        """
        Invoked by pika when RabbitMQ sends a Basic.Cancel for a consumer
        receiving messages.

        :param response_frame: The Basic.Cancel response method frame.
        """
        self._log(f'Consumer was cancelled remotely, shutting down {response_frame}...')
        if self._channel:
            self._channel.close()

    @typechecking
    def _on_message(
            self,
            channel: Channel,
            basic_deliver: Basic.Deliver,
            properties: BasicProperties,
            body: bytes):
        """
        Invoked by pika when a message is delivered from RabbitMQ. The
        channel is passed for your convenience. The basic_deliver object that
        is passed in carries the exchange, routing key, delivery tag and
        a redelivered flag for the message. The properties passed in is an
        instance of BasicProperties with the message properties and the body
        is the message that was sent.

        :param channel: The pike channel object
        :param basic_deliver: The basic deliver method.
        :param properties: The properties.
        :param body: The message body.
        """
        self._log((f'Received message #{basic_deliver.delivery_tag} '
                   f'on channel {channel.channel_number} '
                   f'from {properties.app_id}: {body}'))

        self._acknowledge_message(basic_deliver.delivery_tag)

    @typechecking
    def _acknowledge_message(self, delivery_tag: int):
        """
        Acknowledge the message delivery from RabbitMQ by sending a
        Basic.Ack RPC method for the delivery tag.

        :param delivery_tag: The delivery tag from the Basic.Deliver frame.
        """
        self._log(f'Acknowledging message {delivery_tag}')
        self._channel.basic_ack(delivery_tag)

    def _stop_consuming(self):
        """Tell RabbitMQ that you would like to stop consuming by sending the
        Basic.Cancel RPC command.
        """
        if self._channel:
            self._log('Sending a Basic.Cancel RPC command to RabbitMQ')
            self._channel.basic_cancel(self._on_cancel_ok, self._consumer_tag)

    @typechecking
    def _on_cancel_ok(self, response_frame):
        """This method is invoked by pika when RabbitMQ acknowledges the
        cancellation of a consumer. At this point we will close the channel.
        This will invoke the on_channel_closed method once the channel has been
        closed, which will in-turn close the connection.

        :param response_frame: The Basic.CancelOk response frame.
        """
        self._log(
            f'Cancellation of the consumer on channel {response_frame} OK.')
        self._close_channel()

    def _close_channel(self):
        """Call to close the channel with RabbitMQ cleanly by issuing the
        Channel.Close RPC command.
        """
        self._log('Closing the channel...')
        self._channel.close()

    def _stop(self):
        """
        Cleanly shutdown the connection to RabbitMQ by stopping the consumer
        with RabbitMQ. When RabbitMQ confirms the cancellation, on_cancel_ok
        will be invoked by pika, which will then close the channel and
        connection. The IOLoop is started again because this method is invoked
        when CTRL-C is pressed raising a KeyboardInterrupt exception. This
        exception stops the IOLoop which needs to be running for pika to
        communicate with RabbitMQ. All of the commands issued prior to starting
        the IOLoop will be buffered but not processed.
        """
        self._log('Stopping...')
        self._closing = True
        self._stop_consuming()
        self._connection.ioloop.start()
        self._log('Stopped.')

    def _close_connection(self):
        """
        Closes the connection to RabbitMQ.
        """
        self._log('Closing connection...')
        self._connection.close()

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
        # TODO
        self._amqp_channel.basic_publish(exchange='',
                                         routing_key=ORDER_CHANNEL,
                                         body='modify_order:')

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
