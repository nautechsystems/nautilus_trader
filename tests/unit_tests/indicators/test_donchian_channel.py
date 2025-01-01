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

from nautilus_trader.indicators.donchian_channel import DonchianChannel
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestDonchianChannel:
    def setup(self):
        # Fixture Setup
        self.dc = DonchianChannel(10)

    def test_name_returns_expected_name(self):
        # Arrange, Act, Assert
        assert self.dc.name == "DonchianChannel"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.dc) == "DonchianChannel(10)"
        assert repr(self.dc) == "DonchianChannel(10)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.dc.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.dc.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)

        # Act, Assert
        assert self.dc.initialized is True

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = DonchianChannel(10)

        tick = TestDataStubs.quote_tick()

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.middle == 1.0

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = DonchianChannel(10)

        tick = TestDataStubs.trade_tick()

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.middle == 1.0

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = DonchianChannel(10)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.middle == 1.000025

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.dc.update_raw(1.00020, 1.00000)

        # Act, Assert
        assert self.dc.upper == 1.00020
        assert self.dc.middle == 1.00010
        assert self.dc.lower == 1.00000

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.dc.update_raw(1.00020, 1.00000)
        self.dc.update_raw(1.00030, 1.00010)
        self.dc.update_raw(1.00040, 1.00020)

        # Act, Assert
        assert self.dc.upper == 1.00040
        assert self.dc.middle == 1.00020
        assert self.dc.lower == 1.00000

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.dc.update_raw(1.00020, 1.00000)
        self.dc.update_raw(1.00030, 1.00010)
        self.dc.update_raw(1.00040, 1.00020)

        # Act
        self.dc.reset()

        # Assert
        assert not self.dc.initialized
        assert self.dc.upper == 0
        assert self.dc.middle == 0
        assert self.dc.lower == 0
