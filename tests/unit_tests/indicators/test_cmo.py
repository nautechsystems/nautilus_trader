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

from nautilus_trader.indicators.cmo import ChandeMomentumOscillator
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestChandeMomentumOscillator:
    def setup(self):
        # Fixture Setup
        self.cmo = ChandeMomentumOscillator(10)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.cmo.name == "ChandeMomentumOscillator"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.cmo) == "ChandeMomentumOscillator(10, WILDER)"
        assert repr(self.cmo) == "ChandeMomentumOscillator(10, WILDER)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.cmo.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.cmo.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.cmo.update_raw(1.00000)
        self.cmo.update_raw(2.00000)
        self.cmo.update_raw(3.00000)
        self.cmo.update_raw(4.00000)
        self.cmo.update_raw(5.00000)
        self.cmo.update_raw(6.00000)
        self.cmo.update_raw(7.00000)
        self.cmo.update_raw(8.00000)
        self.cmo.update_raw(9.00000)
        self.cmo.update_raw(10.00000)

        # Act, Assert
        assert self.cmo.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = ChandeMomentumOscillator(10)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)
        # Assert
        assert indicator.has_inputs
        assert indicator.value == 0

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.cmo.update_raw(1.00000)
        # Act, Assert
        assert self.cmo.value == 0

    def test_value_with_all_higher_inputs_returns_expected_value(self):
        # Arrange
        self.cmo.update_raw(109.93)
        self.cmo.update_raw(110.0)
        self.cmo.update_raw(109.77)
        self.cmo.update_raw(109.96)
        self.cmo.update_raw(110.29)
        self.cmo.update_raw(110.53)
        self.cmo.update_raw(110.27)
        self.cmo.update_raw(110.21)
        self.cmo.update_raw(110.06)
        self.cmo.update_raw(110.19)
        self.cmo.update_raw(109.83)
        self.cmo.update_raw(109.9)
        self.cmo.update_raw(110.0)
        self.cmo.update_raw(110.03)
        self.cmo.update_raw(110.13)
        self.cmo.update_raw(109.95)
        self.cmo.update_raw(109.75)
        self.cmo.update_raw(110.15)
        self.cmo.update_raw(109.9)
        self.cmo.update_raw(110.04)
        # Act, Assert
        assert self.cmo.value == 2.0896294562387054

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.cmo.update_raw(1.00020)
        self.cmo.update_raw(1.00030)
        self.cmo.update_raw(1.00050)

        # Act
        self.cmo.reset()

        # Assert
        assert not self.cmo.initialized
        assert self.cmo.value == 0
