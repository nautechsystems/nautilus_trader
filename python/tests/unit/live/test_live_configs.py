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

from nautilus_trader.live import InstrumentProviderConfig
from nautilus_trader.live import LiveDataClientConfig
from nautilus_trader.live import LiveDataEngineConfig
from nautilus_trader.live import LiveExecClientConfig
from nautilus_trader.live import LiveExecEngineConfig
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.live import PortfolioConfig
from nautilus_trader.live import RoutingConfig


def test_instrument_provider_config_defaults():
    config = InstrumentProviderConfig()

    assert config.load_all is False
    assert config.load_ids is None
    assert config.filters == {}
    assert config.filter_callable is None
    assert config.log_warnings is True


def test_instrument_provider_config_explicit():
    config = InstrumentProviderConfig(
        load_all=True,
        load_ids=["BTCUSDT-PERP.BINANCE"],
        filters={"exchange": "BINANCE"},
        filter_callable="my_module:my_filter",
        log_warnings=False,
    )

    assert config.load_all is True
    assert config.load_ids == ["BTCUSDT-PERP.BINANCE"]
    assert config.filters == {"exchange": "BINANCE"}
    assert config.filter_callable == "my_module:my_filter"
    assert config.log_warnings is False


def test_routing_config_defaults():
    config = RoutingConfig()

    assert config.default is False
    assert config.venues is None


def test_routing_config_explicit():
    config = RoutingConfig(default=True, venues=["BINANCE", "BYBIT"])

    assert config.default is True
    assert config.venues == ["BINANCE", "BYBIT"]


def test_live_data_client_config_defaults():
    config = LiveDataClientConfig()

    assert config.handle_revised_bars is False
    assert isinstance(config.instrument_provider, InstrumentProviderConfig)
    assert isinstance(config.routing, RoutingConfig)


def test_live_data_client_config_explicit():
    ip = InstrumentProviderConfig(load_all=True)
    rc = RoutingConfig(default=True)
    config = LiveDataClientConfig(
        handle_revised_bars=True,
        instrument_provider=ip,
        routing=rc,
    )

    assert config.handle_revised_bars is True
    assert config.instrument_provider.load_all is True
    assert config.routing.default is True


def test_live_data_engine_config_defaults():
    config = LiveDataEngineConfig()

    assert isinstance(config, LiveDataEngineConfig)


def test_live_data_engine_config_explicit():
    config = LiveDataEngineConfig(time_bars_build_with_no_updates=False)

    assert isinstance(config, LiveDataEngineConfig)


def test_live_data_engine_config_accepts_string_interval_type():
    config = LiveDataEngineConfig(time_bars_interval_type="left-open")

    assert isinstance(config, LiveDataEngineConfig)


def test_live_data_engine_config_accepts_right_open_string():
    config = LiveDataEngineConfig(time_bars_interval_type="right-open")

    assert isinstance(config, LiveDataEngineConfig)


def test_live_data_engine_config_rejects_invalid_interval_type():
    with pytest.raises(ValueError, match="time_bars_interval_type"):
        LiveDataEngineConfig(time_bars_interval_type="invalid")


def test_live_data_engine_config_rejects_unsupported_args():
    with pytest.raises(TypeError, match="qsize"):
        LiveDataEngineConfig(qsize=50_000)


def test_live_exec_client_config_defaults():
    config = LiveExecClientConfig()

    assert isinstance(config.instrument_provider, InstrumentProviderConfig)
    assert isinstance(config.routing, RoutingConfig)


def test_live_exec_client_config_explicit():
    ip = InstrumentProviderConfig(load_all=True)
    rc = RoutingConfig(default=True)
    config = LiveExecClientConfig(instrument_provider=ip, routing=rc)

    assert config.instrument_provider.load_all is True
    assert config.routing.default is True


def test_live_exec_engine_config_defaults():
    config = LiveExecEngineConfig()

    assert isinstance(config, LiveExecEngineConfig)


def test_live_exec_engine_config_rejects_unsupported_args():
    with pytest.raises(TypeError, match="snapshot_orders"):
        LiveExecEngineConfig(snapshot_orders=True)

    with pytest.raises(TypeError, match="snapshot_positions"):
        LiveExecEngineConfig(snapshot_positions=True)

    with pytest.raises(TypeError, match="purge_from_database"):
        LiveExecEngineConfig(purge_from_database=True)

    with pytest.raises(TypeError, match="graceful_shutdown_on_error"):
        LiveExecEngineConfig(graceful_shutdown_on_error=True)

    with pytest.raises(TypeError, match="qsize"):
        LiveExecEngineConfig(qsize=1)


def test_live_exec_engine_config_rejects_invalid_reconciliation_instrument_ids():
    with pytest.raises(ValueError, match="reconciliation_instrument_ids"):
        LiveExecEngineConfig(reconciliation_instrument_ids=["INVALID"])


@pytest.mark.parametrize("value", [-1.0, float("nan"), float("inf"), float("-inf")])
def test_live_exec_engine_config_rejects_hostile_startup_delay(value):
    with pytest.raises(ValueError, match="reconciliation_startup_delay_secs"):
        LiveExecEngineConfig(reconciliation_startup_delay_secs=value)


def test_live_node_config_defaults():
    config = LiveNodeConfig()

    assert isinstance(config, LiveNodeConfig)
    assert config.load_state is False
    assert config.save_state is False
    assert config.timeout_connection_secs > 0


def test_live_node_config_accepts_portfolio_config_argument():
    portfolio = PortfolioConfig()
    config = LiveNodeConfig(portfolio=portfolio)

    assert isinstance(config, LiveNodeConfig)


def test_live_risk_engine_config_defaults():
    config = LiveRiskEngineConfig()

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_explicit():
    config = LiveRiskEngineConfig(bypass=True)

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_rejects_unsupported_args():
    with pytest.raises(TypeError, match="qsize"):
        LiveRiskEngineConfig(qsize=25_000)


def test_live_risk_engine_config_rejects_invalid_rate_limit():
    with pytest.raises(ValueError, match="max_order_submit_rate"):
        LiveRiskEngineConfig(max_order_submit_rate="bad-rate")


def test_live_risk_engine_config_rejects_zero_rate_limit_values():
    with pytest.raises(ValueError, match="limit must be greater than zero"):
        LiveRiskEngineConfig(max_order_submit_rate="0/00:00:01")

    with pytest.raises(ValueError, match="interval must be greater than zero"):
        LiveRiskEngineConfig(max_order_modify_rate="100/00:00:00")


def test_live_risk_engine_config_accepts_int_notional_values():
    config = LiveRiskEngineConfig(max_notional_per_order={"ETHUSDT.BINANCE": 100_000})

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_accepts_str_notional_values():
    config = LiveRiskEngineConfig(max_notional_per_order={"ETHUSDT.BINANCE": "100000.50"})

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_rejects_invalid_max_notional_per_order():
    with pytest.raises(ValueError, match="max_notional_per_order"):
        LiveRiskEngineConfig(max_notional_per_order={"INVALID": "1000"})


def test_portfolio_config_defaults():
    config = PortfolioConfig()

    assert config.bar_updates is True


def test_portfolio_config_properties():
    config = PortfolioConfig()

    assert config.convert_to_account_base_currency is True
    assert config.use_mark_prices is False
    assert config.use_mark_xrates is False
    assert config.debug is False
    assert config.min_account_state_logging_interval_ms is None
