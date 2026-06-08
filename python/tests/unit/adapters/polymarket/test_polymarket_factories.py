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

import base64

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket import PolymarketExecutionClientFactory
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.adapters.polymarket import SignatureType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
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
    assert PolymarketDataClientFactory().name() == POLYMARKET
    assert PolymarketExecutionClientFactory().name() == POLYMARKET


def test_polymarket_signature_type_exposes_poly_1271() -> None:
    assert int(SignatureType.Poly1271) == 3


def test_live_node_builder_accepts_polymarket_data_factory() -> None:
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
            PolymarketExecClientConfig(
                trader_id="TESTER-001",
                account_id="POLYMARKET-001",
                private_key=SMOKE_PRIVATE_KEY,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                passphrase=SMOKE_PASSPHRASE,
                funder=SMOKE_FUNDER,
                signature_type=SignatureType.PolyGnosisSafe,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_polymarket_data_tester_builds_offline(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, polymarket_data_tester, [])
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["subscribe_trades"] is True
    assert "run_called" not in captured


@pytest.mark.parametrize(
    ("extra_args", "expected_dry_run", "expected_limit_sells"),
    [
        ([], True, False),
        (["--live-orders", "--limit-sells"], False, True),
    ],
)
def test_polymarket_exec_tester_gates_live_orders(
    monkeypatch: pytest.MonkeyPatch,
    extra_args: list[str],
    expected_dry_run: bool,
    expected_limit_sells: bool,
) -> None:
    captured = capture_exec_tester_main(monkeypatch, polymarket_exec_tester, extra_args)
    kwargs = captured["exec_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["dry_run"] is expected_dry_run
    assert kwargs["enable_limit_sells"] is expected_limit_sells
    assert kwargs["enable_stop_buys"] is False
    assert kwargs["enable_stop_sells"] is False
    assert "run_called" not in captured


def test_polymarket_updown_smoke_tester_uses_event_slug_builder(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured = capture_exec_tester_main(
        monkeypatch,
        polymarket_updown_smoke_tester,
        ["--instrument", UPDOWN_FIXTURE_INSTRUMENT],
    )
    data_client_config = captured["data_client_args"][2]
    exec_kwargs = captured["exec_tester_kwargs"]

    assert "event_slug_builder: Some" in repr(data_client_config)
    assert 'assets: ["btc"]' in repr(data_client_config)
    assert exec_kwargs["dry_run"] is True
    assert exec_kwargs["enable_limit_buys"] is False
    assert exec_kwargs["enable_limit_sells"] is False
    assert exec_kwargs["open_position_on_start_qty"] is None
    assert exec_kwargs["open_position_on_first_quote"] is False
    assert "run_called" not in captured


def test_polymarket_updown_smoke_tester_live_orders_are_opt_in(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured = capture_exec_tester_main(
        monkeypatch,
        polymarket_updown_smoke_tester,
        [
            "--instrument",
            UPDOWN_FIXTURE_INSTRUMENT,
            "--live-orders",
            "--limit-sells",
        ],
    )
    exec_kwargs = captured["exec_tester_kwargs"]

    assert exec_kwargs["dry_run"] is False
    assert exec_kwargs["enable_limit_buys"] is True
    assert exec_kwargs["enable_limit_sells"] is True
    assert exec_kwargs["open_position_on_start_qty"] is not None
    assert exec_kwargs["open_position_on_first_quote"] is True
    assert exec_kwargs["cancel_orders_on_stop"] is True
    assert exec_kwargs["close_positions_on_stop"] is True
    assert "run_called" not in captured


def test_polymarket_updown_smoke_tester_builds_aligned_slugs() -> None:
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
