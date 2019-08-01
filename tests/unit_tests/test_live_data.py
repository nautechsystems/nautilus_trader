# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_data.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time
import zmq

from datetime import datetime, timezone

from nautilus_trader.common.logger import TestLogger
from nautilus_trader.model.enums import Venue, Resolution, QuoteType
from nautilus_trader.model.objects import Symbol, Price, Tick, BarSpecification, BarType, Bar
from nautilus_trader.live.data import LiveDataClient
from nautilus_trader.serialization.data import BsonInstrumentSerializer
from test_kit.objects import ObjectStorer
from test_kit.stubs import TestStubs
from test_kit.mocks import MockPublisher

UTF8 = 'utf-8'


class LiveDataClientTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        zmq_context = zmq.Context()
        self.tick_publisher = MockPublisher(zmq_context=zmq_context, port=55502)
        self.bar_publisher = MockPublisher(zmq_context=zmq_context, port=55504)
        self.inst_publisher = MockPublisher(zmq_context=zmq_context, port=55506)

        self.data_client = LiveDataClient(venue=Venue.FXCM, zmq_context=zmq.Context(), logger=TestLogger())

    # Fixture Tear Down
    def tearDown(self):
        self.data_client.disconnect()
        self.tick_publisher.stop()
        self.bar_publisher.stop()
        self.inst_publisher.stop()

    def test_can_connect_and_disconnect_from_live_db(self):
        # Act
        self.data_client.connect()

        # Assert
        time.sleep(1)
        # Does not throw exception

    def test_can_subscribe_to_tick_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue.FXCM)

        # Act
        self.data_client.subscribe_ticks(symbol, handler=data_receiver.store)

        # Assert
        self.assertIn(Symbol('AUDUSD', Venue.FXCM), self.data_client.subscribed_ticks())

    def test_can_unsubscribe_from_tick_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue.FXCM)

        # Act
        self.data_client.subscribe_ticks(symbol, handler=data_receiver.store)
        self.data_client.unsubscribe_ticks(symbol, handler=data_receiver.store)

        # Assert
        self.assertNotIn(Symbol('AUDUSD', Venue.FXCM), self.data_client.subscribed_ticks())

    def test_can_receive_published_tick_data(self):
        # Arrange
        self.data_client.connect()

        symbol = Symbol('AUDUSD', Venue.FXCM)
        data_receiver = ObjectStorer()

        tick = Tick(symbol,
                    Price('1.00000'),
                    Price('1.00001'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

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
                  datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

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
        symbol = Symbol('AUDUSD', Venue.FXCM)

        # Act
        self.data_client.subscribe_instrument(symbol, handler=data_receiver.store)

        # Assert
        self.assertIn(Symbol('AUDUSD', Venue.FXCM), self.data_client.subscribed_instruments())

    def test_can_unsubscribe_from_instrument_data(self):
        # Arrange
        self.data_client.connect()

        data_receiver = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue.FXCM)

        # Act
        self.data_client.subscribe_instrument(symbol, handler=data_receiver.store)
        self.data_client.unsubscribe_instrument(symbol, handler=data_receiver.store)

        # Assert
        self.assertNotIn(Symbol('AUDUSD', Venue.FXCM), self.data_client.subscribed_instruments())

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
