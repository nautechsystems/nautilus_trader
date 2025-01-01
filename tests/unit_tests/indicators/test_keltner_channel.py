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

from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.indicators.keltner_channel import KeltnerChannel
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestKeltnerChannel:
    def setup(self):
        # Fixture Setup
        self.kc = KeltnerChannel(10, 2.5, MovingAverageType.EXPONENTIAL, MovingAverageType.SIMPLE)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.kc.name == "KeltnerChannel"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.kc) == "KeltnerChannel(10, 2.5, EXPONENTIAL, SIMPLE, True, 0.0)"
        assert repr(self.kc) == "KeltnerChannel(10, 2.5, EXPONENTIAL, SIMPLE, True, 0.0)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.kc.period == 10

    def test_k_multiple_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.kc.k_multiplier == 2.5

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.kc.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00020, 1.00000, 1.00010)

        # Act, Assert
        assert self.kc.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = KeltnerChannel(10, 2.5, MovingAverageType.EXPONENTIAL, MovingAverageType.SIMPLE)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.middle == 1.0000266666666666

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.kc.update_raw(1.00020, 1.00000, 1.00010)

        # Act, Assert
        assert self.kc.upper == 1.0006
        assert self.kc.middle == 1.0001
        assert self.kc.lower == 0.9996

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00030, 1.00010, 1.00020)
        self.kc.update_raw(1.00040, 1.00020, 1.00030)

        # Act, Assert
        assert self.kc.upper == 1.0006512396694212
        assert self.kc.middle == 1.0001512396694212
        assert self.kc.lower == 0.9996512396694213

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.kc.update_raw(1.00020, 1.00000, 1.00010)
        self.kc.update_raw(1.00030, 1.00010, 1.00020)
        self.kc.update_raw(1.00040, 1.00020, 1.00030)

        # Act
        self.kc.reset()

        # Assert
        assert not self.kc.initialized
