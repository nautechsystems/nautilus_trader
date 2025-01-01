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

from datetime import timedelta

import pytest

from nautilus_trader.indicators.vwap import VolumeWeightedAveragePrice
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVolumeWeightedAveragePrice:
    def setup(self):
        # Fixture Setup
        self.vwap = VolumeWeightedAveragePrice()

    def test_name_returns_expected_string(self):
        # Act, Assert
        assert self.vwap.name == "VolumeWeightedAveragePrice"

    def test_str_repr_returns_expected_string(self):
        # Act, Assert
        assert str(self.vwap) == "VolumeWeightedAveragePrice()"
        assert repr(self.vwap) == "VolumeWeightedAveragePrice()"

    def test_initialized_without_inputs_returns_false(self):
        # Act, Assert
        assert self.vwap.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange, Act
        self.vwap.update_raw(1.00000, 10000, UNIX_EPOCH)

        # Assert
        assert self.vwap.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = VolumeWeightedAveragePrice()

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0000266666666666

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.vwap.update_raw(1.00000, 10000, UNIX_EPOCH)

        # Act, Assert
        assert self.vwap.value == 1.00000

    def test_values_with_higher_inputs_returns_expected_value(self):
        # Arrange, Act
        self.vwap.update_raw(1.00000, 10000, UNIX_EPOCH)
        self.vwap.update_raw(1.00010, 11000, UNIX_EPOCH)
        self.vwap.update_raw(1.00020, 12000, UNIX_EPOCH)
        self.vwap.update_raw(1.00030, 13000, UNIX_EPOCH)
        self.vwap.update_raw(1.00040, 14000, UNIX_EPOCH)
        self.vwap.update_raw(1.00050, 0, UNIX_EPOCH)
        self.vwap.update_raw(1.00060, 16000, UNIX_EPOCH)
        self.vwap.update_raw(1.00070, 17000, UNIX_EPOCH)
        self.vwap.update_raw(1.00080, 18000, UNIX_EPOCH)
        self.vwap.update_raw(1.00090, 19000, UNIX_EPOCH)

        # Assert
        assert self.vwap.value == pytest.approx(1.0005076923076923, rel=1e-9)

    def test_values_with_all_lower_inputs_returns_expected_value(self):
        # Arrange, Act
        self.vwap.update_raw(1.00100, 20000, UNIX_EPOCH)
        self.vwap.update_raw(1.00090, 19000, UNIX_EPOCH)
        self.vwap.update_raw(1.00080, 18000, UNIX_EPOCH)
        self.vwap.update_raw(1.00070, 17000, UNIX_EPOCH)
        self.vwap.update_raw(1.00060, 16000, UNIX_EPOCH)
        self.vwap.update_raw(1.00050, 15000, UNIX_EPOCH)
        self.vwap.update_raw(1.00040, 14000, UNIX_EPOCH)
        self.vwap.update_raw(1.00030, 13000, UNIX_EPOCH)
        self.vwap.update_raw(1.00020, 12000, UNIX_EPOCH)
        self.vwap.update_raw(1.00010, 11000, UNIX_EPOCH)

        # Assert
        assert self.vwap.value == pytest.approx(1.0006032258064514, rel=1e-9)

    def test_new_day_resets_values(self):
        # Arrange, Act
        self.vwap.update_raw(1.00000, 10000, UNIX_EPOCH)
        self.vwap.update_raw(1.00010, 11000, UNIX_EPOCH)
        self.vwap.update_raw(1.00020, 12000, UNIX_EPOCH)
        self.vwap.update_raw(1.00030, 13000, UNIX_EPOCH)
        self.vwap.update_raw(1.00040, 14000, UNIX_EPOCH)
        self.vwap.update_raw(1.00050, 0, UNIX_EPOCH)
        self.vwap.update_raw(1.00060, 16000, UNIX_EPOCH)
        self.vwap.update_raw(1.00070, 17000, UNIX_EPOCH)
        self.vwap.update_raw(1.00080, 18000, UNIX_EPOCH)
        self.vwap.update_raw(1.00090, 19000, UNIX_EPOCH)
        self.vwap.update_raw(1.00000, 10000, UNIX_EPOCH + timedelta(1))

        # Assert
        assert self.vwap.value == 1.00000

    def test_new_day_with_first_volume_zero_returns_price_as_value(self):
        # Arrange, Act
        self.vwap.update_raw(2.00000, 10000, UNIX_EPOCH)
        self.vwap.update_raw(1.00000, 0, UNIX_EPOCH + timedelta(1))

        # Assert
        assert self.vwap.value == 1.00000

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(100):
            self.vwap.update_raw(1.00000, 10000, UNIX_EPOCH)

        # Act
        self.vwap.reset()

        # Assert
        assert not self.vwap.initialized
        assert self.vwap.value == 0
