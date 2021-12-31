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

import pytest

from nautilus_trader.msgbus.wildcard import is_matching


@pytest.mark.parametrize(
    "topic, pattern, expected",
    [
        ["*", "*", True],
        ["a", "*", True],
        ["a", "a", True],
        ["a", "b", False],
        ["data.quotes.BINANCE", "data.*", True],
        ["data.quotes.BINANCE", "data.quotes*", True],
        ["data.quotes.BINANCE", "data.*.BINANCE", True],
        ["data.trades.BINANCE.ETH/USDT", "data.*.BINANCE.*", True],
        ["data.trades.BINANCE.ETH/USDT", "data.*.BINANCE.ETH*", True],
    ],
)
def test_is_matching_given_various_topic_pattern_combos(topic, pattern, expected):
    # Arrange, Act, Assert
    assert is_matching(topic=topic, pattern=pattern) == expected
