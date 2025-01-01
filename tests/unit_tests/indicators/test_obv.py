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

from nautilus_trader.indicators.obv import OnBalanceVolume
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestOnBalanceVolume:
    def setup(self):
        # Fixture Setup
        self.obv = OnBalanceVolume(100)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.obv.name == "OnBalanceVolume"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.obv) == "OnBalanceVolume(100)"
        assert repr(self.obv) == "OnBalanceVolume(100)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.obv.period == 100

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.obv.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        for _i in range(100):
            self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act, Assert
        assert self.obv.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = OnBalanceVolume(100)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1000000

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act, Assert
        assert self.obv.value == 10000

    def test_values_with_higher_inputs_returns_expected_value(self):
        # Arrange
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00000, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act, Assert
        assert self.obv.value == 90000.0

    def test_values_with_lower_inputs_returns_expected_value(self):
        # Arrange
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00010, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)

        # Act, Assert
        assert self.obv.value == -90000.0

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(100):
            self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act
        self.obv.reset()

        # Assert
        assert not self.obv.initialized
