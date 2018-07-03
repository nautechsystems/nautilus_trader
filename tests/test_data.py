#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_data.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.data import LiveDataClient
from inv_trader.enums import Venue, Resolution, QuoteType


class LiveDataClientTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.data_client = LiveDataClient()

    # Fixture Tear Down
    def tearDown(self):
        self.data_client.disconnect()

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
        result1 = self.data_client._get_tick_channel('audusd', Venue.FXCM)
        result2 = self.data_client._get_tick_channel('gbpusd', Venue.DUKASCOPY)

        # Assert
        self.assertTrue(result1, 'audusd.fxcm')
        self.assertTrue(result2, 'gbpusd.fxcm')

    def test_can_create_correct_bars_channel(self):
        # Arrange
        # Act
        result1 = self.data_client._get_bar_channel('audusd', Venue.FXCM, 1, Resolution.SECOND, QuoteType.BID)
        result2 = self.data_client._get_bar_channel('gbpusd', Venue.DUKASCOPY, 5, Resolution.MINUTE, QuoteType.MID)

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
        self.assertTrue(any('audusd.fxcm' for channels in all_channels))

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
        #self.assertFalse(any('audusd.fxcm' for channels in all_channels))

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
        self.assertEqual("Unsubscribed from tick_data ['audusd.fxcm'].", result[0])

    def test_can_parse_ticks(self):
        # Arrange
        # Act
        result1 = self.data_client._parse_tick('audusd.fxcm', '1.00000,1.00001,2018-01-01T19:59:01.000Z')
        result2 = self.data_client._parse_tick('gbpusd.fxcm', '1.50000,1.55555,2007-01-01T01:00:01.000Z')

        # Assert
        self.assertEqual('Tick: AUDUSD.FXCM,1.00000,1.00001,2018-01-01 19:59:01+00:00', str(result1))
        self.assertEqual('Tick: GBPUSD.FXCM,1.50000,1.55555,2007-01-01 01:00:01+00:00', str(result2))

