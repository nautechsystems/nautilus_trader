# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.indicators.pattern import Pattern
from tests.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestPattern:
    def setup(self):
        # Fixture Setup
        self.pattern = Pattern(10)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.pattern.name == "Pattern"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.pattern.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.pattern.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.pattern.update_raw(0.15359, 0.1551, 0.15248, 0.1546)
        self.pattern.update_raw(0.15492, 0.15497, 0.15099, 0.15283)
        self.pattern.update_raw(0.15283, 0.15381, 0.152, 0.15365)
        self.pattern.update_raw(0.15283, 0.15381, 0.152, 0.15365)
        self.pattern.update_raw(0.15458, 0.15683, 0.15456, 0.15683)
        self.pattern.update_raw(0.15686, 0.157, 0.15372, 0.15385)
        self.pattern.update_raw(0.154, 0.15493, 0.15181, 0.15268)
        self.pattern.update_raw(0.15228, 0.15242, 0.151, 0.15183)
        self.pattern.update_raw(0.1522, 0.1534, 0.15195, 0.15238)
        self.pattern.update_raw(0.15238, 0.154, 0.15162, 0.15391)
        self.pattern.update_raw(0.15375, 0.15452, 0.15302, 0.15339)

        # Act, Assert
        assert self.pattern.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = Pattern(30)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == [0, 0, 0, 0, 0, 0, 0]

    def test_value_with_one_input(self):
        self.pattern.update_raw(0.15329, 0.154, 0.15, 0.15)

        assert self.pattern.value == [0, 0, 0, 0, 0, 0, 0]

    def test_value_with_ten_inputs(self):
        self.pattern.update_raw(0.15329, 0.154, 0.15, 0.15)
        self.pattern.update_raw(0.15001, 0.15083, 0.14915, 0.1497)
        self.pattern.update_raw(0.1495, 0.14961, 0.14701, 0.14847)
        self.pattern.update_raw(0.15001, 0.15083, 0.14915, 0.1497)
        self.pattern.update_raw(0.14846, 0.1488, 0.14722, 0.1474)
        self.pattern.update_raw(0.14721, 0.149, 0.14717, 0.14796)
        self.pattern.update_raw(0.1482, 0.15108, 0.14817, 0.15001)
        self.pattern.update_raw(0.15015, 0.1504, 0.14967, 0.14967)
        self.pattern.update_raw(0.14936, 0.15, 0.14828, 0.149)
        self.pattern.update_raw(0.14908, 0.1498, 0.14867, 0.14971)

        assert self.pattern.value == [0, 0, 0, 0, 0, 0, -100]

    def test_reset(self):
        # Arrange
        self.pattern.update_raw(00.15015, 0.1504, 0.14967, 0.14967)
        self.pattern.update_raw(0.14936, 0.15, 0.14828, 0.149)
        self.pattern.update_raw(0.14908, 0.1498, 0.14867, 0.14971)

        # Act
        self.pattern.reset()

        # Assert
        assert not self.pattern.initialized
