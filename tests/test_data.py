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
import datetime
import pytz
import time

from decimal import Decimal
from typing import List

from inv_trader.data import LiveDataClient
from inv_trader.objects import Tick, Bar
from inv_trader.enums import Venue, Resolution, QuoteType


class ObjectStorer:
    """"
    A test class which stores the given objects.
    """
    def __init__(self):
        """
        Initializes a new instance of the ObjectStorer class.
        """
        self._store = []

    @property
    def count(self) -> int:
        """
        :return: The count of objects stored.
        """
        return len(self._store)

    @property
    def get_store(self) -> List[object]:
        """"
        return: The internal object store.
        """
        return self._store

    def store(self, obj: object):
        """"
        Store the given object.
        """
        print(f"Storing {obj}")
        self._store.append(obj)


class LiveDataClientTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.data_client = LiveDataClient()
        self.redis_tester = redis.StrictRedis(host='localhost', port=6379, db=0)

    # Fixture Tear Down
    def tearDown(self):
        #self.data_client.disconnect()
        print(self.data_client.dispose())

    def test_can_connect_to_live_db(self):
        # Act
        result = self.data_client.connect()

        # Assert
        self.assertEqual('Connected to live database at localhost:6379.', result)

    def test_can_disconnect_from_live_db(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.disconnect()

        # Assert
        self.assertEqual('Unsubscribed from tick_data [].', result[0])
        self.assertEqual('Unsubscribed from bars_data [].', result[1])
        self.assertEqual('Disconnected from live database at localhost:6379.', result[2])

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

    def test_can_create_correct_tick_channel(self):
        # Arrange
        # Act
        result1 = self.data_client._get_tick_channel_name('audusd', Venue.FXCM)
        result2 = self.data_client._get_tick_channel_name('gbpusd', Venue.DUKASCOPY)

        # Assert
        self.assertTrue(result1, 'audusd.fxcm')
        self.assertTrue(result2, 'gbpusd.fxcm')

    def test_can_create_correct_bars_channel(self):
        # Arrange
        # Act
        result1 = self.data_client._get_bar_channel_name('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)
        result2 = self.data_client._get_bar_channel_name('gbpusd', Venue.DUKASCOPY, 5, Resolution.MINUTE, QuoteType.MID)

        # Assert
        self.assertTrue(result1, 'audusd.fxcm-1-second[bid]')
        self.assertTrue(result2, 'gbpusd.fxcm-5-minute[mid]')

    def test_can_subscribe_to_tick_data_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.subscribe_tick_data('audusd', Venue.FXCM)
        tick_channels = self.data_client.subscriptions_ticks
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual('Subscribed to audusd.fxcm.', result)
        self.assertEqual("['audusd.fxcm']", str(tick_channels))
        self.assertTrue(any('audusd.fxcm' for channels in all_channels))

    def test_subscribing_to_tick_data_when_already_subscribed_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        self.data_client.subscribe_tick_data('audusd', Venue.FXCM)
        result = self.data_client.subscribe_tick_data('audusd', Venue.FXCM)
        tick_channels = self.data_client.subscriptions_ticks
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual('Already subscribed to audusd.fxcm.', result)
        self.assertEqual("['audusd.fxcm']", str(tick_channels))
        self.assertTrue(any('audusd.fxcm' for channels in all_channels))

    def test_can_unsubscribe_from_tick_data_returns_correct_message(self):
        # Arrange
        self.data_client.connect()
        self.data_client.subscribe_tick_data('audusd', Venue.FXCM)

        # Act
        result = self.data_client.unsubscribe_tick_data('audusd', Venue.FXCM)
        tick_channels = self.data_client.subscriptions_ticks
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual('Unsubscribed from audusd.fxcm.', result)
        self.assertEqual("[]", str(tick_channels))
        print(all_channels)
        #self.assertFalse(any('audusd.fxcm' for channels in all_channels))

    def test_unsubscribing_from_tick_data_when_never_subscribed_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.unsubscribe_tick_data('audusd', Venue.FXCM)

        # Assert
        self.assertEqual('Already unsubscribed from audusd.fxcm.', result)

    def test_can_subscribe_to_bar_data_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.subscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)
        bar_channels = self.data_client.subscriptions_bars
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual('Subscribed to audusd.fxcm-1-second[bid].', result)
        self.assertEqual("['audusd.fxcm-1-second[bid]']", str(bar_channels))
        self.assertTrue(any('audusd.fxcm-1-second[bid]' for channels in all_channels))

    def test_subscribing_to_bar_data_when_already_subscribed_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        self.data_client.subscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)
        result = self.data_client.subscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)
        bar_channels = self.data_client.subscriptions_bars
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual('Already subscribed to audusd.fxcm-1-second[bid].', result)
        self.assertEqual("['audusd.fxcm-1-second[bid]']", str(bar_channels))
        self.assertTrue(any('audusd.fxcm-1-second[bid]' for channels in all_channels))

    def test_can_unsubscribe_from_bar_data_returns_correct_message(self):
        # Arrange
        self.data_client.connect()
        self.data_client.subscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)

        # Act
        result = self.data_client.unsubscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)
        bar_channels = self.data_client.subscriptions_bars
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual('Unsubscribed from audusd.fxcm-1-second[bid].', result)
        self.assertEqual("[]", str(bar_channels))
        #self.assertFalse(any('audusd.fxcm' for channels in all_channels))

    def test_unsubscribing_from_bar_data_when_never_subscribed_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.unsubscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)
        bar_channels = self.data_client.subscriptions_bars
        all_channels = self.data_client.subscriptions_all

        # Assert
        self.assertEqual('Already unsubscribed from audusd.fxcm-1-second[bid].', result)
        self.assertEqual("[]", str(bar_channels))
        #self.assertFalse(any('audusd.fxcm' in channel for channel in all_channels))

    def test_disconnecting_when_subscribed_to_multiple_channels_then_unsubscribes(self):
        # Arrange
        self.data_client.connect()
        self.data_client.subscribe_tick_data('audusd', Venue.FXCM)
        self.data_client.subscribe_tick_data('gbpusd', Venue.FXCM)
        self.data_client.subscribe_tick_data('eurjpy', Venue.FXCM)
        self.data_client.subscribe_tick_data('usdcad', Venue.FXCM)

        # Act
        result = self.data_client.disconnect()

        # Assert
        self.assertEqual("Unsubscribed from tick_data ['audusd.fxcm', 'eurjpy.fxcm', 'gbpusd.fxcm', 'usdcad.fxcm'].", result[0])

    def test_can_parse_ticks(self):
        # Arrange
        tick = Tick(
            'AUDUSD',
            Venue.FXCM,
            Decimal('1.00000'),
            Decimal('1.00001'),
            datetime.datetime(2018, 1, 1, 19, 59, 1, 0, pytz.UTC))

        # Act
        result = self.data_client._parse_tick(
            'audusd.fxcm',
            '1.00000,1.00001,2018-01-01T19:59:01.000Z')

        # Assert
        self.assertEqual(tick.symbol, result.symbol)
        self.assertEqual(tick.venue, result.venue)
        self.assertEqual(tick.bid, result.bid)
        self.assertEqual(tick.ask, result.ask)
        self.assertEqual(tick.timestamp, result.timestamp)
        self.assertEqual('Tick: AUDUSD.FXCM,1.00000,1.00001,2018-01-01 19:59:01+00:00', str(result))

    def test_can_parse_bars(self):
        # Arrange
        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime.datetime(2018, 1, 1, 19, 59, 1, 0, pytz.UTC))

        # Act
        result = self.data_client._parse_bar(
            '1.00001,1.00004,1.00003,1.00002,100000,2018-01-01T19:59:01.000Z')

        # Assert
        self.assertEqual(bar.open, result.open)
        self.assertEqual(bar.high, result.high)
        self.assertEqual(bar.low, result.low)
        self.assertEqual(bar.close, result.close)
        self.assertEqual(bar.volume, result.volume)
        self.assertEqual(bar.timestamp, result.timestamp)
        self.assertEqual(
            'Bar: 1.00001,1.00004,1.00003,1.00002,100000,2018-01-01 19:59:01+00:00', str(result))

    def test_tick_handler_with_no_subscribers_prints(self):
        # Arrange
        tick = Tick(
            'AUDUSD',
            Venue.FXCM,
            Decimal('1.00000'),
            Decimal('1.00001'),
            datetime.datetime(2018, 1, 1, 19, 59, 1, 0, pytz.UTC))

        # Act
        self.data_client._tick_handler(
            {'channel': b'audusd.fxcm', 'data': b'1.00000,1.00001,2018-01-01T19:59:01.000Z'})

        # Assert
        # Should print to console.

    def test_bar_handler_with_no_subscribers_prints(self):
        # Arrange
        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime.datetime(2018, 1, 1, 19, 59, 1, 0, pytz.UTC))

        # Act
        self.data_client._bar_handler(
            {'channel': b'audusd.fxcm-1-second[bid]',
             'data': b'1.00001,1.00004,1.00003,1.00002,100000,2018-01-01 19:59:01+00:00'})

        # Assert
        # Should print to console.

    def test_can_receive_one_tick(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        log = self.data_client.subscribe_tick_data('audusd', Venue.FXCM, storer.store)

        tick = Tick(
            'AUDUSD',
            Venue.FXCM,
            Decimal('1.00000'),
            Decimal('1.00001'),
            datetime.datetime(2018, 1, 1, 19, 59, 1, 0, pytz.UTC))

        # Act
        self.redis_tester.publish(
            'audusd.fxcm',
            '1.00000,1.00001,2018-01-01T19:59:01.000Z')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(str(tick), str(storer.get_store[0]))

    def test_can_receive_many_ticks(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        self.data_client.subscribe_tick_data('audusd', Venue.FXCM, storer.store)

        # Act
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00001,2018-01-01T19:59:01.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00002,2018-01-01T20:00:02.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00003,2018-01-01T20:00:03.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00004,2018-01-01T20:00:04.000Z')
        self.redis_tester.publish('audusd.fxcm', '1.00000,1.00005,2018-01-01T20:00:05.000Z')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(5, storer.count)
        self.assertEqual('Tick: AUDUSD.FXCM,1.00000,1.00005,2018-01-01 20:00:05+00:00', str(storer.get_store[4]))

    def test_can_receive_ticks_from_multiple_subscribers(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        self.data_client.subscribe_tick_data('audusd', Venue.FXCM, storer.store)
        self.data_client.subscribe_tick_data('audusd', Venue.FXCM, storer.store)
        self.data_client.subscribe_tick_data('gbpusd', Venue.FXCM, storer.store)
        self.data_client.subscribe_tick_data('eurusd', Venue.FXCM, storer.store)

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
        self.assertEqual('Tick: EURUSD.FXCM,1.00000,1.00005,2018-01-01 20:00:05+00:00', str(storer.get_store[14]))

    def test_can_receive_bar(self):
        # Arrange
        storer = ObjectStorer()
        self.data_client.connect()
        self.data_client.subscribe_bar_data('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID, storer.store)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime.datetime(2018, 1, 1, 19, 59, 1, 0, pytz.UTC))

        # Act
        self.redis_tester.publish(
            'audusd.fxcm-1-second[bid]',
            '1.00001,1.00004,1.00003,1.00002,100000,2018-01-01 19:59:01+00:00')

        # Assert
        time.sleep(0.1)  # Allow threads to work.
        self.assertEqual(str(bar), str(storer.get_store[0]))
