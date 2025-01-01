# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytest

from nautilus_trader.indicators.kvo import KlingerVolumeOscillator
from nautilus_trader.test_kit.stubs.data import TestDataStubs


class TestKlingerVolumeOscillator:
    def setup(self):
        # Fixture Setup
        self.kvo = KlingerVolumeOscillator(5, 10, 5)

    def test_name_returns_expected_string(self):
        # Act, Assert
        assert self.kvo.name == "KlingerVolumeOscillator"

    def test_str_repr_returns_expected_string(self):
        # Act, Assert
        assert str(self.kvo) == "KlingerVolumeOscillator(5, 10, 5, EXPONENTIAL)"
        assert repr(self.kvo) == "KlingerVolumeOscillator(5, 10, 5, EXPONENTIAL)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.kvo.fast_period == 5
        assert self.kvo.slow_period == 10
        assert self.kvo.signal_period == 5

    def test_initialized_without_inputs_returns_false(self):
        # Act, Assert
        assert self.kvo.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        for i in range(15):
            self.kvo.update_raw(i, i, i, i)

        # Act, Assert
        assert self.kvo.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = KlingerVolumeOscillator(5, 10, 5)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs

    def test_values_with_one_input_returns_expected_value(self):
        # Arrange
        self.kvo.update_raw(110.08, 109.61, 109.93, 282.55)

        # Act, Assert
        assert self.kvo.value == 0.0

    def test_value_with_all_higher_inputs_returns_expected_value(self):
        # Arrange
        self.kvo.update_raw(110.08, 109.61, 109.93, 282.55)
        self.kvo.update_raw(110.15, 109.91, 110.0, 600.66)
        self.kvo.update_raw(110.1, 109.73, 109.77, 195.84)
        self.kvo.update_raw(110.06, 109.77, 109.96, 282.48)
        self.kvo.update_raw(110.29, 109.88, 110.29, 115.83)
        self.kvo.update_raw(110.53, 110.29, 110.53, 921.23)
        self.kvo.update_raw(110.61, 110.26, 110.27, 150.67)
        self.kvo.update_raw(110.28, 110.17, 110.21, 61.29)
        self.kvo.update_raw(110.3, 110.0, 110.06, 166.29)
        self.kvo.update_raw(110.25, 110.01, 110.19, 40.64)
        self.kvo.update_raw(110.25, 109.81, 109.83, 148.38)
        self.kvo.update_raw(109.92, 109.71, 109.9, 124.88)
        self.kvo.update_raw(110.21, 109.84, 110.0, 172.12)
        self.kvo.update_raw(110.08, 109.95, 110.03, 76.51)
        self.kvo.update_raw(110.2, 109.96, 110.13, 147.98)
        self.kvo.update_raw(110.16, 109.95, 109.95, 71.72)
        self.kvo.update_raw(109.99, 109.75, 109.75, 229.87)
        self.kvo.update_raw(110.2, 109.73, 110.15, 414.76)
        self.kvo.update_raw(110.1, 109.81, 109.9, 205.6)
        self.kvo.update_raw(110.04, 109.96, 110.04, 32.95)
        # Act, Assert
        assert self.kvo.value == pytest.approx(-20.530114132019506, rel=1e-9)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.kvo.update_raw(110.08, 109.61, 109.93, 282.55)

        # Act
        self.kvo.reset()  # No assertion errors

        # Assert
        assert not self.kvo.initialized
        assert self.kvo.value == 0
