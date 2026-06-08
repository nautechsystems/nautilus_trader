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
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.tardis import TardisDataClientConfig
from nautilus_trader.adapters.tardis import TardisDataClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId


TARDIS = "TARDIS"
tardis_data_tester = load_example_module("tardis", "data_tester")


def test_tardis_data_factory_exposes_python_name() -> None:
    assert TardisDataClientFactory().name() == TARDIS


def test_live_node_builder_accepts_tardis_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("TARDIS-DATA-PYTEST-001", trader_id, Environment.SANDBOX)
        .add_data_client(
            None,
            TardisDataClientFactory(),
            TardisDataClientConfig(),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_tardis_data_tester_builds_offline(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, tardis_data_tester, [])
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_funding_rates"] is True
    assert "exec_client_args" not in captured
    assert "run_called" not in captured
