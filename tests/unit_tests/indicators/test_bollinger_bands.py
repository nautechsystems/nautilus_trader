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

from nautilus_trader.indicators import BollingerBands
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestBollingerBands:
    def test_name_returns_expected_name(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act, Assert
        assert indicator.name == "BollingerBands"

    def test_str_repr_returns_expected_string(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act, Assert
        assert str(indicator) == "BollingerBands(20, 2.0, SIMPLE)"
        assert repr(indicator) == "BollingerBands(20, 2.0, SIMPLE)"

    def test_properties_after_instantiation(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act, Assert
        assert indicator.period == 20
        assert indicator.k == 2.0
        assert indicator.upper == 0
        assert indicator.lower == 0
        assert indicator.middle == 0

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        indicator = BollingerBands(5, 2.0)

        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)

        # Act, Assert
        assert indicator.initialized is True

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        tick = TestDataStubs.quote_tick()

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.middle == 1.0

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        tick = TestDataStubs.trade_tick()

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.middle == 1.0

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.middle == 1.0000266666666666

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act
        indicator.update_raw(1.00020, 1.00000, 1.00010)

        # Assert
        assert indicator.upper == 1.00010
        assert indicator.middle == 1.00010
        assert indicator.lower == 1.00010

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act
        indicator.update_raw(1.00020, 1.00000, 1.00015)
        indicator.update_raw(1.00030, 1.00010, 1.00015)
        indicator.update_raw(1.00040, 1.00020, 1.00021)

        # Assert
        assert indicator.upper == 1.0003155506390384
        assert indicator.middle == 1.0001900000000001
        assert indicator.lower == 1.0000644493609618

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        indicator = BollingerBands(5, 2.0)

        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)

        # Act
        indicator.reset()

        # Assert
        assert not indicator.initialized
        assert indicator.upper == 0
        assert indicator.middle == 0
        assert indicator.lower == 0
