# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_data.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid
import unittest
import time
import zmq

from nautilus_trader.core.types import GUID
from nautilus_trader.live.logger import LiveLogger
from nautilus_trader.model.objects import Venue, Symbol, Price, Tick, Bar
from nautilus_trader.live.data import LiveDataClient
from nautilus_trader.network.responses import DataResponse
from nautilus_trader.serialization.data import DataMapper, BsonDataSerializer, BsonInstrumentSerializer
from nautilus_trader.serialization.serializers import MsgPackResponseSerializer
from test_kit.objects import ObjectStorer
from test_kit.stubs import TestStubs
from test_kit.mocks import MockPublisher, MockServer

UTF8 = 'utf-8'
UNIX_EPOCH = TestStubs.unix_epoch()


class LiveDataClientTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.zmq_context = zmq.Context()
        self.data_mapper = DataMapper()
        self.data_serializer = BsonDataSerializer()
        self.response_serializer = MsgPackResponseSerializer()
        self.tick_req_port = 55501
        self.bar_req_port = 55503
        self.inst_req_port = 55505
        self.tick_publisher = MockPublisher(zmq_context=self.zmq_context, port=55502)
        self.bar_publisher = MockPublisher(zmq_context=self.zmq_context, port=55504)
        self.inst_publisher = MockPublisher(zmq_context=self.zmq_context, port=55506)

        self.data_client = LiveDataClient(venue=Venue('FXCM'), zmq_context=zmq.Context(), logger=LiveLogger())

    # Fixture Tear Down
    def tearDown(self):
        self.data_client.disconnect()
        self.tick_publisher.stop()
        self.bar_publisher.stop()
        self.inst_publisher.stop()
        time.sleep(0.1)

    def test_can_connect_and_disconnect_from_service(self):
        # Act
        self.data_client.connect()

        # Assert
        time.sleep(0.1)
        # Does not throw exception

    def test_can_subscribe_to_tick_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue('FXCM'))

        # Act
        self.data_client.subscribe_ticks(symbol, handler=data_receiver.store)

        # Assert
        self.assertIn(Symbol('AUDUSD', Venue('FXCM')), self.data_client.subscribed_ticks())

    def test_can_unsubscribe_from_tick_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue('FXCM'))

        # Act
        self.data_client.subscribe_ticks(symbol, handler=data_receiver.store)
        self.data_client.unsubscribe_ticks(symbol, handler=data_receiver.store)

        # Assert
        self.assertNotIn(Symbol('AUDUSD', Venue('FXCM')), self.data_client.subscribed_ticks())

    def test_can_receive_published_tick_data(self):
        # Arrange
        self.data_client.connect()

        symbol = Symbol('AUDUSD', Venue('FXCM'))
        data_receiver = ObjectStorer()

        tick = Tick(symbol,
                    Price('1.00000'),
                    Price('1.00001'),
                    UNIX_EPOCH)

        # Act
        self.data_client.subscribe_ticks(symbol, handler=data_receiver.store)

        time.sleep(0.1)
        self.tick_publisher.publish(symbol.value, str(tick).encode(UTF8))
        time.sleep(0.1)

        # Assert
        self.assertEqual(1, len(data_receiver.get_store()))
        self.assertEqual(tick, data_receiver.get_store()[0])

    def test_can_subscribe_to_bar_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act
        self.data_client.subscribe_bars(bar_type, handler=data_receiver.store_2)

        # Assert
        self.assertIn(bar_type, self.data_client.subscribed_bars())

    def test_can_unsubscribe_from_bar_data(self):
        # Arrange
        self.data_client.connect()

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
        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  UNIX_EPOCH)

        # Act
        self.data_client.subscribe_bars(bar_type, handler=data_receiver.store_2)

        time.sleep(0.1)
        self.bar_publisher.publish(str(bar_type), str(bar).encode(UTF8))
        time.sleep(0.1)

        # Assert
        self.assertEqual(1, len(data_receiver.get_store()))
        self.assertEqual((bar_type, bar), data_receiver.get_store()[0])

    def test_can_subscribe_to_instrument_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue('FXCM'))

        # Act
        self.data_client.subscribe_instrument(symbol, handler=data_receiver.store)

        # Assert
        self.assertIn(Symbol('AUDUSD', Venue('FXCM')), self.data_client.subscribed_instruments())

    def test_can_unsubscribe_from_instrument_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue('FXCM'))

        # Act
        self.data_client.subscribe_instrument(symbol, handler=data_receiver.store)
        self.data_client.unsubscribe_instrument(symbol, handler=data_receiver.store)

        # Assert
        self.assertNotIn(Symbol('AUDUSD', Venue('FXCM')), self.data_client.subscribed_instruments())

    def test_can_receive_published_instrument_data(self):
        # Arrange
        self.data_client.connect()

        instrument = TestStubs.instrument_gbpusd()
        data_receiver = ObjectStorer()
        serializer = BsonInstrumentSerializer()

        # Act
        self.data_client.subscribe_instrument(instrument.symbol, handler=data_receiver.store)

        time.sleep(0.1)
        self.inst_publisher.publish(instrument.symbol.value, serializer.serialize(instrument))
        time.sleep(0.1)

        # Assert
        self.assertEqual(1, len(data_receiver.get_store()))
        self.assertEqual(instrument, data_receiver.get_store()[0])

    def test_can_request_tick_data(self):
        # Arrange
        symbol = Symbol('AUDUSD', Venue('FXCM'))
        tick = Tick(symbol,
                    Price('1.00000'),
                    Price('1.00001'),
                    UNIX_EPOCH)
        ticks = [tick, tick, tick, tick, tick]
        tick_data = self.data_mapper.map_ticks(ticks)

        data = self.data_serializer.serialize(tick_data)
        data_response = DataResponse(data, 'BSON', GUID(uuid.uuid4()), GUID(uuid.uuid4()), UNIX_EPOCH)
        response_bytes = self.response_serializer.serialize(data_response)
        server = MockServer(zmq_context=self.zmq_context, port=self.tick_req_port, responses=[response_bytes])
        server.start()

        self.data_client.connect()

        data_receiver = ObjectStorer()

        # Act
        self.data_client.request_ticks(symbol, UNIX_EPOCH, UNIX_EPOCH, data_receiver.store)

        time.sleep(0.1)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(ticks, response)
        server.stop()

    def test_can_request_bar_data(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        bar = Bar(Price('1.00001'),
                  Price('1.00004'),
                  Price('1.00002'),
                  Price('1.00003'),
                  100000,
                  UNIX_EPOCH)
        bars = [bar, bar, bar, bar, bar]
        bar_data = self.data_mapper.map_bars(bars, bar_type)

        data = self.data_serializer.serialize(bar_data)
        data_response = DataResponse(data, 'BSON', GUID(uuid.uuid4()), GUID(uuid.uuid4()), UNIX_EPOCH)
        response_bytes = self.response_serializer.serialize(data_response)
        server = MockServer(zmq_context=self.zmq_context, port=self.bar_req_port, responses=[response_bytes])
        server.start()

        self.data_client.connect()

        data_receiver = ObjectStorer()

        # Act
        self.data_client.request_bars(bar_type, UNIX_EPOCH, UNIX_EPOCH, data_receiver.store_2)

        time.sleep(0.1)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(bar_type, response[0])
        self.assertEqual(bar, response[1][0])
        server.stop()

    def test_can_request_instrument_data(self):
        # Arrange
        symbol = Symbol('GBPUSD', Venue('FXCM'))
        instruments = [TestStubs.instrument_gbpusd()]
        instrument_data = self.data_mapper.map_instruments(instruments)

        data = self.data_serializer.serialize(instrument_data)
        data_response = DataResponse(data, 'BSON', GUID(uuid.uuid4()), GUID(uuid.uuid4()), UNIX_EPOCH)
        response_bytes = self.response_serializer.serialize(data_response)
        server = MockServer(zmq_context=self.zmq_context, port=self.inst_req_port, responses=[response_bytes])
        server.start()

        self.data_client.connect()

        data_receiver = ObjectStorer()

        # Act
        self.data_client.request_instrument(symbol, data_receiver.store)

        time.sleep(0.1)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(instruments[0], response)
        server.stop()

    def test_can_request_instruments_data(self):
        # Arrange
        instruments = [TestStubs.instrument_gbpusd(), TestStubs.instrument_usdjpy()]
        instrument_data = self.data_mapper.map_instruments(instruments)

        data = self.data_serializer.serialize(instrument_data)
        data_response = DataResponse(data, 'BSON', GUID(uuid.uuid4()), GUID(uuid.uuid4()), UNIX_EPOCH)
        response_bytes = self.response_serializer.serialize(data_response)
        server = MockServer(zmq_context=self.zmq_context, port=self.inst_req_port, responses=[response_bytes])
        server.start()

        self.data_client.connect()

        data_receiver = ObjectStorer()

        # Act
        self.data_client.request_instruments(data_receiver.store)

        time.sleep(0.1)
        response = data_receiver.get_store()[0]

        # Assert
        self.assertEqual(instruments, response)
        server.stop()
