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
import uuid
import unittest

from nautilus_trader.core.types import GUID
from nautilus_trader.model.objects import Price, Volume, Tick, Bar
from nautilus_trader.model.identifiers import Venue, TraderId
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.network.identifiers import ServerId
from nautilus_trader.network.messages import DataResponse, DataRequest
from nautilus_trader.network.encryption import EncryptionSettings
from nautilus_trader.network.compression import BypassCompressor
from nautilus_trader.network.node_servers import MessageServer, MessagePublisher
from nautilus_trader.serialization.data import Utf8TickSerializer, Utf8BarSerializer, DataMapper
from nautilus_trader.serialization.data import BsonDataSerializer, BsonInstrumentSerializer
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.live.clock import LiveClock
from nautilus_trader.live.guid import LiveGuidFactory
from nautilus_trader.live.logging import LiveLogger
from nautilus_trader.live.data import LiveDataClient

from tests.test_kit.stubs import TestStubs, UNIX_EPOCH
from tests.test_kit.mocks import ObjectStorer

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()

TEST_DATA_REQ_PORT = 57601
TEST_DATA_REP_PORT = 57602
TEST_DATA_PUB_PORT = 57603
TEST_TICK_PUB_PORT = 57604


class LiveDataClientTests(unittest.TestCase):
    # Fixture Setup
    def setUp(self):
        # Arrange
        self.data_mapper = DataMapper()
        self.data_serializer = BsonDataSerializer()

        self.header_serializer = MsgPackDictionarySerializer()
        self.request_serializer = MsgPackRequestSerializer()
        self.response_serializer = MsgPackResponseSerializer()
        self.compressor = BypassCompressor()
        self.encryption = EncryptionSettings()
        self.clock = LiveClock()
        self.guid_factory = LiveGuidFactory()
        self.logger = LiveLogger()

        self.data_server = MessageServer(
            server_id=ServerId('DataServer-001'),
            recv_port=TEST_DATA_REQ_PORT,
            send_port=TEST_DATA_REP_PORT,
            header_serializer=self.header_serializer,
            request_serializer=self.request_serializer,
            response_serializer=self.response_serializer,
            compressor=self.compressor,
            encryption=self.encryption,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=LoggerAdapter('DataServer', self.logger))

        self.data_server_sink = []
        self.data_server.register_request_handler(self.data_server_sink.append)

        self.data_publisher = MessagePublisher(
            server_id=ServerId('DataPublisher-001'),
            port=TEST_DATA_PUB_PORT,
            compressor=self.compressor,
            encryption=self.encryption,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=LoggerAdapter('DataPublisher', self.logger))

        self.tick_publisher = MessagePublisher(
            server_id=ServerId('TickPublisher-001'),
            port=TEST_TICK_PUB_PORT,
            compressor=self.compressor,
            encryption=self.encryption,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=LoggerAdapter('TickPublisher', self.logger))

        self.data_server.start()
        self.data_publisher.start()
        self.tick_publisher.start()
        time.sleep(0.1)

        self.data_client = LiveDataClient(
            trader_id=TraderId('Tester', '000'),
            host='127.0.0.1',
            data_req_port=TEST_DATA_REQ_PORT,
            data_res_port=TEST_DATA_REP_PORT,
            data_pub_port=TEST_DATA_PUB_PORT,
            tick_pub_port=TEST_TICK_PUB_PORT,
            compressor=self.compressor,
            encryption=self.encryption,
            header_serializer=self.header_serializer,
            request_serializer=self.request_serializer,
            response_serializer=self.response_serializer,
            data_serializer=self.data_serializer,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=self.logger)
        self.data_client.connect()
        time.sleep(0.1)

    # Fixture Tear Down
    def tearDown(self):
        self.data_client.disconnect()
        self.data_server.stop()
        self.data_publisher.stop()
        self.tick_publisher.stop()
        # Allowing the garbage collector to clean up resources avoids threading
        # errors caused by the continuous disposal of sockets. Thus for testing
        # we're avoiding calling .dispose() on the sockets.

    def test_can_subscribe_to_tick_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        # Act
        self.data_client.subscribe_ticks(AUDUSD_FXCM, handler=data_receiver.store)

        # Assert
        self.assertIn(AUDUSD_FXCM, self.data_client.subscribed_ticks())

    def test_can_unsubscribe_from_tick_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        # Act
        self.data_client.subscribe_ticks(AUDUSD_FXCM, handler=data_receiver.store)
        self.data_client.unsubscribe_ticks(AUDUSD_FXCM, handler=data_receiver.store)

        # Assert
        self.assertNotIn(AUDUSD_FXCM, self.data_client.subscribed_ticks())

    def test_can_receive_published_tick_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        tick = Tick(AUDUSD_FXCM,
                    Price(1.00000, 5),
                    Price(1.00001, 5),
                    Volume(1),
                    Volume(1),
                    UNIX_EPOCH)

        # Act
        self.data_client.subscribe_ticks(AUDUSD_FXCM, handler=data_receiver.store)

        time.sleep(0.1)
        self.tick_publisher.publish(AUDUSD_FXCM.value, Utf8TickSerializer.py_serialize(tick))
        time.sleep(0.1)

        # Assert
        self.assertEqual(1, len(data_receiver.get_store()))
        self.assertEqual(tick, data_receiver.get_store()[0])

    def test_can_subscribe_to_bar_data(self):
        # Arrange
        data_receiver = ObjectStorer()
        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act
        self.data_client.subscribe_bars(bar_type, handler=data_receiver.store_2)

        # Assert
        self.assertIn(bar_type, self.data_client.subscribed_bars())

    def test_can_unsubscribe_from_bar_data(self):
        # Arrange
        data_receiver = ObjectStorer()
        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act
        self.data_client.subscribe_bars(bar_type, handler=data_receiver.store_2)
        self.data_client.unsubscribe_bars(bar_type, handler=data_receiver.store_2)

        # Assert
        self.assertNotIn(bar_type, self.data_client.subscribed_ticks())

    def test_can_receive_published_bar_data(self):
        # Arrange
        self.data_client.connect()
        data_receiver = ObjectStorer()
        bar_type = TestStubs.bartype_audusd_1min_bid()
        bar = Bar(Price(1.00001, 5),
                  Price(1.00004, 5),
                  Price(1.00002, 5),
                  Price(1.00003, 5),
                  Volume(100000),
                  UNIX_EPOCH)

        # Act
        self.data_client.subscribe_bars(bar_type, handler=data_receiver.store_2)

        time.sleep(0.1)
        self.data_publisher.publish('Bar:' + str(bar_type), Utf8BarSerializer.py_serialize(bar))
        time.sleep(0.1)

        # Assert
        self.assertEqual(1, len(data_receiver.get_store()))
        self.assertEqual((bar_type, bar), data_receiver.get_store()[0])

    def test_can_subscribe_to_instrument_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        # Act
        self.data_client.subscribe_instrument(AUDUSD_FXCM, handler=data_receiver.store)

        # Assert
        self.assertIn(AUDUSD_FXCM, self.data_client.subscribed_instruments())

    def test_can_unsubscribe_from_instrument_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        # Act
        self.data_client.subscribe_instrument(AUDUSD_FXCM, handler=data_receiver.store)
        self.data_client.unsubscribe_instrument(AUDUSD_FXCM, handler=data_receiver.store)

        # Assert
        self.assertNotIn(AUDUSD_FXCM, self.data_client.subscribed_instruments())

    def test_can_receive_published_instrument_data(self):
        # Arrange
        instrument = TestStubs.instrument_gbpusd()
        data_receiver = ObjectStorer()
        serializer = BsonInstrumentSerializer()

        # Act
        self.data_client.subscribe_instrument(instrument.symbol, handler=data_receiver.store)

        time.sleep(0.1)
        self.data_publisher.publish('Instrument:' + instrument.symbol.value, serializer.serialize(instrument))
        time.sleep(0.1)

        # Assert
        self.assertEqual(1, len(data_receiver.get_store()))
        self.assertEqual(instrument, data_receiver.get_store()[0])

    def test_can_request_tick_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        # Act
        self.data_client.request_ticks(
            AUDUSD_FXCM,
            UNIX_EPOCH.date(),
            UNIX_EPOCH.date(),
            limit=0,
            callback=data_receiver.store)

        time.sleep(0.2)
        # Assert
        self.assertEqual(1, len(self.data_server_sink))
        self.assertEqual(DataRequest, type(self.data_server_sink[0]))

    def test_can_receive_tick_data(self):
        # Arrange
        data_receiver = ObjectStorer()
        self.data_client.request_ticks(
            AUDUSD_FXCM,
            UNIX_EPOCH.date(),
            UNIX_EPOCH.date(),
            limit=0,
            callback=data_receiver.store)

        time.sleep(0.2)

        tick = Tick(AUDUSD_FXCM,
                    Price(1.00000, 5),
                    Price(1.00001, 5),
                    Volume(1),
                    Volume(1),
                    UNIX_EPOCH)
        ticks = [tick, tick, tick, tick, tick]
        tick_data = self.data_mapper.map_ticks(ticks)

        data = self.data_serializer.serialize(tick_data)
        data_response = DataResponse(
            data,
            'Tick[]',
            'BSON',
            self.data_client.last_request_id,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.data_server.send_response(data_response, self.data_client.client_id)

        time.sleep(0.1)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(ticks, response)

    def test_can_request_bar_data(self):
        # Arrange
        data_receiver = ObjectStorer()
        bar_type = TestStubs.bartype_audusd_1min_bid()

        self.data_client.request_bars(
            bar_type,
            UNIX_EPOCH.date(),
            UNIX_EPOCH.date(),
            limit=0,
            callback=data_receiver.store_2)

        time.sleep(0.1)

        bar = Bar(Price(1.00001, 5),
                  Price(1.00004, 5),
                  Price(1.00002, 5),
                  Price(1.00003, 5),
                  Volume(100000),
                  UNIX_EPOCH)
        bars = [bar, bar, bar, bar, bar]
        bar_data = self.data_mapper.map_bars(bars, bar_type)

        data = self.data_serializer.serialize(bar_data)
        data_response = DataResponse(
            data,
            'Bar[]',
            'BSON',
            self.data_client.last_request_id,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.data_server.send_response(data_response, self.data_client.client_id)

        time.sleep(0.2)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(bar_type, response[0])
        self.assertEqual(bars, response[1])

    def test_can_request_instrument_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        self.data_client.request_instrument(GBPUSD_FXCM, data_receiver.store)

        time.sleep(0.1)

        instruments = [TestStubs.instrument_gbpusd()]
        instrument_data = self.data_mapper.map_instruments(instruments)

        data = self.data_serializer.serialize(instrument_data)
        data_response = DataResponse(
            data,
            'Instrument[]',
            'BSON',
            self.data_client.last_request_id,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.data_server.send_response(data_response, self.data_client.client_id)

        time.sleep(0.2)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(instruments, response)

    def test_can_request_instruments_data(self):
        # Arrange
        data_receiver = ObjectStorer()

        self.data_client.connect()
        self.data_client.request_instruments(Venue('FXCM'), data_receiver.store)

        time.sleep(0.1)

        instruments = [TestStubs.instrument_gbpusd(), TestStubs.instrument_usdjpy()]
        instrument_data = self.data_mapper.map_instruments(instruments)

        data = self.data_serializer.serialize(instrument_data)
        data_response = DataResponse(
            data,
            'Instrument[]',
            'BSON',
            self.data_client.last_request_id,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.data_server.send_response(data_response, self.data_client.client_id)

        time.sleep(0.2)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(instruments, response)
