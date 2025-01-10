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

from nautilus_trader.indicators.vhf import VerticalHorizontalFilter
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVerticalHorizontalFilter:
    def setup(self):
        # Fixture Setup
        self.period = 10
        self.vhf = VerticalHorizontalFilter(period=self.period)

    def test_init(self):
        assert not self.vhf.initialized
        assert not self.vhf.has_inputs
        assert self.vhf.period == self.period
        assert self.vhf.value == 0

    def test_name_returns_expected_string(self):
        assert self.vhf.name == "VerticalHorizontalFilter"

    def test_handle_bar_updates_indicator(self):
        for _ in range(self.period):
            self.vhf.handle_bar(TestDataStubs.bar_5decimal())

        assert self.vhf.has_inputs
        assert self.vhf.value == 0

    def test_value_with_one_input(self):
        self.vhf.update_raw(56.87)

        assert self.vhf.value == 0

    def test_value_with_twenty_inputs(self):
        self.vhf.update_raw(56.87)
        self.vhf.update_raw(56.96)
        self.vhf.update_raw(57.17)
        self.vhf.update_raw(57.54)
        self.vhf.update_raw(57.88)
        self.vhf.update_raw(57.85)
        self.vhf.update_raw(57.86)
        self.vhf.update_raw(57.97)
        self.vhf.update_raw(58.07)
        self.vhf.update_raw(58.04)
        self.vhf.update_raw(57.96)
        self.vhf.update_raw(57.98)
        self.vhf.update_raw(58.05)
        self.vhf.update_raw(57.94)
        self.vhf.update_raw(57.99)
        self.vhf.update_raw(58.11)
        self.vhf.update_raw(58.22)
        self.vhf.update_raw(58.19)
        self.vhf.update_raw(58.04)
        self.vhf.update_raw(58.02)

        assert self.vhf.value == 0.36842105263158487

    def test_reset(self):
        self.vhf.update_raw(56.87)

        self.vhf.reset()

        assert not self.vhf.initialized
        assert not self.vhf.has_inputs
        assert self.vhf.period == self.period
        assert self.vhf.value == 0
