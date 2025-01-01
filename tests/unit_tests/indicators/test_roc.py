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

from nautilus_trader.indicators.roc import RateOfChange
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestRateOfChange:
    def setup(self):
        # Fixture Setup
        self.roc = RateOfChange(3)

    def test_name_returns_expected_string(self):
        # Act, Assert
        assert self.roc.name == "RateOfChange"

    def test_str_repr_returns_expected_string(self):
        # Act, Assert
        assert str(self.roc) == "RateOfChange(3)"
        assert repr(self.roc) == "RateOfChange(3)"

    def test_period(self):
        # Act, Assert
        assert self.roc.period == 3

    def test_initialized_without_inputs_returns_false(self):
        # Act, Assert
        assert self.roc.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange, Act
        for _i in range(3):
            self.roc.update_raw(1.00000)

        # Assert
        assert self.roc.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = RateOfChange(3)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 0

    def test_value_with_one_input(self):
        # Arrange
        self.roc.update_raw(1.00000)

        # Act, Assert
        assert self.roc.value == 0

    def test_value_with_efficient_higher_inputs(self):
        # Arrange
        price = 1.00000

        # Act
        for _i in range(10):
            price += 0.10000
            self.roc.update_raw(price)

        # Assert
        assert self.roc.value == 0.11111111111111116

    def test_value_with_oscillating_inputs_returns_zero(self):
        # Arrange
        self.roc.update_raw(1.00000)
        self.roc.update_raw(1.00010)
        self.roc.update_raw(1.00000)
        self.roc.update_raw(0.99990)
        self.roc.update_raw(1.00000)

        # Act, Assert
        assert self.roc.value == 0.0

    def test_value_with_half_oscillating_inputs_returns_zero(self):
        # Arrange
        self.roc.update_raw(1.00000)
        self.roc.update_raw(1.00020)
        self.roc.update_raw(1.00010)
        self.roc.update_raw(1.00030)
        self.roc.update_raw(1.00020)

        # Act, Assert
        assert self.roc.value == 9.9990000999889e-05

    def test_value_with_noisy_inputs(self):
        # Arrange
        self.roc.update_raw(1.00000)
        self.roc.update_raw(1.00010)
        self.roc.update_raw(1.00008)
        self.roc.update_raw(1.00007)
        self.roc.update_raw(1.00012)
        self.roc.update_raw(1.00005)
        self.roc.update_raw(1.00015)

        # Act, Assert
        assert self.roc.value == 2.9996400432144683e-05

    def test_log_returns_value_with_noisy_inputs(self):
        # Arrange
        roc = RateOfChange(3, use_log=True)

        roc.update_raw(1.00000)
        roc.update_raw(1.00010)
        roc.update_raw(1.00008)
        roc.update_raw(1.00007)
        roc.update_raw(1.00012)
        roc.update_raw(1.00005)
        roc.update_raw(1.00015)

        # Act, Assert
        assert roc.value == 2.999595054919663e-05

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(10):
            self.roc.update_raw(1.00000)

        # Act
        self.roc.reset()

        # Assert
        assert not self.roc.initialized
        assert self.roc.value == 0
