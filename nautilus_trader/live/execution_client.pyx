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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.commands cimport Command, AccountInquiry
from nautilus_trader.model.commands cimport SubmitOrder, SubmitBracketOrder, ModifyOrder, CancelOrder
from nautilus_trader.model.events cimport Event
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.execution cimport ExecutionEngine, ExecutionClient
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.node_clients cimport MessageClient, MessageSubscriber
from nautilus_trader.network.compression cimport Compressor, BypassCompressor
from nautilus_trader.network.encryption cimport EncryptionSettings
from nautilus_trader.serialization.base cimport DictionarySerializer
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer, RequestSerializer
from nautilus_trader.serialization.serializers cimport EventSerializer, MsgPackEventSerializer
from nautilus_trader.serialization.serializers cimport MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers cimport MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.live.factories cimport LiveUUIDFactory
from nautilus_trader.live.logging cimport LiveLogger


cdef str _UTF8 = "utf-8"
cdef str _EVENT = "Event"

cdef class LiveExecClient(ExecutionClient):
    """
    Provides an execution client for live trading utilizing a ZMQ transport
    to the execution service.
    """

    def __init__(
            self,
            ExecutionEngine exec_engine not None,
            str host not None,
            int command_req_port,
            int command_res_port,
            int event_pub_port,
            Compressor compressor not None=BypassCompressor(),
            EncryptionSettings encryption not None=EncryptionSettings(),
            CommandSerializer command_serializer not None=MsgPackCommandSerializer(),
            DictionarySerializer header_serializer not None=MsgPackDictionarySerializer(),
            RequestSerializer request_serializer not None=MsgPackRequestSerializer(),
            ResponseSerializer response_serializer not None=MsgPackResponseSerializer(),
            EventSerializer event_serializer not None=MsgPackEventSerializer(),
            Clock clock not None=LiveClock(),
            UUIDFactory uuid_factory not None=LiveUUIDFactory(),
            Logger logger not None=LiveLogger()):
        """
        Initialize a new instance of the LiveExecClient class.

        :param exec_engine: The execution engine for the component.
        :param host: The execution service host IP address.
        :param command_req_port: The execution service command request port.
        :param command_res_port: The execution service command response port.
        :param event_pub_port: The execution service event publisher port.
        :param encryption: The encryption configuration.
        :param command_serializer: The command serializer for the client.
        :param header_serializer: The header serializer for the client.
        :param response_serializer: The response serializer for the client.
        :param event_serializer: The event serializer for the client.
        :param clock: The clock for the component.
        :param uuid_factory: The uuid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the events_topic is not a valid string.
        :raises ValueError: If the commands_req_port is not in range [49152, 65535].
        :raises ValueError: If the commands_rep_port is not in range [49152, 65535].
        :raises ValueError: If the events_port is not in range [49152, 65535].
        """
        Condition.valid_string(host, "host")
        Condition.in_range_int(command_req_port, 0, 65535, "command_req_port")
        Condition.in_range_int(command_res_port, 0, 65535, "command_res_port")
        Condition.in_range_int(event_pub_port, 0, 65535, "event_pub_port")
        super().__init__(exec_engine, logger)

        self._command_serializer = command_serializer
        self._event_serializer = event_serializer

        self.trader_id = exec_engine.trader_id
        self.client_id = ClientId(self.trader_id.value)

        self._command_client = MessageClient(
            self.client_id,
            host,
            command_req_port,
            command_res_port,
            header_serializer,
            request_serializer,
            response_serializer,
            compressor,
            encryption,
            clock,
            uuid_factory,
            self._log)

        self._event_subscriber = MessageSubscriber(
            self.client_id,
            host,
            event_pub_port,
            compressor,
            encryption,
            clock,
            uuid_factory,
            self._log)

        self._event_subscriber.register_handler(self._recv_event)

    cpdef void connect(self) except *:
        """
        Connect to the execution service.
        """
        self._event_subscriber.connect()
        self._command_client.connect()
        self._event_subscriber.subscribe(_EVENT)

    cpdef void disconnect(self) except *:
        """
        Disconnect from the execution service.
        """
        self._event_subscriber.unsubscribe(_EVENT)
        self._command_client.disconnect()
        self._event_subscriber.disconnect()

    cpdef void dispose(self) except *:
        """
        Disposes of the execution client.
        """
        self._command_client.dispose()
        self._event_subscriber.dispose()

    cpdef void reset(self) except *:
        """
        Reset the execution client.
        """
        self._reset()

    cpdef void account_inquiry(self, AccountInquiry command) except *:
        self._send_command(command)

    cpdef void submit_order(self, SubmitOrder command) except *:
        self._send_command(command)

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        self._send_command(command)

    cpdef void modify_order(self, ModifyOrder command) except *:
        self._send_command(command)

    cpdef void cancel_order(self, CancelOrder command) except *:
        self._send_command(command)

    cpdef void _send_command(self, Command command) except *:
        cdef bytes payload = self._command_serializer.serialize(command)
        self._command_client.send_message(command, payload)

    cpdef void _recv_event(self, str topic, bytes event_bytes) except *:
        cdef Event event = self._event_serializer.deserialize(event_bytes)
        self._exec_engine.handle_event(event)
