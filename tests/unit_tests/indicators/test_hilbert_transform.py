# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import sys

from nautilus_trader.indicators.hilbert_transform import HilbertTransform
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestHilbertTransform:
    def setup(self):
        # Fixture Setup
        self.ht = HilbertTransform()

    def test_name_returns_expected_name(self):
        # Arrange
        # Act
        # Assert
        assert self.ht.name == "HilbertTransform"

    def test_str_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        assert str(self.ht) == "HilbertTransform(7)"
        assert repr(self.ht) == "HilbertTransform(7)"

    def test_period_returns_expected_value(self):
        # Arrange
        # Act
        # Assert
        assert self.ht.period == 7

    def test_initialized_without_inputs_returns_false(self):
        # Arrange
        # Act
        # Assert
        assert self.ht.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Act
        for _i in range(10):
            self.ht.update_raw(1.00000)

        # Assert
        assert self.ht.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = HilbertTransform()

        bar = TestStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value_quad == 0

    def test_value_with_no_inputs_returns_none(self):
        # Arrange
        # Act
        # Assert
        assert self.ht.value_in_phase == 0.0
        assert self.ht.value_quad == 0.0

    def test_value_with_epsilon_inputs_returns_expected_value(self):
        # Arrange
        for _i in range(100):
            self.ht.update_raw(sys.float_info.epsilon)

        # Act
        # Assert
        assert self.ht.value_in_phase == 0.0
        assert self.ht.value_quad == 0.0

    def test_value_with_ones_inputs_returns_expected_value(self):
        # Arrange
        for _i in range(100):
            self.ht.update_raw(1.00000)

        # Act
        # Assert
        assert self.ht.value_in_phase == 0.0
        assert self.ht.value_quad == 0.0

    def test_value_with_seven_inputs_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(9):
            high += 0.00010
            low += 0.00010
            self.ht.update_raw((high + low) / 2)

        # Assert
        assert self.ht.value_in_phase == 0.0
        assert self.ht.value_quad == 0.0

    def test_value_with_close_on_high_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(1000):
            high += 0.00010
            low += 0.00010
            self.ht.update_raw((high + low) / 2)

        # Assert
        assert self.ht.value_in_phase == 0.001327272727272581
        assert self.ht.value_quad == 0.0005999999999999338

    def test_value_with_close_on_low_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(1000):
            high -= 0.00010
            low -= 0.00010
            self.ht.update_raw((high + low) / 2)

        # Assert
        assert self.ht.value_in_phase == -0.001327272727272581
        assert self.ht.value_quad == -0.0005999999999999338

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.ht.update_raw(1.00000)

        # Act
        self.ht.reset()

        # Assert
        assert self.ht.value_in_phase == 0.0  # No assertion errors.
        assert self.ht.value_quad == 0.0
