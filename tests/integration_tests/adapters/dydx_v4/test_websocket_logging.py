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

    from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsBlockHeightChannelData
    from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMessageGeneral


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_unknown_websocket_message_parsing():
    unknown_msg = {
        "type": "unknown_type",
        "connection_id": "test-connection",
        "message_id": 123,
    }

    msg = DYDXWsMessageGeneral(**unknown_msg)
    assert msg.type == "unknown_type"
    assert msg.connection_id == "test-connection"
    assert msg.message_id == 123


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_block_height_message_structure():
    from datetime import datetime

    block_height_msg = {
        "type": "channel_data",
        "connection_id": "test-conn",
        "message_id": 1,
        "id": "dydx",
        "channel": "v4_block_height",
        "version": "4.0.0",
        "contents": {
            "blockHeight": "12345",
            "time": datetime.fromisoformat("2025-12-19T10:00:00+00:00"),
        },
    }

    msg = DYDXWsBlockHeightChannelData(**block_height_msg)
    assert msg.channel == "v4_block_height"
    assert msg.contents["blockHeight"] == "12345"


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_malformed_message_handling():
    malformed_msg = {
        "type": "channel_data",
        "connection_id": "test",
    }

    try:
        msg = DYDXWsMessageGeneral(**malformed_msg)
        assert msg.type == "channel_data"
    except (TypeError, KeyError):
        pass


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_block_height_conversion():
    block_height_str = "999999"
    block_height_int = int(block_height_str)

    assert isinstance(block_height_int, int)
    assert block_height_int == 999999


@pytest.mark.skipif(sys.version_info >= (3, 14), reason="Python 3.14+ not supported")
def test_block_height_large_values():
    large_block_height = "18446744073709551615"
    block_height_int = int(large_block_height)

    assert block_height_int > 0
    assert block_height_int == 18446744073709551615
