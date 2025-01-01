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

from nautilus_trader.indicators.amat import ArcherMovingAveragesTrends
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestArcherMovingAveragesTrends:
    def setup(self):
        # Fixture Setup
        self.amat = ArcherMovingAveragesTrends(5, 10, 5)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.amat.name == "ArcherMovingAveragesTrends"

    def test_period(self):
        # Arrange, Act, Assert
        assert self.amat.fast_period == 5
        assert self.amat.slow_period == 10
        assert self.amat.signal_period == 5

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.amat.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange, Act
        for _i in range(20):
            self.amat.update_raw(109.61)

        # Assert
        assert self.amat.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = ArcherMovingAveragesTrends(5, 10, 5)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.long_run == 0
        assert indicator.short_run == 0

    def test_value_with_one_input(self):
        # Arrange, Act
        self.amat.update_raw(109.93)

        # Assert
        assert self.amat.long_run == 0
        assert self.amat.short_run == 0

    def test_value_with_twenty_inputs(self):
        # Arrange, Act
        self.amat.update_raw(109.93)
        self.amat.update_raw(110.0)
        self.amat.update_raw(109.77)
        self.amat.update_raw(109.96)
        self.amat.update_raw(110.29)
        self.amat.update_raw(110.53)
        self.amat.update_raw(110.27)
        self.amat.update_raw(110.21)
        self.amat.update_raw(110.06)
        self.amat.update_raw(110.19)
        self.amat.update_raw(109.83)
        self.amat.update_raw(109.9)
        self.amat.update_raw(110.0)
        self.amat.update_raw(110.03)
        self.amat.update_raw(110.13)
        self.amat.update_raw(109.95)
        self.amat.update_raw(109.75)
        self.amat.update_raw(110.15)
        self.amat.update_raw(109.9)
        self.amat.update_raw(110.04)

        # Assert
        assert self.amat.long_run == 0
        assert self.amat.short_run == 1

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.amat.update_raw(109.93)

        # Act
        self.amat.reset()

        # Assert
        assert not self.amat.initialized
        assert self.amat.long_run == 0
        assert self.amat.short_run == 0
