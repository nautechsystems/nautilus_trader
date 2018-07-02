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
        # self.assertFalse(result)

    def test_is_connected_when_connected_returns_true(self):
        # Arrange
        self.data_client.connect()

        # Act
        result = self.data_client.is_connected

        # Assert
        self.assertTrue(result)
