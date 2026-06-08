# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.adapters.okx import OKXHttpClient


def test_http_client_exposes_generic_spread_execution_methods() -> None:
    assert hasattr(OKXHttpClient, "place_order")
    assert hasattr(OKXHttpClient, "cancel_order")
    assert hasattr(OKXHttpClient, "cancel_all_orders")
    assert hasattr(OKXHttpClient, "request_order_status_reports")
    assert hasattr(OKXHttpClient, "request_fill_reports")


def test_http_client_does_not_expose_spread_specific_execution_methods() -> None:
    assert not hasattr(OKXHttpClient, "place_spread_order")
    assert not hasattr(OKXHttpClient, "cancel_spread_order")
    assert not hasattr(OKXHttpClient, "cancel_all_spread_orders")
    assert not hasattr(OKXHttpClient, "request_spread_order_status_reports")
    assert not hasattr(OKXHttpClient, "request_spread_fill_reports")
