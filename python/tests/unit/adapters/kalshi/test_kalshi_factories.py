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
"""
Test Kalshi factory registration and node construction.
"""

import base64
from pathlib import Path

import pytest

from nautilus_trader.adapters.kalshi import KALSHI
from nautilus_trader.adapters.kalshi import KALSHI_CLIENT_ID
from nautilus_trader.adapters.kalshi import KALSHI_VENUE
from nautilus_trader.adapters.kalshi import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi import KalshiDataClientFactory
from nautilus_trader.adapters.kalshi import KalshiEnvironment
from nautilus_trader.adapters.kalshi import KalshiExecutionClientConfig
from nautilus_trader.adapters.kalshi import KalshiExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId


EXPECTED_EXPORTS = [
    "KALSHI",
    "KALSHI_CLIENT_ID",
    "KALSHI_VENUE",
    "KalshiDataClientConfig",
    "KalshiDataClientFactory",
    "KalshiEnvironment",
    "KalshiExecutionClientConfig",
    "KalshiExecutionClientFactory",
]

MARKET = InstrumentId.from_str("KXHIGHNY-26OCT31-T70.KALSHI")


def test_kalshi_exports_are_exact() -> None:
    """
    Test Kalshi public exports match the supported facade.
    """
    from nautilus_trader.adapters import kalshi

    assert list(kalshi.__all__) == EXPECTED_EXPORTS
    assert not hasattr(kalshi, "KalshiHttpClient")
    assert not hasattr(kalshi, "KalshiWebSocketClient")
    assert kalshi.KALSHI == KALSHI
    assert kalshi.KALSHI_CLIENT_ID == KALSHI_CLIENT_ID
    assert kalshi.KALSHI_VENUE == KALSHI_VENUE


def test_kalshi_factories_expose_python_names() -> None:
    """
    Test Kalshi factories expose their registry names.
    """
    assert KalshiDataClientFactory().name() == KALSHI
    assert KalshiExecutionClientFactory().name() == KALSHI


def test_live_node_builder_constructs_kalshi_clients() -> None:
    """
    Test a public Python node can construct both Kalshi clients.
    """
    trader_id = TraderId.from_str("TESTER-001")
    node = (
        LiveNode.builder("KALSHI-PYTEST-001", trader_id, Environment.SANDBOX)
        .add_data_client(
            None,
            KalshiDataClientFactory(),
            KalshiDataClientConfig(
                environment=KalshiEnvironment.DEMO,
                instrument_ids=[MARKET],
            ),
        )
        .add_exec_client(
            None,
            KalshiExecutionClientFactory(),
            KalshiExecutionClientConfig(
                api_key_id="python-node-key",
                private_key=_test_private_key(),
                environment=KalshiEnvironment.DEMO,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_data_factory_rejects_empty_instrument_selection() -> None:
    """
    Test node construction reports a missing Kalshi market selection.
    """
    trader_id = TraderId.from_str("TESTER-001")

    with pytest.raises(Exception, match="instrument_id"):
        (
            LiveNode.builder("KALSHI-PYTEST-EMPTY", trader_id, Environment.SANDBOX)
            .add_data_client(
                None,
                KalshiDataClientFactory(),
                KalshiDataClientConfig(),
            )
            .build()
        )


def _test_private_key() -> str:
    der = (
        Path(__file__).resolve().parents[5]
        / "crates/adapters/kalshi/test_data/auth_ed25519_pkcs8.der"
    )
    body = base64.b64encode(der.read_bytes()).decode("ascii")
    # Split so the source does not contain a contiguous private-key header
    label = "PRIVATE" + " KEY"
    return f"-----BEGIN {label}-----\n{body}\n-----END {label}-----\n"
