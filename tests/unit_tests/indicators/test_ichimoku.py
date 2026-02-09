# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.indicators import IchimokuCloud
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestIchimokuCloud:
    def setup(self):
        self.ich = IchimokuCloud(9, 26, 52, 26)

    def test_name_returns_expected_string(self):
        assert self.ich.name == "IchimokuCloud"

    def test_str_repr_returns_expected_string(self):
        assert str(self.ich) == "IchimokuCloud(9, 26, 52, 26)"
        assert repr(self.ich) == "IchimokuCloud(9, 26, 52, 26)"

    def test_parameters_after_instantiation(self):
        assert self.ich.tenkan_period == 9
        assert self.ich.kijun_period == 26
        assert self.ich.senkou_period == 52
        assert self.ich.displacement == 26
        assert self.ich.tenkan_sen == 0.0
        assert self.ich.kijun_sen == 0.0
        assert self.ich.senkou_span_a == 0.0
        assert self.ich.senkou_span_b == 0.0
        assert self.ich.chikou_span == 0.0

    def test_initialized_without_inputs_returns_false(self):
        assert self.ich.initialized is False

    def test_has_inputs_after_one_bar_returns_true(self):
        self.ich.update_raw(11.0, 9.0, 10.0)
        assert self.ich.has_inputs is True
        assert self.ich.tenkan_sen == 0.0
        assert self.ich.kijun_sen == 0.0

    def test_value_with_one_input_returns_expected_value(self):
        self.ich.update_raw(11.0, 9.0, 10.0)
        assert self.ich.tenkan_sen == 0.0
        assert self.ich.kijun_sen == 0.0
        assert self.ich.senkou_span_a == 0.0
        assert self.ich.senkou_span_b == 0.0
        assert self.ich.chikou_span == 0.0

    def test_initialized_after_required_inputs_returns_true(self):
        for _ in range(52):
            self.ich.update_raw(10.0, 8.0, 9.0)
        assert self.ich.initialized is True

    def test_tenkan_sen_after_nine_bars_returns_midpoint(self):
        for _ in range(9):
            self.ich.update_raw(12.0, 8.0, 10.0)
        assert self.ich.tenkan_sen == 10.0

    def test_kijun_sen_after_twenty_six_bars_returns_midpoint(self):
        for _ in range(26):
            self.ich.update_raw(12.0, 8.0, 10.0)
        assert self.ich.kijun_sen == 10.0

    def test_senkou_and_chikou_after_displacement_bars_returns_expected(self):
        for _ in range(52 + 26):
            self.ich.update_raw(12.0, 8.0, 10.0)
        assert self.ich.senkou_span_a == 10.0
        assert self.ich.senkou_span_b == 10.0
        assert self.ich.chikou_span == 10.0

    def test_handle_bar_updates_indicator(self):
        indicator = IchimokuCloud(9, 26, 52, 26)
        bar = TestDataStubs.bar_5decimal()
        indicator.handle_bar(bar)
        assert indicator.has_inputs

    def test_handle_bar_nine_times_returns_expected_tenkan_sen(self):
        indicator = IchimokuCloud(9, 26, 52, 26)
        bar = TestDataStubs.bar_5decimal()
        for _ in range(9):
            indicator.handle_bar(bar)
        assert indicator.tenkan_sen == 1.000025

    def test_reset_returns_to_fresh_state(self):
        for _ in range(20):
            self.ich.update_raw(10.0, 8.0, 9.0)
        self.ich.reset()
        assert self.ich.initialized is False
        assert self.ich.tenkan_sen == 0.0
        assert self.ich.kijun_sen == 0.0
        assert self.ich.senkou_span_a == 0.0
        assert self.ich.senkou_span_b == 0.0
        assert self.ich.chikou_span == 0.0

    def test_custom_periods_initialization(self):
        ich = IchimokuCloud(tenkan_period=5, kijun_period=10, senkou_period=20, displacement=10)
        assert ich.tenkan_period == 5
        assert ich.kijun_period == 10
        assert ich.senkou_period == 20
        assert ich.displacement == 10
        for _ in range(20):
            ich.update_raw(1.0, 1.0, 1.0)
        assert ich.initialized is True
        assert ich.tenkan_sen == 1.0
        assert ich.kijun_sen == 1.0

    def test_tenkan_sen_updates_with_varying_data(self):
        ich = IchimokuCloud(tenkan_period=3, kijun_period=3, senkou_period=3, displacement=2)

        # Fill the window: highs=[10, 12, 14], lows=[5, 6, 7]
        ich.update_raw(10.0, 5.0, 8.0)
        ich.update_raw(12.0, 6.0, 9.0)
        ich.update_raw(14.0, 7.0, 10.0)
        assert ich.tenkan_sen == (14.0 + 5.0) / 2.0  # 9.5

        # Push a new bar that evicts the (10, 5) pair: highs=[12, 14, 8], lows=[6, 7, 3]
        ich.update_raw(8.0, 3.0, 6.0)
        assert ich.tenkan_sen == (14.0 + 3.0) / 2.0  # 8.5

        # Push another bar that evicts the (12, 6) pair: highs=[14, 8, 20], lows=[7, 3, 4]
        ich.update_raw(20.0, 4.0, 12.0)
        assert ich.tenkan_sen == (20.0 + 3.0) / 2.0  # 11.5

    def test_invalid_periods_raises(self):
        with pytest.raises(ValueError):
            IchimokuCloud(9, 5, 52, 26)
        with pytest.raises(ValueError):
            IchimokuCloud(9, 26, 20, 26)
        with pytest.raises(ValueError):
            IchimokuCloud(9, 26, 52, 0)
