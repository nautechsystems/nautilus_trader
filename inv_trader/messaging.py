#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import json

from typing import Callable
from threading import Thread
from collections import namedtuple
from pika import ConnectionParameters, SelectConnection
from pika.channel import Channel
from pika.frame import Method
from pika.spec import Basic, BasicProperties

from inv_trader.core.checks import typechecking


# Holder for AMQP exchange properties.
MQProps = namedtuple('MQProps', 'exchange_name, exchange_type, queue_name, routing_key')


class MQWorker(Thread):
    """
    Provides an AMQP message queue worker with a separate thread and connection.
    """

    @typechecking
    def __init__(
            self,
            connection_params: ConnectionParameters,
            mq_props: MQProps,
            message_handler: Callable,
            worker_name: str='MQWorker'):
        """
        Initializes a new instance of the MQWorker class.

        :param connection_params: The AMQP broker connection parameters.
        :param mq_props: The AMQP broker properties.
        :param message_handler: The handler to send received messages to.
        :param worker_name: The name of the message queue worker.
        """
        super().__init__()
        self._worker_name = worker_name
        self._connection_params = connection_params
        self._exchange_name = mq_props.exchange_name
        self._exchange_type = mq_props.exchange_type
        self._queue_name = mq_props.queue_name
        self._routing_key = mq_props.routing_key
        self._message_handler = message_handler
        self._connection = None
        self._channel = None
        self._closing = False
        self._consumer_tag = None

    @property
    def name(self) -> str:
        """
        :return: The name of the message queue worker.
        """
        return self._worker_name

    def run(self):
        """
        Beings the message queue process by connecting to the execution service
        and establish the messaging channels and queues needed.
        """
        self._open_connection()

    def send(self, message: bytes):
        """
        Send the given bytes as a message to the AMQP broker.

        :param message: The message body to send to the AMQP broker.
        """
        self._channel.basic_publish(exchange=self._exchange_name,
                                    routing_key=self._routing_key,
                                    body=message)
        self._log((f"Sent message: exchange={self._exchange_name}, "
                   f"routing_key={self._routing_key}, "
                   f"body={message}"))

    def stop(self):
        """
        Stops consuming messages from and closes the connection to the AMQP broker.
        """
        self._stop()
        self._close_connection()
        self._log((f"Disconnected from AMQP broker at "
                   f"{self._connection_params.host}:{self._connection_params.port}."))

    def _open_connection(self):
        """
        Open a new connection with the AMQP broker.

        :return: The pika connection object.
        """
        self._log(f"Connecting to message exchange {self._exchange_name}...")
        self._connection = SelectConnection(self._connection_params,
                                            self._on_connection_open,
                                            stop_ioloop_on_close=False)

        self._connection.ioloop.start()
        return self._connection

    def _on_connection_open(self, connection: SelectConnection):
        """
        Called once the connection to the AMQP broker has been established.

        :param: connection: The pika connection object.
        """
        self._log((f"Connected to AMQP broker at "
                   f"{self._connection_params.host}:{self._connection_params.port}."))
        self._log(f'{json.dumps(connection.server_properties, sort_keys=True, indent=2)}.')
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
            self._log((f'Warning: Connection closed, '
                       f'reopening in 5 seconds: '
                       f'(reply_code={reply_code}, reply_text={reply_text}).'))
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
        Open a new channel with the execution service by issuing the
        Channel.Open RPC command. When the execution service responds that the
        channel is open, the on_channel_open callback will be invoked.
        """
        self._log('Creating a new channel...')
        self._connection.channel(on_open_callback=self._on_channel_open)

    def _on_channel_open(self, channel: Channel):
        """
        Invoked by pika when the channel has been opened.
        The channel object is passed in so we can make use of it.
        Since the channel is now open, we'll declare the exchange to use.

        :param channel: The pike channel object
        """
        self._log('Channel opened.')
        self._channel = channel
        self._add_on_channel_close_callback()
        self._setup_exchange()

    def _add_on_channel_close_callback(self):
        """
        This method tells pika to call the on_channel_closed method if
        the AMQP broker unexpectedly closes the channel.
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
        self._log((f'Warning: Channel {channel.channel_number} was closed: '
                   f'(reply_code={reply_code}, reply_text=\'{reply_text}\').'))
        self._connection.close()

    @typechecking
    def _setup_exchange(self):
        """
        Setup the exchange on the AMQP broker by invoking the Exchange.Declare
        RPC command. When it is complete, the on_exchange_declare_ok method will
        be invoked by pika.
        """
        self._channel.exchange_declare(self._on_exchange_declare_ok,
                                       self._exchange_name,
                                       self._exchange_type,
                                       durable=True,
                                       auto_delete=False)

        self._log(f'Declared exchange {self._exchange_name} (type={self._exchange_type}).')

    @typechecking
    def _on_exchange_declare_ok(self, response_frame: Method):
        """
        Invoked by pika when the AMQP broker has finished the
        Exchange.Declare RPC command.

        :param response_frame: The Exchange.DeclareOk response frame.
        """
        self._log(f'Exchange declared on channel {response_frame.channel_number}.')
        self._setup_queue()

    @typechecking
    def _setup_queue(self):
        """
        Setup the queue on the AMQP broker by invoking the Queue.Declare RPC
        command. When it is complete, the on_queue_declare_ok method will
        be invoked by pika.
        """
        self._log(f'Declaring queue {self._queue_name}.')
        self._channel.queue_declare(self._on_queue_declare_ok,
                                    self._queue_name,
                                    durable=True,
                                    auto_delete=False)

    @typechecking
    def _on_queue_declare_ok(self, response_frame: Method):
        """
        Method invoked by pika when the Queue.Declare RPC call made in
        setup_queue has completed. In this method we will bind the queue
        and exchange together with the routing key by issuing the Queue.Bind
        RPC command. When this command is complete, the on_bind_ok method will
        be invoked by pika.

        :param response_frame: The Queue.DeclareOk response frame.
        """
        self._log(f'Queue {self._queue_name} declared on channel {response_frame.channel_number}.')
        self._channel.queue_bind(self._on_bind_ok,
                                 self._queue_name,
                                 self._exchange_name,
                                 self._routing_key)

        self._log((f'Binding (exchange={self._exchange_name}, '
                   f'queue={self._queue_name}, '
                   f'routing_key={self._routing_key}).'))

    @typechecking
    def _on_bind_ok(self, response_frame: Method):
        """
        Invoked by pika when the Queue.Bind method has completed. At this
        point we will start consuming messages by calling start_consuming
        which will invoke the needed RPC commands to start the process.

        :param response_frame: The Queue.BindOk response frame.
        """
        self._log(f'Queue {self._queue_name} bound on channel {response_frame.channel_number}.')
        self._start_consuming()
        self._log(f'Ready...')

    def _start_consuming(self):
        """
        This method sets up the consumer by first calling
        add_on_cancel_callback so that the object is notified if the AMQP broker
        cancels the consumer. It then issues the Basic.Consume RPC command
        which returns the consumer tag that is used to uniquely identify the
        consumer with AMQP broker. We keep the value to use it when we want to
        cancel consuming. The on_message method is passed in as a callback pika
        will invoke when a message is fully received.
        """
        self._log('Issuing consumer related RPC commands.')
        self._log('Adding consumer cancellation callback.')
        self._channel.add_on_cancel_callback(self._on_consumer_cancelled)
        self._consumer_tag = self._channel.basic_consume(self._on_message, self._queue_name)

    @typechecking
    def _on_consumer_cancelled(self, response_frame: Method):
        """
        Invoked by pika when the AMQP broker sends a Basic.Cancel for a consumer
        receiving messages.

        :param response_frame: The Basic.Cancel response method frame.
        """
        self._log(f'Consumer was cancelled remotely, shutting down.')
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
        Invoked by pika when a message is delivered from the AMQP broker. The
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
                   f'from {properties.app_id}.'))

        self._acknowledge_message(basic_deliver.delivery_tag)
        self._message_handler(body)

    @typechecking
    def _acknowledge_message(self, delivery_tag: int):
        """
        Acknowledge the message delivery from the AMQP broker by sending a
        Basic.Ack RPC method for the delivery tag to the AMQP broker.

        :param delivery_tag: The delivery tag from the Basic.Deliver frame.
        """
        self._log(f'Acknowledging message {delivery_tag}.')
        self._channel.basic_ack(delivery_tag)

    def _stop_consuming(self):
        """
        Tell the AMQP broker that you would like to stop consuming by sending the
        Basic.Cancel RPC command.
        """
        if self._channel:
            self._log('Sending basic cancel command to AMQP broker...')
            self._channel.basic_cancel(self._on_cancel_ok, self._consumer_tag)

    @typechecking
    def _on_cancel_ok(self, response_frame: Method):
        """
        Called when the AMQP broker acknowledges the cancellation of a consumer.
        At this point the channel is closed. This will then invoke the
        on_channel_closed method once the channel has been closed, which will
        in-turn close the connection.

        :param response_frame: The Basic.CancelOk response frame.
        """
        self._log(
            f'Cancellation of the consumer on channel {response_frame.channel_number} OK.')
        self._close_channel()

    def _close_channel(self):
        """
        Call to close the channel with the AMQP broker cleanly by issuing the
        Channel.Close RPC command.
        """
        self._log('Closing the channel...')
        self._channel.close()

    def _stop(self):
        """
        Cleanly shutdown the connection to the AMQP broker by stopping the
        consumer. When the AMQP broker confirms the cancellation, on_cancel_ok
        will be invoked by pika, which will then close the channel and
        connection. The IOLoop is started again because this method is invoked
        when CTRL-C is pressed raising a KeyboardInterrupt exception. This
        exception stops the IOLoop which needs to be running for pika to
        communicate with the AMQP broker. All of the commands issued prior to
        starting the IOLoop will be buffered but not processed.
        """
        self._log('Stopping...')
        self._closing = True
        self._stop_consuming()
        self._connection.ioloop.start()
        self._log('Stopped.')

    def _close_connection(self):
        """
        Closes the connection to AMQP broker.
        """
        self._log('Closing connection...')
        self._connection.close()

    @typechecking
    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"{self._worker_name}: {message}")
