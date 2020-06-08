# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.indicators.roc import RateOfChange
from tests.test_kit.series import BatterySeries


class RateOfChangeTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.roc = RateOfChange(3)

    def test_name(self):
        # Act
        # Assert
        self.assertEqual('RateOfChange', self.roc.name)

    def test_str(self):
        # Act
        # Assert
        self.assertEqual('RateOfChange(3)', str(self.roc))

    def test_repr(self):
        # Act
        # Assert
        self.assertTrue(repr(self.roc).startswith('<RateOfChange(3) object at'))
        self.assertTrue(repr(self.roc).endswith('>'))

    def test_period(self):
        # Act
        # Assert
        self.assertEqual(3, self.roc.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.roc.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Act
        for i in range(3):
            self.roc.update(1.00000)

        # Assert
        self.assertEqual(True, self.roc.initialized)

    def test_value_with_one_input(self):
        # Arrange
        self.roc.update(1.00000)

        # Act
        # Assert
        self.assertEqual(0., self.roc.value)

    def test_value_with_efficient_higher_inputs(self):
        # Arrange
        price = 1.00000

        # Act
        for i in range(10):
            price += 0.10000
            self.roc.update(price)

        # Assert
        self.assertEqual(0.11111111111111116, self.roc.value)

    def test_value_with_oscillating_inputs_returns_zero(self):
        # Arrange
        self.roc.update(1.00000)
        self.roc.update(1.00010)
        self.roc.update(1.00000)
        self.roc.update(0.99990)
        self.roc.update(1.00000)

        # Act
        # Assert
        self.assertEqual(0., self.roc.value)

    def test_value_with_half_oscillating_inputs_returns_zero(self):
        # Arrange
        self.roc.update(1.00000)
        self.roc.update(1.00020)
        self.roc.update(1.00010)
        self.roc.update(1.00030)
        self.roc.update(1.00020)

        # Act
        # Assert
        self.assertEqual(9.9990000999889e-05, self.roc.value)

    def test_value_with_noisy_inputs(self):
        # Arrange
        self.roc.update(1.00000)
        self.roc.update(1.00010)
        self.roc.update(1.00008)
        self.roc.update(1.00007)
        self.roc.update(1.00012)
        self.roc.update(1.00005)
        self.roc.update(1.00015)

        # Act
        # Assert
        self.assertEqual(2.9996400432144683e-05, self.roc.value)

    def test_log_returns_value_with_noisy_inputs(self):
        # Arrange
        roc = RateOfChange(3, use_log=True)

        roc.update(1.00000)
        roc.update(1.00010)
        roc.update(1.00008)
        roc.update(1.00007)
        roc.update(1.00012)
        roc.update(1.00005)
        roc.update(1.00015)

        # Act
        # Assert
        self.assertEqual(2.999595054919663e-05, roc.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for i in range(10):
            self.roc.update(1.00000)

        # Act
        self.roc.reset()

        # Assert
        self.assertEqual(0, self.roc.value)  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            self.roc.update(point)
            output.append(self.roc.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
        print(self.roc.value)
