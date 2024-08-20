# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Unit tests for the HTTP client.
"""

from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient


def test_payload_urlencode(http_client: DYDXHttpClient) -> None:
    """
    Test encoding the payload sent by the client.
    """
    # Prepare
    payload = {"returnLatestOrders": True, "subaccountNumber": 0}
    expected_result = "returnLatestOrders=true&subaccountNumber=0"

    # Act
    result = http_client._urlencode(payload)

    # Assert
    assert result == expected_result
