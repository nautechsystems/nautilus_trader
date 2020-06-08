# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.indicators.average.hma import HullMovingAverage
from tests.test_kit.series import BatterySeries


class HullMovingAverageTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.hma = HullMovingAverage(10)

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual('HullMovingAverage', self.hma.name)

    def test_str_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual('HullMovingAverage(10)', str(self.hma))

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.hma).startswith('<HullMovingAverage(10) object at'))
        self.assertTrue(repr(self.hma).endswith('>'))

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.hma.update(1.00000)
        self.hma.update(2.00000)
        self.hma.update(3.00000)
        self.hma.update(4.00000)
        self.hma.update(5.00000)
        self.hma.update(6.00000)
        self.hma.update(7.00000)
        self.hma.update(8.00000)
        self.hma.update(9.00000)
        self.hma.update(10.00000)

        # Act
        # Assert
        self.assertEqual(True, self.hma.initialized)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.hma.update(1.00000)

        # Act
        # Assert
        self.assertEqual(1.0, self.hma.value)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.hma.update(1.00000)
        self.hma.update(2.00000)
        self.hma.update(3.00000)

        # Act
        # Assert
        self.assertEqual(1.8245614035087718, self.hma.value)

    def test_value_with_ten_inputs_returns_expected_value(self):
        # Arrange
        self.hma.update(1.00000)
        self.hma.update(1.00010)
        self.hma.update(1.00020)
        self.hma.update(1.00030)
        self.hma.update(1.00040)
        self.hma.update(1.00050)
        self.hma.update(1.00040)
        self.hma.update(1.00030)
        self.hma.update(1.00020)
        self.hma.update(1.00010)
        self.hma.update(1.00000)

        # Act
        # Assert
        self.assertEqual(1.0001403928170594, self.hma.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.hma.update(1.00020)
        self.hma.update(1.00030)
        self.hma.update(1.00050)

        # Act
        self.hma.reset()  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            self.hma.update(point)
            output.append(self.hma.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
