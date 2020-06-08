# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.indicators.efficiency_ratio import EfficiencyRatio

from tests.test_kit.series import BatterySeries


class EfficiencyRatioTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.er = EfficiencyRatio(10)

    def test_name(self):
        # Act
        # Assert
        self.assertEqual('EfficiencyRatio', self.er.name)

    def test_str(self):
        # Act
        # Assert
        self.assertEqual('EfficiencyRatio(10)', str(self.er))

    def test_repr(self):
        # Act
        # Assert
        self.assertTrue(repr(self.er).startswith('<EfficiencyRatio(10) object at'))
        self.assertTrue(repr(self.er).endswith('>'))

    def test_period(self):
        # Act
        # Assert
        self.assertEqual(10, self.er.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.er.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Act
        for i in range(10):
            self.er.update(1.00000)

        # Assert
        self.assertEqual(True, self.er.initialized)

    def test_value_with_one_input(self):
        # Arrange
        self.er.update(1.00000)

        # Act
        # Assert
        self.assertEqual(0.0, self.er.value)

    def test_value_with_efficient_higher_inputs(self):
        # Arrange
        initial_price = 1.00000

        # Act
        for i in range(10):
            initial_price += 0.00001
            self.er.update(initial_price)

        # Assert
        self.assertEqual(1.0, self.er.value)

    def test_value_with_efficient_lower_inputs(self):
        # Arrange
        initial_price = 1.00000

        # Act
        for i in range(10):
            initial_price -= 0.00001
            self.er.update(initial_price)

        # Assert
        self.assertEqual(1.0, self.er.value)

    def test_value_with_oscillating_inputs_returns_zero(self):
        # Arrange
        self.er.update(1.00000)
        self.er.update(1.00010)
        self.er.update(1.00000)
        self.er.update(0.99990)
        self.er.update(1.00000)

        # Act
        # Assert
        self.assertEqual(0.0, self.er.value)

    def test_value_with_half_oscillating_inputs_returns_zero(self):
        # Arrange
        self.er.update(1.00000)
        self.er.update(1.00020)
        self.er.update(1.00010)
        self.er.update(1.00030)
        self.er.update(1.00020)

        # Act
        # Assert
        self.assertEqual(0.3333333333333333, self.er.value)

    def test_value_with_noisy_inputs(self):
        # Arrange
        self.er.update(1.00000)
        self.er.update(1.00010)
        self.er.update(1.00008)
        self.er.update(1.00007)
        self.er.update(1.00012)
        self.er.update(1.00005)
        self.er.update(1.00015)

        # Act
        # Assert
        self.assertEqual(0.42857142857215363, self.er.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for i in range(10):
            self.er.update(1.00000)

        # Act
        self.er.reset()

        # Assert
        self.assertEqual(0, self.er.value)  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            self.er.update(point)
            output.append(self.er.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
