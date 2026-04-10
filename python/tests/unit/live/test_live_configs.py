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
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.live import PortfolioConfig
from nautilus_trader.live import RoutingConfig


def test_instrument_provider_config_defaults():
    config = InstrumentProviderConfig()

    assert config.load_all is False
    assert config.load_ids is True
    assert config.filters == {}


def test_instrument_provider_config_explicit():
    config = InstrumentProviderConfig(
        load_all=True,
        load_ids=False,
        filters={"exchange": "BINANCE"},
    )

    assert config.load_all is True
    assert config.load_ids is False
    assert config.filters == {"exchange": "BINANCE"}


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
    with pytest.raises(TypeError, match="snapshot_positions"):
        LiveExecEngineConfig(snapshot_positions=True)

    with pytest.raises(TypeError, match="purge_from_database"):
        LiveExecEngineConfig(purge_from_database=True)

    with pytest.raises(TypeError, match="graceful_shutdown_on_error"):
        LiveExecEngineConfig(graceful_shutdown_on_error=True)

    with pytest.raises(TypeError, match="qsize"):
        LiveExecEngineConfig(qsize=1)


def test_live_risk_engine_config_defaults():
    config = LiveRiskEngineConfig()

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_explicit():
    config = LiveRiskEngineConfig(bypass=True)

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_rejects_unsupported_args():
    with pytest.raises(TypeError, match="qsize"):
        LiveRiskEngineConfig(qsize=25_000)


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
