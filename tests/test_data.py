#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_data.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import redis
import time

from datetime import datetime, timezone
from decimal import Decimal

from inv_trader.data import LiveDataClient
from inv_trader.model.objects import Symbol, Tick, BarType, Bar
from inv_trader.model.enums import Venue, Resolution, QuoteType
from test_kit.objects import ObjectStorer


class LiveDataClientTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.data_client = LiveDataClient()
        self.redis_tester = redis.StrictRedis(host='localhost', port=6379, db=0)

    # Fixture Tear Down
    def tearDown(self):
        self.data_client.disconnect()

    def test_can_connect_to_live_db(self):
        # Act
        self.data_client.connect()

        # Assert
        # Does not throw exception.

    def test_can_disconnect_from_live_db(self):
        # Arrange
        self.data_client.connect()

        # Act
        self.data_client.disconnect()

        # Assert
        # Does not throw exception.

    def test_is_connected_when_not_connected_returns_false(self):
        # Arrange
        self.data_client.connect()
        self.data_client.disconnect()

        # Act
        result = self.data_client.is_connected

        # Assert
        self.assertFalse(result)

    def test_is_connected_when_connected_returns_true(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.is_connected

        # Assert
        self.assertTrue(result)

    def test_can_create_correct_tick_channel_name(self):
        # Arrange
        # Act
        result1 = self.data_client._get_tick_channel_name(Symbol('AUDUSD', Venue.FXCM))
        result2 = self.data_client._get_tick_channel_name(Symbol('GBPUSD', Venue.DUKASCOPY))

        # Assert
        self.assertEqual(result1, 'audusd.fxcm')
        self.assertEqual(result2, 'gbpusd.dukascopy')

    def test_can_create_correct_bar_channel_name(self):
        # Arrange
        # Act
        result1 = self.data_client._get_bar_channel_name(BarType(Symbol('AUDUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID))
        result2 = self.data_client._get_bar_channel_name(BarType(Symbol('GBPUSD', Venue.DUKASCOPY), 5, Resolution.MINUTE, QuoteType.MID))

        # Assert
        self.assertEqual(result1, 'audusd.fxcm-1-second[bid]')
        self.assertEqual(result2, 'gbpusd.dukascopy-5-minute[mid]')

    def test_can_subscribe_to_tick_data(self):
        # Arrange
        self.data_client.connect()

        # Act
        self.data_client.subscribe_ticks(Symbol('AUDUSD', Venue.FXCM))
        tick_channels = self.data_client.subscriptions_ticks
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual("['audusd.fxcm']", str(tick_channels))
        self.assertTrue(any('audusd.fxcm' for channels in all_channels))

    def test_can_unsubscribe_from_tick_data(self):
        # Arrange
        self.data_client.connect()
        self.data_client.subscribe_ticks(Symbol('AUDUSD', Venue.FXCM))

        # Act
        self.data_client.unsubscribe_ticks(Symbol('AUDUSD', Venue.FXCM))
        tick_channels = self.data_client.subscriptions_ticks

        # Assert
        self.assertEqual("[]", str(tick_channels))

    def test_can_subscribe_to_bar_data(self):
        # Arrange
        self.data_client.connect()

        # Act
        self.data_client.subscribe_bars(BarType(Symbol('AUDUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID))
        bar_channels = self.data_client.subscriptions_bars
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual("['audusd.fxcm-1-second[bid]']", str(bar_channels))
        self.assertTrue(any('audusd.fxcm-1-second[bid]' in channel for channel in all_channels))

    def test_can_unsubscribe_from_bar_data(self):
        # Arrange
        self.data_client.connect()
        self.data_client.subscribe_bars(BarType(Symbol('AUDUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID))

        # Act
        self.data_client.unsubscribe_bars(BarType(Symbol('AUDUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID))
        bar_channels = self.data_client.subscriptions_bars
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual("[]", str(bar_channels))
        self.assertFalse(any('audusd.fxcm-1-second[bid]' in channel for channel in all_channels))

    def test_disconnecting_when_subscribed_to_multiple_channels_then_unsubscribes(self):
        # Arrange
        self.data_client.connect()
        self.data_client.subscribe_ticks(Symbol('AUDUSD', Venue.FXCM))
        self.data_client.subscribe_ticks(Symbol('GBPUSD', Venue.FXCM))
        self.data_client.subscribe_ticks(Symbol('EURJPY', Venue.FXCM))
        self.data_client.subscribe_ticks(Symbol('USDCAD', Venue.FXCM))

        # Act
        self.data_client.disconnect()
        result = self.data_client.subscriptions_ticks

        # Assert
        self.assertEqual(0, len(result))

    def test_can_parse_tick_symbol(self):
        # Arrange
        symbol = Symbol('AUDUSD', Venue.FXCM)

        # Act
        result = self.data_client._parse_tick_symbol('audusd.fxcm')

        # Assert
        self.assertEqual(symbol, result)
        self.assertEqual('AUDUSD.FXCM', str(result))

    def test_can_parse_ticks(self):
        # Arrange
        symbol = Symbol('AUDUSD', Venue.FXCM)
        tick = Tick(symbol,
                    Decimal('1.00000'),
                    Decimal('1.00001'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        # Act
        result = self.data_client._parse_tick(symbol, '1.00000,1.00001,2018-01-01T19:59:01.000Z')

        # Assert
        self.assertEqual(tick, result)
        self.assertEqual('Tick(AUDUSD.FXCM,1.00000,1.00001,2018-01-01T19:59:01+00:00)', str(result))

    def test_can_parse_bars(self):
        # Arrange
        bar = Bar(Decimal('1.00001'),
                  Decimal('1.00004'),
                  Decimal('1.00002'),
                  Decimal('1.00003'),
                  100000,
                  datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        # Act
        result = self.data_client._parse_bar(
            '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T19:59:01.000Z')

        # Assert
        self.assertEqual(bar, result)
        self.assertEqual(
            'Bar(1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T19:59:01+00:00)', str(result))

    def test_can_parse_bar_type(self):
        # Arrange
        bar_type = BarType(Symbol('audusd', Venue.FXCM),
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)
        # Act
        result = self.data_client._parse_bar_type(str(bar_type))

        # Assert
        self.assertEqual(bar_type, result)

    def test_tick_handler_with_no_subscribers_prints(self):
        # Arrange
        # Act
        self.data_client._handle_tick(
            {'channel': b'audusd.fxcm', 'data': b'1.00000,1.00001,2018-01-01T19:59:01.000Z'})

        # Assert
        # Should print to console.

    def test_bar_handler_with_no_subscribers_prints(self):
        # Arrange
        # Act
        self.data_client._handle_bar(
            {'channel': b'audusd.fxcm-1-second[bid]',
             'data': b'1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T19:59:01+00:00'})

        # Assert
        # Should print to console.

    def test_can_receive_one_tick(self):
        # Arrange
        storer = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue.FXCM)

        self.data_client.connect()
        self.data_client.subscribe_ticks(symbol, storer.store)

        tick = Tick(symbol,
                    Decimal('1.00000'),
                    Decimal('1.00001'),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        # Act
        self.redis_tester.publish(
            'audusd.fxcm',
            '1.00000,1.00001,2018-01-01T19:59:01.000Z')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(tick, storer.get_store[0])

    def test_can_receive_many_ticks(self):
        # Arrange
        storer = ObjectStorer()
        symbol = Symbol('AUDUSD', Venue.FXCM)

        self.data_client.connect()
        self.data_client.subscribe_ticks(symbol, storer.store)

        # Act
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00001,2018-01-01T19:59:01.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00002,2018-01-01T20:00:02.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00003,2018-01-01T20:00:03.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00004,2018-01-01T20:00:04.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00005,2018-01-01T20:00:05.000Z')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(5, storer.count)
        self.assertEqual('Tick(AUDUSD.FXCM,1.00000,1.00005,2018-01-01T20:00:05+00:00)', str(storer.get_store[4]))

    def test_can_receive_ticks_from_multiple_subscribers(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        self.data_client.subscribe_ticks(Symbol('AUDUSD', Venue.FXCM), storer.store)
        self.data_client.subscribe_ticks(Symbol('GBPUSD', Venue.FXCM), storer.store)
        self.data_client.subscribe_ticks(Symbol('EURUSD', Venue.FXCM), storer.store)

        # Act
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00001,2018-01-01T19:59:01.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00002,2018-01-01T20:00:02.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00003,2018-01-01T20:00:03.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00004,2018-01-01T20:00:04.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00005,2018-01-01T20:00:05.000Z')
        self.redis_tester.publish('gbpusd.fxcm', '1.00000,1.00001,2018-01-01T19:59:01.000Z')
        self.redis_tester.publish('gbpusd.fxcm', '1.00000,1.00002,2018-01-01T20:00:02.000Z')
        self.redis_tester.publish('gbpusd.fxcm', '1.00000,1.00003,2018-01-01T20:00:03.000Z')
        self.redis_tester.publish('gbpusd.fxcm', '1.00000,1.00004,2018-01-01T20:00:04.000Z')
        self.redis_tester.publish('gbpusd.fxcm', '1.00000,1.00005,2018-01-01T20:00:05.000Z')
        self.redis_tester.publish('eurusd.fxcm', '1.00000,1.00001,2018-01-01T19:59:01.000Z')
        self.redis_tester.publish('eurusd.fxcm', '1.00000,1.00002,2018-01-01T20:00:02.000Z')
        self.redis_tester.publish('eurusd.fxcm', '1.00000,1.00003,2018-01-01T20:00:03.000Z')
        self.redis_tester.publish('eurusd.fxcm', '1.00000,1.00004,2018-01-01T20:00:04.000Z')
        self.redis_tester.publish('eurusd.fxcm', '1.00000,1.00005,2018-01-01T20:00:05.000Z')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(15, storer.count)
        self.assertEqual('Tick(EURUSD.FXCM,1.00000,1.00005,2018-01-01T20:00:05+00:00)', str(storer.get_store[14]))

    def test_can_receive_bar(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        self.data_client.subscribe_bars(BarType(Symbol('AUDUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID), storer.store_both)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            100000,
            datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        # Act
        self.redis_tester.publish(
            'audusd.fxcm-1-second[bid]',
            '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T19:59:01+00:00')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(str(bar), str(storer.get_store[1]))

    def test_can_receive_many_bars(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        self.data_client.subscribe_bars(BarType(Symbol('AUDUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID), storer.store_both)

        # Act
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:00+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:01+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:02+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:03+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:04+00:00')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(10, storer.count)  # All bar types and bars.
        self.assertEqual('Bar(1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:04+00:00)', str(storer.get_store[9]))

    def test_can_receive_bars_from_multiple_subscribers(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        self.data_client.subscribe_bars(BarType(Symbol('AUDUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID), storer.store_both)
        self.data_client.subscribe_bars(BarType(Symbol('GBPUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID), storer.store_both)
        self.data_client.subscribe_bars(BarType(Symbol('EURUSD', Venue.FXCM), 1, Resolution.SECOND, QuoteType.BID), storer.store_both)

        # Act
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:00+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:01+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:02+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:03+00:00')
        self.redis_tester.publish('audusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:04+00:00')
        self.redis_tester.publish('eurusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:00+00:00')
        self.redis_tester.publish('eurusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:01+00:00')
        self.redis_tester.publish('eurusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:02+00:00')
        self.redis_tester.publish('eurusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:03+00:00')
        self.redis_tester.publish('eurusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:04+00:00')
        self.redis_tester.publish('gbpusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:00+00:00')
        self.redis_tester.publish('gbpusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:01+00:00')
        self.redis_tester.publish('gbpusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:02+00:00')
        self.redis_tester.publish('gbpusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:03+00:00')
        self.redis_tester.publish('gbpusd.fxcm-1-second[bid]',
                                  '1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:04+00:00')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(30, storer.count)  # All bar types and bars.
        self.assertEqual('Bar(1.00001,1.00004,1.00002,1.00003,100000,2018-01-01T12:00:04+00:00)', str(storer.get_store[29]))

    def test_can_add_bartype_to_dict(self):
        # Arrange
        bar_type = BarType(Symbol('audusd', Venue.FXCM),
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            100000,
            datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        # Act
        bar_dictionary = {bar_type: bar}

        # Assert
        self.assertEqual(bar_dictionary[bar_type], bar)

    def test_can_update_all_instruments(self):
        # Arrange
        self.data_client.connect()

        # Act
        self.data_client.update_all_instruments()
        audusd = self.data_client.get_instrument(Symbol('AUDUSD', Venue.FXCM))

        # Assert
        self.assertTrue(len(self.data_client.instruments) >= 1)
        print(audusd.quote_currency)
        print(audusd.security_type)
        print(audusd.tick_decimals)
        print(audusd.tick_size)
        print(audusd.tick_value)
        print(audusd.target_direct_spread)
        print(audusd.rollover_interest_buy)
        print(audusd.rollover_interest_sell)

    def test_can_update_instrument(self):
        # Arrange
        self.data_client.connect()
        symbol = Symbol('AUDUSD', Venue.FXCM)

        # Act
        self.data_client.update_instrument(symbol)

        # Assert
        self.assertTrue(symbol in self.data_client._instruments)
