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
from inv_trader.enums import Resolution, QuoteType


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
        result1 = self.data_client._get_tick_channel('audusd', 'fxcm')
        result2 = self.data_client._get_tick_channel('gbpusd', 'dukascopy')

        # Assert
        self.assertTrue(result1, 'audusd.fxcm')
        self.assertTrue(result2, 'gbpusd.fxcm')

    def test_can_create_correct_bars_channel(self):
        # Arrange
        # Act
        result1 = self.data_client._get_bar_channel('audusd', 'fxcm', 1, Resolution.SECOND, QuoteType.BID)
        result2 = self.data_client._get_bar_channel('gbpusd', 'dukascopy', 5, Resolution.MINUTE, QuoteType.MID)

        # Assert
        self.assertTrue(result1, 'audusd.fxcm-1-second[bid]')
        self.assertTrue(result2, 'gbpusd.fxcm-5-minute[mid]')

    def test_can_subscribe_to_tick_data_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.subscribe_tick_data('audusd', 'fxcm')

        # Assert
        self.assertEqual('Subscribed to audusd.fxcm.', result)

    def test_subscribing_to_tick_data_when_already_subscribed_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        self.data_client.subscribe_tick_data('audusd', 'fxcm')
        result = self.data_client.subscribe_tick_data('audusd', 'fxcm')

        # Assert
        self.assertEqual('Already subscribed to audusd.fxcm.', result)

    def test_can_unsubscribe_from_tick_data_returns_correct_message(self):
        # Arrange
        self.data_client.connect()
        self.data_client.subscribe_tick_data('audusd', 'fxcm')

        # Act
        result = self.data_client.unsubscribe_tick_data('audusd', 'fxcm')

        # Assert
        self.assertEqual('Unsubscribed from audusd.fxcm.', result)

    def test_unsubscribing_from_tick_data_when_never_subscribed_returns_correct_message(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.unsubscribe_tick_data('audusd', 'fxcm')

        # Assert
        self.assertEqual('Already unsubscribed from audusd.fxcm.', result)

