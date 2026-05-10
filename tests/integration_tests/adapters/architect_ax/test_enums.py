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

from nautilus_trader.core.nautilus_pyo3 import AxEnvironment
from nautilus_trader.core.nautilus_pyo3 import AxMarketDataLevel


class TestAxEnvironment:
    @pytest.mark.parametrize(
        ("variant", "expected_name"),
        [
            (AxEnvironment.SANDBOX, "SANDBOX"),
            (AxEnvironment.PRODUCTION, "PRODUCTION"),
        ],
    )
    def test_variant_name(self, variant, expected_name):
        assert variant.name == expected_name

    @pytest.mark.parametrize(
        ("variant", "expected_value"),
        [
            (AxEnvironment.SANDBOX, 0),
            (AxEnvironment.PRODUCTION, 1),
        ],
    )
    def test_variant_value(self, variant, expected_value):
        assert variant.value == expected_value

    @pytest.mark.parametrize(
        ("input_str", "expected"),
        [
            ("SANDBOX", AxEnvironment.SANDBOX),
            ("sandbox", AxEnvironment.SANDBOX),
            ("PRODUCTION", AxEnvironment.PRODUCTION),
            ("production", AxEnvironment.PRODUCTION),
        ],
    )
    def test_from_str(self, input_str, expected):
        assert AxEnvironment.from_str(input_str) == expected

    def test_str(self):
        assert str(AxEnvironment.SANDBOX) == "SANDBOX"
        assert str(AxEnvironment.PRODUCTION) == "PRODUCTION"

    def test_hashable(self):
        env_set = {AxEnvironment.SANDBOX, AxEnvironment.PRODUCTION}
        assert len(env_set) == 2
        assert AxEnvironment.SANDBOX in env_set


class TestAxMarketDataLevel:
    @pytest.mark.parametrize(
        ("variant", "expected_name"),
        [
            (AxMarketDataLevel.LEVEL1, "LEVEL_1"),
            (AxMarketDataLevel.LEVEL2, "LEVEL_2"),
            (AxMarketDataLevel.LEVEL3, "LEVEL_3"),
        ],
    )
    def test_variant_name(self, variant, expected_name):
        assert variant.name == expected_name

    @pytest.mark.parametrize(
        ("variant", "expected_value"),
        [
            (AxMarketDataLevel.LEVEL1, 0),
            (AxMarketDataLevel.LEVEL2, 1),
            (AxMarketDataLevel.LEVEL3, 2),
        ],
    )
    def test_variant_value(self, variant, expected_value):
        assert variant.value == expected_value

    @pytest.mark.parametrize(
        ("input_str", "expected"),
        [
            ("LEVEL_1", AxMarketDataLevel.LEVEL1),
            ("level_1", AxMarketDataLevel.LEVEL1),
            ("LEVEL_2", AxMarketDataLevel.LEVEL2),
            ("LEVEL_3", AxMarketDataLevel.LEVEL3),
        ],
    )
    def test_from_str(self, input_str, expected):
        assert AxMarketDataLevel.from_str(input_str) == expected

    def test_str(self):
        assert str(AxMarketDataLevel.LEVEL1) == "LEVEL_1"
        assert str(AxMarketDataLevel.LEVEL2) == "LEVEL_2"
        assert str(AxMarketDataLevel.LEVEL3) == "LEVEL_3"

    def test_hashable(self):
        level_set = {AxMarketDataLevel.LEVEL1, AxMarketDataLevel.LEVEL2}
        assert len(level_set) == 2
        assert AxMarketDataLevel.LEVEL1 in level_set
