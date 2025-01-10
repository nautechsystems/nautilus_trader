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

import numpy as np

from nautilus_trader.indicators.cci import CommodityChannelIndex
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestCommodityChannelIndex:
    def setup(self):
        # Fixture Setup
        self.period = 10
        self.cci = CommodityChannelIndex(period=self.period)

    def test_init(self):
        assert not self.cci.initialized
        assert not self.cci.has_inputs
        assert self.cci.period == self.period
        assert self.cci.scalar == 0.015
        assert self.cci._mad == 0
        assert self.cci.value == 0

    def test_name_returns_expected_string(self):
        assert self.cci.name == "CommodityChannelIndex"

    def test_handle_bar_updates_indicator(self):
        for _ in range(self.period):
            self.cci.handle_bar(TestDataStubs.bar_5decimal())

        assert self.cci.has_inputs
        assert self.cci.scalar == 0.015
        assert self.cci._mad == 0
        assert np.isnan(self.cci.value)

    def test_value_with_one_input(self):
        self.cci.update_raw(0.18000, 0.01001, 0.13810)

        assert self.cci.scalar == 0.015
        assert self.cci._mad == 0
        assert self.cci.value == 0

    def test_value_with_ten_inputs(self):
        self.cci.update_raw(0.18000, 0.01001, 0.13810)
        self.cci.update_raw(0.14499, 0.136, 0.14131)
        self.cci.update_raw(0.155, 0.13945, 0.15)
        self.cci.update_raw(0.17, 0.1468, 0.15829)
        self.cci.update_raw(0.172, 0.15712, 0.15938)
        self.cci.update_raw(0.15937, 0.14352, 0.14564)
        self.cci.update_raw(0.15171, 0.14571, 0.148)
        self.cci.update_raw(0.15699, 0.148, 0.15456)
        self.cci.update_raw(0.15547, 0.14894, 0.15029)
        self.cci.update_raw(0.15199, 0.14908, 0.15181)

        assert self.cci.scalar == 0.015
        assert self.cci._mad == 0.008899733333333352
        assert self.cci.value == 27.284213259823147

    def test_reset(self):
        self.cci.update_raw(0.18000, 0.01001, 0.13810)

        self.cci.reset()

        assert not self.cci.initialized
        assert not self.cci.has_inputs
        assert self.cci.period == self.period
        assert self.cci.scalar == 0.015
        assert self.cci._mad == 0
        assert self.cci.value == 0
