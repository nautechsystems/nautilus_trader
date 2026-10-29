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
Test polymarket factories behavior.
"""

import base64
from decimal import Decimal
from types import ModuleType

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketExecutionClientConfig
from nautilus_trader.adapters.polymarket import PolymarketExecutionClientFactory
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.adapters.polymarket import PolymarketPositionClient
from nautilus_trader.adapters.polymarket import PolymarketSignatureType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId


POLYMARKET = "POLYMARKET"
SMOKE_PRIVATE_KEY = "0x" + "0" * 63 + "1"
SMOKE_API_KEY = "test_api_key"
SMOKE_API_SECRET = base64.urlsafe_b64encode(b"test_secret").decode()
SMOKE_PASSPHRASE = "test_passphrase"
SMOKE_FUNDER = "0x0000000000000000000000000000000000000000"
UPDOWN_FIXTURE_INSTRUMENT = (
    "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b-"
    "104239898038807136052399800151408521467737075933964991162589336683346093173875."
    f"{POLYMARKET}"
)
polymarket_data_tester = load_example_module("polymarket", "data_tester")
polymarket_exec_tester = load_example_module("polymarket", "exec_tester")
polymarket_updown_smoke_tester = load_example_module("polymarket", "updown_smoke_tester")


def test_polymarket_factories_expose_python_names() -> None:
    """
    Test polymarket factories expose python names.
    """
    assert PolymarketDataClientFactory().name() == POLYMARKET
    assert PolymarketExecutionClientFactory().name() == POLYMARKET


def test_polymarket_signature_type_exposes_poly_1271() -> None:
    """
    Test polymarket signature type exposes poly 1271.
    """
    assert int(PolymarketSignatureType.Poly1271) == 3


def test_live_node_builder_accepts_polymarket_data_factory() -> None:
    """
    Test live node builder accepts polymarket data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("POLYMARKET-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=PolymarketInstrumentProviderConfig(
                    event_slugs=["gta-vi-released-before-june-2026"],
                ),
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_polymarket_exec_factory() -> None:
    """
    Test live node builder accepts polymarket exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("POLYMARKET-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=PolymarketInstrumentProviderConfig(
                    event_slugs=["gta-vi-released-before-june-2026"],
                ),
            ),
        )
        .add_exec_client(
            None,
            PolymarketExecutionClientFactory(),
            PolymarketExecutionClientConfig(
                account_id="POLYMARKET-001",
                private_key=SMOKE_PRIVATE_KEY,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                passphrase=SMOKE_PASSPHRASE,
                funder=SMOKE_FUNDER,
                signature_type=PolymarketSignatureType.PolyGnosisSafe,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_polymarket_data_tester_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test polymarket data tester runs.
    """
    captured = capture_data_tester_main(monkeypatch, polymarket_data_tester)
    kwargs = captured["data_tester_kwargs"]
    data_client_config = captured["data_client_args"][2]

    assert isinstance(kwargs, dict)
    assert kwargs["instrument_ids"] == [polymarket_data_tester.INSTRUMENT_ID]
    assert kwargs["subscribe_trades"] is True
    assert 'event_slugs: Some(["presidential-election-winner-2028"])' in repr(data_client_config)
    assert captured["run_called"] is True


def test_polymarket_exec_tester_defaults_to_dry_run(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Test polymarket exec tester defaults to dry run.
    """
    captured = capture_exec_tester_main(monkeypatch, polymarket_exec_tester)
    kwargs = captured["exec_tester_kwargs"]
    exec_engine_config = captured["exec_engine_config"]
    exec_engine_repr = repr(exec_engine_config)

    assert isinstance(kwargs, dict)
    assert (
        f'reconciliation_instrument_ids: Some(["{polymarket_exec_tester.INSTRUMENT_ID}"])'
        in exec_engine_repr
    )
    assert "open_check_interval_secs: Some(10.0)" in exec_engine_repr
    assert "position_check_interval_secs: Some(30.0)" in exec_engine_repr
    assert captured["reconciliation"] is True
    assert captured["timeout_disconnection_secs"] == 30
    assert captured["delay_post_stop_secs"] == 30
    assert kwargs["use_uuid_client_order_ids"] is True
    assert kwargs["dry_run"] is True
    assert "risk_engine_config" not in captured
    assert kwargs["enable_limit_buys"] is False
    assert kwargs["enable_limit_sells"] is False
    assert kwargs["use_post_only"] is False
    assert kwargs["use_quote_quantity"] is True
    assert kwargs["open_position_on_start_qty"] == Decimal(5)
    assert kwargs["cancel_orders_on_stop"] is True
    assert kwargs["close_positions_on_stop"] is True
    assert kwargs["close_positions_qty_precision"] == 2
    assert kwargs["close_positions_time_in_force"] == TimeInForce.IOC
    assert kwargs["enable_stop_buys"] is False
    assert kwargs["enable_stop_sells"] is False
    assert captured["run_called"] is True


def test_polymarket_updown_smoke_tester_defaults_to_dry_run(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test polymarket updown smoke tester defaults to dry run.
    """
    monkeypatch.setattr(
        polymarket_updown_smoke_tester,
        "resolve_updown_instrument_id",
        lambda **_: InstrumentId.from_str(UPDOWN_FIXTURE_INSTRUMENT),
    )
    captured = capture_exec_tester_main(monkeypatch, polymarket_updown_smoke_tester)
    data_client_config = captured["data_client_args"][2]
    exec_kwargs = captured["exec_tester_kwargs"]

    assert "event_slug_builder: Some" in repr(data_client_config)
    assert 'assets: ["btc"]' in repr(data_client_config)
    assert exec_kwargs["instrument_id"] == InstrumentId.from_str(UPDOWN_FIXTURE_INSTRUMENT)
    assert exec_kwargs["dry_run"] is True
    assert "risk_engine_config" not in captured
    assert captured["timeout_disconnection_secs"] == 30
    assert captured["delay_post_stop_secs"] == 30
    exec_client_config = captured["exec_client_args"][2]
    assert data_client_config.instrument_config.load_ids == [
        InstrumentId.from_str(UPDOWN_FIXTURE_INSTRUMENT),
    ]
    assert repr(exec_client_config.instrument_config) == repr(data_client_config.instrument_config)
    assert exec_kwargs["enable_limit_buys"] is True
    assert exec_kwargs["enable_limit_sells"] is False
    assert exec_kwargs["open_position_on_start_qty"] == Decimal(5)
    assert exec_kwargs["open_position_on_first_quote"] is True
    assert exec_kwargs["cancel_orders_on_stop"] is True
    assert exec_kwargs["close_positions_on_stop"] is True
    assert exec_kwargs["close_positions_qty_precision"] == 2
    assert exec_kwargs["close_positions_time_in_force"] == TimeInForce.IOC
    assert captured["run_called"] is True


def test_polymarket_updown_smoke_tester_builds_aligned_slugs() -> None:
    """
    Test polymarket updown smoke tester builds aligned slugs.
    """
    slugs = polymarket_updown_smoke_tester.build_updown_event_slugs(
        assets=["BTC", " eth ", "btc"],
        interval_mins=5,
        periods=2,
        start_offset_periods=0,
        unix_secs=1_700_000_000,
    )

    assert slugs == [
        "btc-updown-5m-1699999800",
        "eth-updown-5m-1699999800",
        "btc-updown-5m-1700000100",
        "eth-updown-5m-1700000100",
    ]


def test_polymarket_updown_smoke_tester_finds_outcome_token() -> None:
    """
    Test polymarket updown smoke tester finds outcome token.
    """
    events = [
        {
            "markets": [
                {
                    "conditionId": "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b",
                    "active": True,
                    "closed": False,
                    "acceptingOrders": True,
                    "enableOrderBook": True,
                    "outcomes": '["Up", "Down"]',
                    "clobTokenIds": '["111", "222"]',
                },
            ],
        },
    ]

    instrument_id = polymarket_updown_smoke_tester.find_updown_instrument_id(events, "down")

    assert (
        str(instrument_id)
        == "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b-222.POLYMARKET"
    )


@pytest.mark.parametrize("explicit_private_key", [False, True])
def test_execution_client_uses_environment_credentials(
    monkeypatch: pytest.MonkeyPatch,
    explicit_private_key: bool,
) -> None:
    """
    Test omitted credentials use the environment and explicit fields take precedence.
    """
    monkeypatch.setenv("POLYMARKET_PK", "invalid" if explicit_private_key else SMOKE_PRIVATE_KEY)
    monkeypatch.setenv("POLYMARKET_API_KEY", SMOKE_API_KEY)
    monkeypatch.setenv("POLYMARKET_API_SECRET", SMOKE_API_SECRET)
    monkeypatch.setenv("POLYMARKET_PASSPHRASE", SMOKE_PASSPHRASE)
    monkeypatch.delenv("POLYMARKET_FUNDER", raising=False)
    trader_id = TraderId.from_str("ENV-001")
    config = PolymarketExecutionClientConfig(
        private_key=SMOKE_PRIVATE_KEY if explicit_private_key else None,
    )
    node = (
        LiveNode.builder("POLYMARKET-ENV-PYTEST", trader_id, Environment.LIVE)
        .add_exec_client(None, PolymarketExecutionClientFactory(), config)
        .build()
    )
    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


@pytest.mark.parametrize("module", [polymarket_exec_tester, polymarket_updown_smoke_tester])
def test_execution_examples_require_explicit_live_order_opt_in(
    monkeypatch: pytest.MonkeyPatch,
    module: ModuleType,
) -> None:
    """
    Test a deliberate live-order opt-in reaches the strategy configuration.
    """
    if module is polymarket_updown_smoke_tester:
        monkeypatch.setattr(
            module,
            "resolve_updown_instrument_id",
            lambda **_: InstrumentId.from_str(UPDOWN_FIXTURE_INSTRUMENT),
        )
    monkeypatch.setattr(module, "DRY_RUN", False)
    captured = capture_exec_tester_main(monkeypatch, module)
    assert captured["exec_tester_kwargs"]["dry_run"] is False
    assert "risk_engine_config" not in captured


@pytest.mark.parametrize("relayer_api_key", [None, "", " \t"])
def test_position_client_preserves_relayer_environment_fallback(
    monkeypatch: pytest.MonkeyPatch,
    relayer_api_key: str | None,
) -> None:
    """
    Test omitted and blank position-client Relayer keys retain environment fallback.
    """
    monkeypatch.setenv("POLYMARKET_RELAYER_API_KEY", "test-relayer-key")
    client = PolymarketPositionClient(
        private_key=SMOKE_PRIVATE_KEY,
        funder=SMOKE_FUNDER,
        relayer_api_key=relayer_api_key,
        relayer_api_key_address=SMOKE_FUNDER,
    )
    assert type(client) is PolymarketPositionClient
