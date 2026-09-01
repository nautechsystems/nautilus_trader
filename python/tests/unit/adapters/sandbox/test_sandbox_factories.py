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
Test sandbox factories behavior.
"""

import pytest
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceSpotMarketDataMode
from nautilus_trader.adapters.sandbox import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox import SandboxExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.execution import DefaultFillModel
from nautilus_trader.execution import FeeModel
from nautilus_trader.execution import ProbabilityPriceFeeModel
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue


SANDBOX = "SANDBOX"
sandbox_exec_tester = load_example_module("sandbox", "exec_tester")


def test_sandbox_execution_factory_exposes_python_name() -> None:
    """
    Test sandbox execution factory exposes python name.
    """
    assert SandboxExecutionClientFactory().name() == SANDBOX


def test_live_node_builder_accepts_sandbox_simulated_exec_factory() -> None:
    """
    Test live node builder accepts sandbox simulated exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("SANDBOX-EXEC-PYTEST-001", trader_id, Environment.SANDBOX)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_simulated_exec_client(
            None,
            SandboxExecutionClientFactory(),
            SandboxExecutionClientConfig(
                venue=Venue.from_str(SANDBOX),
                starting_balances=[Money(100000.0, Currency.from_str("USD"))],
                account_id=AccountId.from_str("SANDBOX-001"),
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_live_node_builder_accepts_sandbox_probability_price_fee_model() -> None:
    """
    Test live node builder accepts sandbox probability price fee model.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("SANDBOX-EXEC-PYTEST-002", trader_id, Environment.SANDBOX)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_simulated_exec_client(
            None,
            SandboxExecutionClientFactory(),
            SandboxExecutionClientConfig(
                venue=Venue.from_str(SANDBOX),
                starting_balances=[Money(100000.0, Currency.from_str("USD"))],
                account_id=AccountId.from_str("SANDBOX-001"),
                fee_model=ProbabilityPriceFeeModel(),
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_sandbox_config_exposes_fee_model_property() -> None:
    """
    Test sandbox config exposes fee model property.
    """
    config = SandboxExecutionClientConfig(
        venue=Venue.from_str(SANDBOX),
        starting_balances=[Money(100000.0, Currency.from_str("USD"))],
        fee_model=ProbabilityPriceFeeModel(),
    )

    assert isinstance(config.fee_model, ProbabilityPriceFeeModel)


def test_live_node_builder_accepts_sandbox_matching_knobs() -> None:
    """
    Test live node builder accepts sandbox matching knobs.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("SANDBOX-EXEC-PYTEST-003", trader_id, Environment.SANDBOX)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_simulated_exec_client(
            None,
            SandboxExecutionClientFactory(),
            SandboxExecutionClientConfig(
                venue=Venue.from_str(SANDBOX),
                starting_balances=[Money(100000.0, Currency.from_str("USD"))],
                account_id=AccountId.from_str("SANDBOX-001"),
                fill_model=DefaultFillModel(prob_fill_on_limit=0.0),
                queue_position=True,
                liquidity_consumption=True,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_sandbox_config_exposes_matching_knobs() -> None:
    """
    Test sandbox config exposes matching knobs.
    """
    config = SandboxExecutionClientConfig(
        venue=Venue.from_str(SANDBOX),
        starting_balances=[Money(100000.0, Currency.from_str("USD"))],
        fill_model=DefaultFillModel(prob_fill_on_limit=0.0),
        queue_position=True,
        liquidity_consumption=True,
        bar_adaptive_high_low_ordering=True,
        use_market_order_acks=True,
        oto_full_trigger=True,
        price_protection_points=100,
    )

    assert isinstance(config.fill_model, DefaultFillModel)
    assert config.queue_position is True
    assert config.liquidity_consumption is True
    assert config.bar_adaptive_high_low_ordering is True
    assert config.use_market_order_acks is True
    assert config.oto_full_trigger is True
    assert config.price_protection_points == 100


def test_sandbox_config_accepts_custom_fee_model() -> None:
    """
    Test sandbox config accepts custom fee model.
    """

    class CustomFeeModel(FeeModel):
        """
        Collect custom fee model tests.
        """

        def get_commission(
            self,
            _order: object,
            _fill_quantity: object,
            _fill_px: object,
            _instrument: object,
        ) -> object:
            """
            Get commission.
            """
            return Money.from_str("1.23 USD")

    fee_model = CustomFeeModel()
    config = SandboxExecutionClientConfig(
        venue=Venue.from_str(SANDBOX),
        starting_balances=[Money(100000.0, Currency.from_str("USD"))],
        fee_model=fee_model,
    )

    assert config.fee_model is fee_model


def test_sandbox_exec_tester_uses_simulated_exec_and_runs(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test sandbox exec tester uses simulated exec and runs.
    """
    captured = capture_exec_tester_main(monkeypatch, sandbox_exec_tester)
    kwargs = captured["exec_tester_kwargs"]
    _, _, data_config = captured["data_client_args"]
    simulated_venue, _, simulated_config = captured["simulated_exec_client_args"]

    assert isinstance(kwargs, dict)
    assert isinstance(data_config, BinanceDataClientConfig)
    assert data_config.spot_market_data_mode == BinanceSpotMarketDataMode.Json
    assert simulated_venue == "BINANCE"
    assert isinstance(simulated_config, SandboxExecutionClientConfig)
    assert simulated_config.venue == Venue.from_str("BINANCE")
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert "simulated_exec_client_args" in captured
    assert "data_client_args" in captured
    assert "exec_client_args" not in captured
    assert captured["run_called"] is True
