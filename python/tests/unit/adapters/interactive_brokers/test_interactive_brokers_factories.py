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
Test interactive brokers factories behavior.
"""

from pathlib import Path
from uuid import UUID

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecutionClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecutionClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersSubscriptionIdle
from nautilus_trader.adapters.interactive_brokers import MarketDataType
from nautilus_trader.adapters.interactive_brokers import SymbologyMethod
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import ClientId
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId


IB = "IB"
IB_EXAMPLES_DIR = Path(__file__).resolve().parents[5] / "examples" / "live" / "interactive_brokers"
IB_NOTEBOOK_SCRIPTS = sorted((IB_EXAMPLES_DIR / "notebooks").glob("*.py"))
ib_data_tester = load_example_module("interactive_brokers", "data_tester")
ib_exec_tester = load_example_module("interactive_brokers", "exec_tester")
ib_order_strategies = load_example_module("interactive_brokers", "ib_v2_order_strategies")
ib_common = load_example_module("interactive_brokers", "_common")


def test_interactive_brokers_factories_expose_python_names() -> None:
    """
    Test interactive brokers factories expose python names.
    """
    assert InteractiveBrokersDataClientFactory().name() == IB
    assert InteractiveBrokersExecutionClientFactory().name() == IB


def test_interactive_brokers_instrument_provider_config_and_empty_surface() -> None:
    """
    Test interactive brokers instrument provider config and empty surface.
    """
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    contract = {"secType": "STK", "symbol": "MSFT", "exchange": "SMART"}
    config = InteractiveBrokersInstrumentProviderConfig(
        symbology_method=SymbologyMethod.RAW,
        load_ids={instrument_id},
        load_contracts=[contract],
        min_expiry_days=2,
        max_expiry_days=30,
        build_options_chain=True,
        build_futures_chain=False,
        cache_validity_days=7,
        convert_exchange_to_mic_venue=True,
        symbol_to_mic_venue={"AAPL": "XNAS"},
        filter_sec_types={"OPT", "STK"},
        filter_callable="package.module:filter_instrument",
        cache_path="cache/instruments.json",
    )
    provider = InteractiveBrokersInstrumentProvider(config)

    assert config.symbology_method == SymbologyMethod.RAW
    assert config.load_ids == {instrument_id}
    assert config.load_contracts == [
        {
            "comboLegsDescrip": "",
            "conId": 0,
            "currency": "",
            "description": "",
            "exchange": "SMART",
            "includeExpired": False,
            "issuerId": "",
            "lastTradeDateOrContractMonth": "",
            "localSymbol": "",
            "multiplier": "",
            "primaryExchange": "",
            "right": None,
            "secId": "",
            "secIdType": None,
            "secType": "STK",
            "strike": 0.0,
            "symbol": "MSFT",
            "tradingClass": "",
        },
    ]
    assert config.min_expiry_days == 2
    assert config.max_expiry_days == 30
    assert config.build_options_chain is True
    assert config.build_futures_chain is False
    assert config.cache_validity_days == 7
    assert config.convert_exchange_to_mic_venue is True
    assert config.symbol_to_mic_venue == {"AAPL": "XNAS"}
    assert set(config.filter_sec_types) == {"OPT", "STK"}
    assert config.filter_callable == "package.module:filter_instrument"
    assert config.cache_path == "cache/instruments.json"
    assert provider.count() == 0
    assert provider.get_all() == []
    assert provider.find(instrument_id) is None


def test_interactive_brokers_example_futures_symbols_keep_the_venue_local_symbol() -> None:
    """
    Test interactive brokers example futures symbols keep the venue local symbol.
    """
    assert ib_common.contract_month_code(2026, 12) == "Z6"
    assert ib_common.active_quarterly_contract(
        symbol="ES",
        venue="XCME",
        today=ib_common.dt.date(2026, 1, 1),
    ) == ("ESH6", "ESH6.XCME", "20260320")
    assert ib_common.active_monthly_contract(
        symbol="CL",
        venue="XNYM",
        today=ib_common.dt.date(2026, 1, 1),
    ) == ("CLH6", "CLH6.XNYM", "202603")


def test_live_node_builder_accepts_interactive_brokers_data_factory() -> None:
    """
    Test live node builder accepts interactive brokers data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("IB-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            InteractiveBrokersDataClientFactory(),
            InteractiveBrokersDataClientConfig(
                client_id=101,
                market_data_type=MarketDataType.DELAYED,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_interactive_brokers_exec_factory() -> None:
    """
    Test live node builder accepts interactive brokers exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")
    node = (
        LiveNode.builder("IB-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            InteractiveBrokersDataClientFactory(),
            InteractiveBrokersDataClientConfig(
                client_id=101,
                market_data_type=MarketDataType.DELAYED,
            ),
        )
        .add_exec_client(
            None,
            InteractiveBrokersExecutionClientFactory(),
            InteractiveBrokersExecutionClientConfig(client_id=101, account_id="U1234567"),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_interactive_brokers_data_tester_runs(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers data tester runs.
    """
    captured = capture_data_tester_main(monkeypatch, ib_data_tester)
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["request_instruments"] is True
    assert captured["run_called"] is True


def test_interactive_brokers_exec_tester_requires_account(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers exec tester requires account.
    """
    monkeypatch.delenv("TWS_ACCOUNT", raising=False)

    with pytest.raises(SystemExit, match="TWS_ACCOUNT must be set"):
        ib_exec_tester.main()


def test_interactive_brokers_exec_tester_runs_live_orders(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers exec tester runs live orders.
    """
    monkeypatch.setenv("TWS_ACCOUNT", "U1234567")
    captured = capture_exec_tester_main(monkeypatch, ib_exec_tester)
    kwargs = captured["exec_tester_kwargs"]
    _, _, _exec_config = captured["exec_client_args"]

    assert isinstance(kwargs, dict)
    assert kwargs["use_uuid_client_order_ids"] is True
    assert "external_order_claims" not in kwargs
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert kwargs["subscribe_trades"] is False
    assert kwargs["use_post_only"] is False
    assert kwargs["cancel_orders_on_stop"] is True
    assert kwargs["close_positions_on_stop"] is True
    assert captured["run_called"] is True


def test_interactive_brokers_order_examples_use_per_run_client_order_ids(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers order examples use per run client order ids.
    """
    run_ids = iter(
        [
            UUID("11111111-1111-4111-8111-111111111111"),
            UUID("22222222-2222-4222-8222-222222222222"),
        ],
    )
    monkeypatch.setattr(ib_order_strategies, "uuid4", lambda: next(run_ids))

    first = ib_order_strategies.IbV2OrderStrategy()
    second = ib_order_strategies.IbV2OrderStrategy()

    assert str(first.client_order_id("ENTRY")) == "IBV2-11111111-ENTRY"
    assert str(second.client_order_id("ENTRY")) == "IBV2-22222222-ENTRY"


def test_interactive_brokers_order_examples_clean_state_on_stop(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers order examples clean state on stop.
    """
    calls: list[tuple[str, object, object]] = []

    def record_cancel(strategy: object, instrument_id: object, client_id: object) -> None:
        calls.append(("cancel", instrument_id, client_id))

    def record_close(strategy: object, instrument_id: object, client_id: object) -> None:
        calls.append(("close", instrument_id, client_id))

    def record_unsubscribe(strategy: object, instrument_id: object, client_id: object) -> None:
        calls.append(("unsubscribe", instrument_id, client_id))

    monkeypatch.setenv("IB_V2_ENABLE_ORDER_SUBMISSION", "1")
    monkeypatch.setattr(
        ib_order_strategies.IbV2OrderStrategy,
        "cancel_all_orders",
        record_cancel,
    )
    monkeypatch.setattr(
        ib_order_strategies.IbV2OrderStrategy,
        "close_all_positions",
        record_close,
    )
    monkeypatch.setattr(
        ib_order_strategies.IbV2OrderStrategy,
        "unsubscribe_quotes",
        record_unsubscribe,
    )
    strategy = ib_order_strategies.IbV2OrderStrategy()
    strategy._quotes_subscribed = True

    strategy.on_stop()

    assert calls == [
        ("cancel", strategy.instrument_id, ib_order_strategies.ib_client_id()),
        ("close", strategy.instrument_id, ib_order_strategies.ib_client_id()),
        ("unsubscribe", strategy.instrument_id, ib_order_strategies.ib_client_id()),
    ]


@pytest.mark.parametrize(
    "script_path",
    IB_NOTEBOOK_SCRIPTS,
    ids=lambda path: path.name,
)
def test_interactive_brokers_notebook_inlines_setup_and_strategy(script_path: Path) -> None:
    """
    Test interactive brokers notebook inlines setup and strategy.
    """
    source = script_path.read_text(encoding="utf-8")

    assert "# %% [markdown]" in source
    assert "build_ib_live_node" in source
    assert "node.add_strategy(" in source
    assert "order_example_driver" not in source
    assert "add_strategy_from_config" not in source


def test_trade_selection_and_idle_config() -> None:
    """
    Test trade selection and idle config.
    """
    default = InteractiveBrokersDataClientConfig()
    selected = InteractiveBrokersDataClientConfig(
        all_last_trades=False,
        subscription_idle_timeout_secs=17,
    )
    assert default.all_last_trades is True
    assert default.subscription_idle_timeout_secs is None
    assert selected.all_last_trades is False
    assert selected.subscription_idle_timeout_secs == 17


@pytest.mark.parametrize("timeout", [0, 2**64 - 1])
def test_invalid_subscription_idle_timeout(timeout: int) -> None:
    """
    Test invalid subscription idle timeout.
    """
    with pytest.raises(ValueError, match="subscription_idle_timeout_secs"):
        InteractiveBrokersDataClientConfig(subscription_idle_timeout_secs=timeout)


@pytest.mark.parametrize("last_received", [None, 123])
def test_subscription_idle_custom_data_preserves_fields(last_received: int | None) -> None:
    """
    Test subscription idle custom data preserves fields.
    """
    event = InteractiveBrokersSubscriptionIdle(
        ClientId("IB-DATA-17"),
        InstrumentId.from_str("AAPL=STK.SMART"),
        "trades",
        17,
        last_received,
        456,
        789,
    )
    custom = CustomData(DataType("InteractiveBrokersSubscriptionIdle"), event)
    payload = custom.data
    assert isinstance(payload, InteractiveBrokersSubscriptionIdle)
    assert payload.client_id == ClientId("IB-DATA-17")
    assert payload.instrument_id == InstrumentId.from_str("AAPL=STK.SMART")
    assert payload.subscription == "trades"
    assert payload.idle_timeout_secs == 17
    assert payload.last_data_received_ns == last_received
    assert payload.ts_event == 456
    assert payload.ts_init == 789
