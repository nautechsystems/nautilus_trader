# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.indicators.rsi import RelativeStrengthIndex
from tests.test_kit.series import BatterySeries


class RelativeStrengthIndexTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.rsi = RelativeStrengthIndex(10)

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual('RelativeStrengthIndex', self.rsi.name)

    def test_str_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual('RelativeStrengthIndex(10, EXPONENTIAL)', str(self.rsi))

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.rsi).startswith(
            '<RelativeStrengthIndex(10, EXPONENTIAL) object at'))
        self.assertTrue(repr(self.rsi).endswith('>'))

    def test_period_returns_expected_value(self):
        # Act
        # Assert
        self.assertEqual(10, self.rsi.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.rsi.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.rsi.update(1.00000)
        self.rsi.update(2.00000)
        self.rsi.update(3.00000)
        self.rsi.update(4.00000)
        self.rsi.update(5.00000)
        self.rsi.update(6.00000)
        self.rsi.update(7.00000)
        self.rsi.update(8.00000)
        self.rsi.update(9.00000)
        self.rsi.update(10.00000)

        # Act
        # Assert
        self.assertEqual(True, self.rsi.initialized)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.rsi.update(1.00000)

        # Act
        # Assert
        self.assertEqual(1, self.rsi.value)

    def test_value_with_all_higher_inputs_returns_expected_value(self):
        # Arrange
        self.rsi.update(1.00000)
        self.rsi.update(2.00000)
        self.rsi.update(3.00000)
        self.rsi.update(4.00000)

        # Act
        # Assert
        self.assertEqual(1, self.rsi.value)

    def test_value_with_all_lower_inputs_returns_expected_value(self):
        # Arrange
        self.rsi.update(3.00000)
        self.rsi.update(2.00000)
        self.rsi.update(1.00000)
        self.rsi.update(0.50000)

        # Act
        # Assert
        self.assertEqual(0, self.rsi.value)

    def test_value_with_various_inputs_returns_expected_value(self):
        # Arrange
        self.rsi.update(3.00000)
        self.rsi.update(2.00000)
        self.rsi.update(5.00000)
        self.rsi.update(6.00000)
        self.rsi.update(7.00000)
        self.rsi.update(6.00000)

        # Act
        # Assert
        self.assertEqual(0.6837363325825265, self.rsi.value)

    def test_value_at_returns_expected_value(self):
        # Arrange
        self.rsi.update(3.00000)
        self.rsi.update(2.00000)
        self.rsi.update(5.00000)
        self.rsi.update(6.00000)
        self.rsi.update(7.00000)
        self.rsi.update(6.00000)
        self.rsi.update(6.00000)
        self.rsi.update(7.00000)

        # Act
        # Assert
        self.assertEqual(0.7615344667662725, self.rsi.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.rsi.update(1.00020)
        self.rsi.update(1.00030)
        self.rsi.update(1.00050)

        # Act
        self.rsi.reset()  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            self.rsi.update(point)
            output.append(self.rsi.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
