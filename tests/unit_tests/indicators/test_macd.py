# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.indicators.macd import MovingAverageConvergenceDivergence

from tests.test_kit.series import BatterySeries


class MovingAverageConvergenceDivergenceTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.macd = MovingAverageConvergenceDivergence(3, 10)

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual('MovingAverageConvergenceDivergence', self.macd.name)

    def test_str_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual('MovingAverageConvergenceDivergence(3, 10, EXPONENTIAL)', str(self.macd))

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.macd).startswith(
            '<MovingAverageConvergenceDivergence(3, 10, EXPONENTIAL) object at'))
        self.assertTrue(repr(self.macd).endswith('>'))

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.macd.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.macd.update(1.00000)
        self.macd.update(2.00000)
        self.macd.update(3.00000)
        self.macd.update(4.00000)
        self.macd.update(5.00000)
        self.macd.update(6.00000)
        self.macd.update(7.00000)
        self.macd.update(8.00000)
        self.macd.update(9.00000)
        self.macd.update(10.00000)
        self.macd.update(11.00000)
        self.macd.update(12.00000)
        self.macd.update(13.00000)
        self.macd.update(14.00000)
        self.macd.update(15.00000)
        self.macd.update(16.00000)

        # Act
        # Assert
        self.assertEqual(True, self.macd.initialized)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.macd.update(1.00000)

        # Act
        # Assert
        self.assertEqual(0, self.macd.value)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.macd.update(1.00000)
        self.macd.update(2.00000)
        self.macd.update(3.00000)

        # Act
        # Assert
        self.assertEqual(0.7376033057851243, self.macd.value)

    def test_value_with_more_inputs_expected_value(self):
        # Arrange
        self.macd.update(1.00000)
        self.macd.update(2.00000)
        self.macd.update(3.00000)
        self.macd.update(4.00000)
        self.macd.update(5.00000)
        self.macd.update(6.00000)
        self.macd.update(7.00000)
        self.macd.update(8.00000)
        self.macd.update(9.00000)
        self.macd.update(10.00000)
        self.macd.update(11.00000)
        self.macd.update(12.00000)
        self.macd.update(13.00000)
        self.macd.update(14.00000)
        self.macd.update(15.00000)
        self.macd.update(16.00000)

        # Act
        # Assert
        self.assertEqual(3.2782313673122907, self.macd.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.macd.update(1.00020)
        self.macd.update(1.00030)
        self.macd.update(1.00050)

        # Act
        self.macd.reset()  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            self.macd.update(point)
            output.append(self.macd.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
