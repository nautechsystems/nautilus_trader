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
Test databento factories behavior.
"""

from pathlib import Path

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.databento import DatabentoDataClientConfig
from nautilus_trader.adapters.databento import DatabentoDataClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId


DATABENTO = "DATABENTO"
SMOKE_API_KEY = "00000000000000000000000000000000"
databento_data_tester = load_example_module("databento", "data_tester")


def test_databento_data_factory_exposes_python_name() -> None:
    """
    Test databento data factory exposes python name.
    """
    assert DatabentoDataClientFactory().name() == DATABENTO


def test_live_node_builder_accepts_databento_data_factory() -> None:
    """
    Test live node builder accepts databento data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("DATABENTO-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            DatabentoDataClientFactory(),
            DatabentoDataClientConfig(
                api_key=SMOKE_API_KEY,
                publishers_filepath=publishers_filepath(),
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_databento_data_client_config_stores_venue_dataset_map() -> None:
    """
    Test databento data client config stores venue dataset map.
    """
    config = DatabentoDataClientConfig(
        api_key=SMOKE_API_KEY,
        publishers_filepath=publishers_filepath(),
        venue_dataset_map={"EQUS": "EQUS.PLUS"},
    )

    # No field getter is exposed, so the repr is the observable for the stored override.
    assert "EQUS" in repr(config)
    assert "EQUS.PLUS" in repr(config)


def test_databento_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test databento data tester runs.
    """
    monkeypatch.setenv("DATABENTO_API_KEY", "test-api-key")
    captured = capture_data_tester_main(monkeypatch, databento_data_tester)
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_trades"] is True
    assert "exec_client_args" not in captured
    assert captured["run_called"] is True


def test_databento_data_tester_requires_api_key(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test databento data tester requires api key.
    """
    monkeypatch.delenv("DATABENTO_API_KEY", raising=False)

    with pytest.raises(SystemExit, match="DATABENTO_API_KEY must be set"):
        databento_data_tester.main()


def publishers_filepath() -> Path:
    """
    Return the publishers filepath.
    """
    return Path(__file__).resolve().parents[5] / "crates/adapters/databento/publishers.json"
