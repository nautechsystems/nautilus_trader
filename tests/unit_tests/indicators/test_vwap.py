# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import timedelta

from nautilus_trader.indicators.vwap import VolumeWeightedAveragePrice
from tests.test_kit.series import BatterySeries
from tests.test_kit.stubs import UNIX_EPOCH


class VolumeWeightedAveragePriceTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.vwap = VolumeWeightedAveragePrice()

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual('VolumeWeightedAveragePrice', self.vwap.name)

    def test_str_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual('VolumeWeightedAveragePrice()', str(self.vwap))

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.vwap).startswith('<VolumeWeightedAveragePrice() object at'))
        self.assertTrue(repr(self.vwap).endswith('>'))

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.vwap.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Act
        self.vwap.update(1.00000, 10000, UNIX_EPOCH)

        # Assert
        self.assertEqual(True, self.vwap.initialized)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.vwap.update(1.00000, 10000, UNIX_EPOCH)

        # Act
        # Assert
        self.assertEqual(1.00000, self.vwap.value)

    def test_values_with_higher_inputs_returns_expected_value(self):
        # Arrange
        # Act
        self.vwap.update(1.00000, 10000, UNIX_EPOCH)
        self.vwap.update(1.00010, 11000, UNIX_EPOCH)
        self.vwap.update(1.00020, 12000, UNIX_EPOCH)
        self.vwap.update(1.00030, 13000, UNIX_EPOCH)
        self.vwap.update(1.00040, 14000, UNIX_EPOCH)
        self.vwap.update(1.00050, 0, UNIX_EPOCH)
        self.vwap.update(1.00060, 16000, UNIX_EPOCH)
        self.vwap.update(1.00070, 17000, UNIX_EPOCH)
        self.vwap.update(1.00080, 18000, UNIX_EPOCH)
        self.vwap.update(1.00090, 19000, UNIX_EPOCH)

        # Assert
        self.assertEqual(1.0005076923076923, self.vwap.value)

    def test_values_with_all_lower_inputs_returns_expected_value(self):
        # Arrange
        # Act
        self.vwap.update(1.00100, 20000, UNIX_EPOCH)
        self.vwap.update(1.00090, 19000, UNIX_EPOCH)
        self.vwap.update(1.00080, 18000, UNIX_EPOCH)
        self.vwap.update(1.00070, 17000, UNIX_EPOCH)
        self.vwap.update(1.00060, 16000, UNIX_EPOCH)
        self.vwap.update(1.00050, 15000, UNIX_EPOCH)
        self.vwap.update(1.00040, 14000, UNIX_EPOCH)
        self.vwap.update(1.00030, 13000, UNIX_EPOCH)
        self.vwap.update(1.00020, 12000, UNIX_EPOCH)
        self.vwap.update(1.00010, 11000, UNIX_EPOCH)

        # Assert
        self.assertEqual(1.0006032258064514, self.vwap.value)

    def test_new_day_resets_values(self):
        # Arrange
        # Act
        self.vwap.update(1.00000, 10000, UNIX_EPOCH)
        self.vwap.update(1.00010, 11000, UNIX_EPOCH)
        self.vwap.update(1.00020, 12000, UNIX_EPOCH)
        self.vwap.update(1.00030, 13000, UNIX_EPOCH)
        self.vwap.update(1.00040, 14000, UNIX_EPOCH)
        self.vwap.update(1.00050, 0, UNIX_EPOCH)
        self.vwap.update(1.00060, 16000, UNIX_EPOCH)
        self.vwap.update(1.00070, 17000, UNIX_EPOCH)
        self.vwap.update(1.00080, 18000, UNIX_EPOCH)
        self.vwap.update(1.00090, 19000, UNIX_EPOCH)
        self.vwap.update(1.00000, 10000, UNIX_EPOCH + timedelta(1))

        # Assert
        self.assertEqual(1.00000, self.vwap.value)

    def test_new_day_with_first_volume_zero_returns_price_as_value(self):
        # Arrange
        # Act
        self.vwap.update(2.00000, 10000, UNIX_EPOCH)
        self.vwap.update(1.00000, 0, UNIX_EPOCH + timedelta(1))

        # Assert
        self.assertEqual(1.00000, self.vwap.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for i in range(100):
            self.vwap.update(1.00000, 10000, UNIX_EPOCH)

        # Act
        self.vwap.reset()  # No assertion errors.
        self.assertFalse(self.vwap.initialized)

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in BatterySeries.create():
            self.vwap.update(point, 10000, UNIX_EPOCH)
            output.append(self.vwap.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
