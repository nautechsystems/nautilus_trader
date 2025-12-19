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

import sys

import pytest


pytestmark = pytest.mark.skipif(
    sys.version_info >= (3, 14),
    reason="dYdX adapter requires Python < 3.14 (coincurve incompatibility)",
)


if sys.version_info < (3, 14):
    from nautilus_trader.adapters.dydx.common.enums import DYDXChannel
    from nautilus_trader.adapters.dydx.common.enums import DYDXOrderStatus


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_dydx_channel_enum_values():
    assert DYDXChannel.TRADES.value == "v4_trades"
    assert DYDXChannel.ORDERBOOK.value == "v4_orderbook"
    assert DYDXChannel.CANDLES.value == "v4_candles"
    assert DYDXChannel.MARKETS.value == "v4_markets"
    assert DYDXChannel.SUBACCOUNTS.value == "v4_subaccounts"
    assert DYDXChannel.BLOCK_HEIGHT.value == "v4_block_height"


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_dydx_order_status_enum_values():
    assert DYDXOrderStatus.OPEN.value == "OPEN"
    assert DYDXOrderStatus.FILLED.value == "FILLED"
    assert DYDXOrderStatus.CANCELED.value == "CANCELED"
    assert DYDXOrderStatus.BEST_EFFORT_CANCELED.value == "BEST_EFFORT_CANCELED"
    assert DYDXOrderStatus.UNTRIGGERED.value == "UNTRIGGERED"
    assert DYDXOrderStatus.BEST_EFFORT_OPENED.value == "BEST_EFFORT_OPENED"


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_enum_string_representation():
    channel = DYDXChannel.BLOCK_HEIGHT
    assert str(channel.value) == "v4_block_height"

    status = DYDXOrderStatus.OPEN
    assert str(status.value) == "OPEN"


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_enum_from_string():
    channel_str = "v4_block_height"
    channel = DYDXChannel(channel_str)
    assert channel == DYDXChannel.BLOCK_HEIGHT

    status_str = "FILLED"
    status = DYDXOrderStatus(status_str)
    assert status == DYDXOrderStatus.FILLED
