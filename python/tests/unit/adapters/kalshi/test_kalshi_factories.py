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
Test kalshi factories behavior.
"""

import pytest

from nautilus_trader.adapters import kalshi
from nautilus_trader.adapters.kalshi import KALSHI
from nautilus_trader.adapters.kalshi import KALSHI_CLIENT_ID
from nautilus_trader.adapters.kalshi import KALSHI_VENUE
from nautilus_trader.adapters.kalshi import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi import KalshiDataClientFactory
from nautilus_trader.adapters.kalshi import KalshiExecutionClientConfig
from nautilus_trader.adapters.kalshi import KalshiExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import ClientId
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue


KALSHI_EXEC_CLIENT_ID = "KALSHI-EXEC"
SMOKE_API_KEY_ID = "test_api_key_id"
SMOKE_API_KEY_PEM = "test-pem-placeholder"


def test_kalshi_facade_exposes_canonical_constants() -> None:
    """
    Test kalshi facade exposes canonical constants.
    """
    assert KALSHI == "KALSHI"
    assert ClientId.from_str(KALSHI_EXEC_CLIENT_ID) == KALSHI_CLIENT_ID
    assert Venue.from_str("KALSHI") == KALSHI_VENUE


def test_kalshi_facade_all_is_contract() -> None:
    """
    Test kalshi facade all is contract.
    """
    expected = [
        "KALSHI",
        "KALSHI_CLIENT_ID",
        "KALSHI_VENUE",
        "KalshiDataClientConfig",
        "KalshiDataClientFactory",
        "KalshiExecutionClientConfig",
        "KalshiExecutionClientFactory",
    ]

    assert list(kalshi.__all__) == expected


def test_kalshi_factories_expose_python_names() -> None:
    """
    Test kalshi factories expose python names.
    """
    assert KalshiDataClientFactory().name() == KALSHI
    assert KalshiExecutionClientFactory().name() == KALSHI


def test_kalshi_configs_construct_with_documented_defaults() -> None:
    """
    Test kalshi configs construct with documented defaults.
    """
    # Zero-argument construction has to pick up the documented Rust defaults
    # (demo environment, a 2000 millisecond poll interval, account KALSHI-001).
    data_config = KalshiDataClientConfig()
    exec_config = KalshiExecutionClientConfig()

    assert isinstance(data_config, KalshiDataClientConfig)
    assert isinstance(exec_config, KalshiExecutionClientConfig)

    overridden = KalshiDataClientConfig(
        event_tickers=["KXBTCD"],
        series_ticker="KXBTCD",
        poll_interval_millis=1_500,
    )
    exec_overridden = KalshiExecutionClientConfig(
        reconciliation=False,
        poll_interval_millis=1_500,
    )

    assert isinstance(overridden, KalshiDataClientConfig)
    assert isinstance(exec_overridden, KalshiExecutionClientConfig)


def test_live_node_builder_accepts_kalshi_data_factory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test live node builder accepts kalshi data factory.
    """
    monkeypatch.setenv("KALSHI_API_KEY_ID", SMOKE_API_KEY_ID)
    monkeypatch.setenv("KALSHI_API_KEY_PEM", SMOKE_API_KEY_PEM)

    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("KALSHI-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            KalshiDataClientFactory(),
            KalshiDataClientConfig(),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_kalshi_exec_factory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test live node builder accepts kalshi exec factory.
    """
    monkeypatch.setenv("KALSHI_API_KEY_ID", SMOKE_API_KEY_ID)
    monkeypatch.setenv("KALSHI_API_KEY_PEM", SMOKE_API_KEY_PEM)

    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("KALSHI-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            KalshiDataClientFactory(),
            KalshiDataClientConfig(),
        )
        .add_exec_client(
            None,
            KalshiExecutionClientFactory(),
            KalshiExecutionClientConfig(),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE
