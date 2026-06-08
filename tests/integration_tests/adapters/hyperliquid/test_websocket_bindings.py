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

import inspect

import pytest

from nautilus_trader.core import nautilus_pyo3


TEST_PRIVATE_KEY = "0x" + ("12" * 32)


@pytest.mark.asyncio
async def test_websocket_trading_binding_surface_and_empty_cancel_short_circuit():
    ws_client = nautilus_pyo3.HyperliquidWebSocketClient(
        url="ws://127.0.0.1:9/ws",
        environment=nautilus_pyo3.HyperliquidEnvironment.MAINNET,
        account_id=None,
        proxy_url=None,
    )
    signer = nautilus_pyo3.HyperliquidHttpClient(
        private_key=TEST_PRIVATE_KEY,
        environment=nautilus_pyo3.HyperliquidEnvironment.MAINNET,
        timeout_secs=1,
    )

    ws_client.set_post_timeout(timeout_secs=1)
    result = await ws_client.cancel_orders(signer=signer, cancels=[])

    signatures = {
        name: list(inspect.signature(getattr(ws_client, name)).parameters)
        for name in (
            "submit_order",
            "submit_orders",
            "cancel_order",
            "cancel_orders",
            "modify_order",
        )
    }

    assert result == []
    assert signatures["submit_order"] == [
        "signer",
        "instrument_id",
        "client_order_id",
        "order_side",
        "order_type",
        "quantity",
        "time_in_force",
        "price",
        "trigger_price",
        "post_only",
        "reduce_only",
    ]
    assert signatures["submit_orders"] == ["signer", "orders"]
    assert signatures["cancel_order"] == [
        "signer",
        "instrument_id",
        "client_order_id",
        "venue_order_id",
    ]
    assert signatures["cancel_orders"] == ["signer", "cancels"]
    assert signatures["modify_order"] == [
        "signer",
        "instrument_id",
        "venue_order_id",
        "order_side",
        "order_type",
        "price",
        "quantity",
        "trigger_price",
        "reduce_only",
        "post_only",
        "time_in_force",
        "client_order_id",
    ]
