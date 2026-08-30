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
Test live configs behavior.
"""

import re

import pytest

from nautilus_trader.common import CacheConfig
from nautilus_trader.common import LoggerConfig
from nautilus_trader.common import MessageBusConfig
from nautilus_trader.core import UUID4
from nautilus_trader.live import DataClientConfig
from nautilus_trader.live import ExecutionClientConfig
from nautilus_trader.live import InstrumentProviderConfig
from nautilus_trader.live import LiveDataEngineConfig
from nautilus_trader.live import LiveExecutionEngineConfig
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.live import PluginConfig
from nautilus_trader.live import PortfolioConfig
from nautilus_trader.live import QueueMonitorConfig
from nautilus_trader.live import RoutingConfig
from nautilus_trader.model import BarIntervalType
from nautilus_trader.model import ClientId
from nautilus_trader.model import Venue


def test_instrument_provider_config_defaults() -> None:
    """
    Test instrument provider config defaults.
    """
    config = InstrumentProviderConfig()

    assert config.load_all is False
    assert config.load_ids is None
    assert config.filters == {}
    assert config.filter_callable is None
    assert config.log_warnings is True


def test_instrument_provider_config_explicit() -> None:
    """
    Test instrument provider config explicit.
    """
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


def test_routing_config_defaults() -> None:
    """
    Test routing config defaults.
    """
    config = RoutingConfig()

    assert config.default is False
    assert config.venues is None


def test_routing_config_explicit() -> None:
    """
    Test routing config explicit.
    """
    config = RoutingConfig(default=True, venues=["BINANCE", "BYBIT"])

    assert config.default is True
    assert config.venues == ["BINANCE", "BYBIT"]


def test_data_client_config_defaults() -> None:
    """
    Test data client config defaults.
    """
    config = DataClientConfig()

    assert config.handle_revised_bars is False
    assert isinstance(config.instrument_provider, InstrumentProviderConfig)
    assert isinstance(config.routing, RoutingConfig)


def test_plugin_config_explicit() -> None:
    """
    Test plugin config explicit.
    """
    config = PluginConfig(
        path="./target/debug/examples/libruntime_smoke_plugin.so",
        type_name="RuntimeSmokeActor",
        config={
            "actor_id": "RuntimeSmokeActor-001",
            "threshold": 10,
            "strategy_config": {"strategy_id": "RuntimeSmokeStrategy-001"},
        },
        sha256="0" * 64,
    )

    assert config.path == "./target/debug/examples/libruntime_smoke_plugin.so"
    assert config.type_name == "RuntimeSmokeActor"
    assert config.config == {
        "actor_id": "RuntimeSmokeActor-001",
        "threshold": 10,
        "strategy_config": {"strategy_id": "RuntimeSmokeStrategy-001"},
    }
    assert config.sha256 == "0" * 64


def test_queue_monitor_config_explicit() -> None:
    """
    Test queue monitor config explicit.
    """
    config = QueueMonitorConfig(
        queue_depth_trigger=1_000,
        queue_depth_clear=500,
        mean_dispatch_ns_trigger=250_000,
        mean_dispatch_ns_clear=150_000,
    )

    assert config.queue_depth_trigger == 1_000
    assert config.queue_depth_clear == 500
    assert config.mean_dispatch_ns_trigger == 250_000
    assert config.mean_dispatch_ns_clear == 150_000


def test_data_client_config_explicit() -> None:
    """
    Test data client config explicit.
    """
    ip = InstrumentProviderConfig(load_all=True)
    rc = RoutingConfig(default=True)
    config = DataClientConfig(
        handle_revised_bars=True,
        instrument_provider=ip,
        routing=rc,
    )

    assert config.handle_revised_bars is True
    assert config.instrument_provider.load_all is True
    assert config.routing.default is True


def test_live_data_engine_config_defaults() -> None:
    """
    Test live data engine config defaults.
    """
    config = LiveDataEngineConfig()

    assert isinstance(config, LiveDataEngineConfig)


def test_live_data_engine_config_explicit() -> None:
    """
    Test live data engine config explicit.
    """
    client_id = ClientId("DATA-001")
    config = LiveDataEngineConfig(
        time_bars_build_with_no_updates=False,
        time_bars_timestamp_on_close=False,
        time_bars_skip_first_non_full_bar=True,
        time_bars_interval_type=BarIntervalType.RIGHT_OPEN,
        time_bars_build_delay=1,
        time_bars_origin_offset={"minute": 2},
        validate_data_sequence=True,
        buffer_deltas=True,
        emit_quotes_from_book=True,
        emit_quotes_from_book_depths=True,
        external_clients=[client_id],
        debug=True,
    )

    assert config.time_bars_build_with_no_updates is False
    assert config.time_bars_timestamp_on_close is False
    assert config.time_bars_skip_first_non_full_bar is True
    assert config.time_bars_interval_type == BarIntervalType.RIGHT_OPEN
    assert config.time_bars_build_delay == 1
    assert config.time_bars_origin_offset == {"minute": 2}
    assert config.validate_data_sequence is True
    assert config.buffer_deltas is True
    assert config.emit_quotes_from_book is True
    assert config.emit_quotes_from_book_depths is True
    assert config.external_clients == [client_id]
    assert config.debug is True


def test_live_data_engine_config_accepts_string_interval_type() -> None:
    """
    Test live data engine config accepts string interval type.
    """
    config = LiveDataEngineConfig(time_bars_interval_type="left-open")

    assert isinstance(config, LiveDataEngineConfig)


def test_live_data_engine_config_accepts_right_open_string() -> None:
    """
    Test live data engine config accepts right open string.
    """
    config = LiveDataEngineConfig(time_bars_interval_type="right-open")

    assert isinstance(config, LiveDataEngineConfig)


def test_live_data_engine_config_rejects_invalid_interval_type() -> None:
    """
    Test live data engine config rejects invalid interval type.
    """
    with pytest.raises(ValueError, match="time_bars_interval_type"):
        LiveDataEngineConfig(time_bars_interval_type="invalid")


def test_live_data_engine_config_rejects_unsupported_args() -> None:
    """
    Test live data engine config rejects unsupported args.
    """
    with pytest.raises(TypeError, match="qsize"):
        LiveDataEngineConfig(qsize=50_000)


def test_execution_client_config_defaults() -> None:
    """
    Test execution client config defaults.
    """
    config = ExecutionClientConfig()

    assert isinstance(config.instrument_provider, InstrumentProviderConfig)
    assert isinstance(config.routing, RoutingConfig)


def test_execution_client_config_explicit() -> None:
    """
    Test execution client config explicit.
    """
    ip = InstrumentProviderConfig(load_all=True)
    rc = RoutingConfig(default=True)
    config = ExecutionClientConfig(instrument_provider=ip, routing=rc)

    assert config.instrument_provider.load_all is True
    assert config.routing.default is True


def test_live_exec_engine_config_defaults() -> None:
    """
    Test live exec engine config defaults.
    """
    config = LiveExecutionEngineConfig()

    assert isinstance(config, LiveExecutionEngineConfig)
    assert config.snapshot_positions is False


def test_live_exec_engine_config_readback() -> None:
    """
    Test live exec engine config readback.
    """
    client_id = ClientId("EXEC-001")
    config = LiveExecutionEngineConfig(
        load_cache=False,
        manage_own_order_books=True,
        snapshot_positions_interval_secs=1.5,
        external_clients=[client_id],
        allow_overfills=True,
        reconciliation=False,
        reconciliation_startup_delay_secs=2.5,
        reconciliation_lookback_mins=3,
        reconciliation_instrument_ids=["BTCUSDT.BINANCE"],
        filter_unclaimed_external_orders=True,
        filter_position_reports=True,
        filtered_client_order_ids=["O-001"],
        generate_missing_orders=False,
        inflight_check_interval_ms=4,
        inflight_check_threshold_ms=5,
        inflight_check_retries=6,
        open_check_interval_secs=7.5,
        open_check_lookback_mins=8,
        open_check_threshold_ms=9,
        open_check_missing_retries=10,
        open_check_open_only=False,
        max_single_order_queries_per_cycle=11,
        single_order_query_delay_ms=12,
        position_check_interval_secs=13.5,
        position_check_lookback_mins=14,
        position_check_threshold_ms=15,
        position_check_retries=16,
        purge_closed_orders_interval_mins=17,
        purge_closed_orders_buffer_mins=18,
        purge_closed_positions_interval_mins=19,
        purge_closed_positions_buffer_mins=20,
        purge_account_events_interval_mins=21,
        purge_account_events_lookback_mins=22,
        own_books_audit_interval_secs=23.5,
        debug=True,
        snapshot_positions=True,
    )

    assert config.load_cache is False
    assert config.manage_own_order_books is True
    assert config.snapshot_positions_interval_secs == 1.5
    assert config.external_clients == [client_id]
    assert config.allow_overfills is True
    assert config.reconciliation is False
    assert config.reconciliation_startup_delay_secs == 2.5
    assert config.reconciliation_lookback_mins == 3
    assert config.reconciliation_instrument_ids == ["BTCUSDT.BINANCE"]
    assert config.filter_unclaimed_external_orders is True
    assert config.filter_position_reports is True
    assert config.filtered_client_order_ids == ["O-001"]
    assert config.generate_missing_orders is False
    assert config.inflight_check_interval_ms == 4
    assert config.inflight_check_threshold_ms == 5
    assert config.inflight_check_retries == 6
    assert config.open_check_interval_secs == 7.5
    assert config.open_check_lookback_mins == 8
    assert config.open_check_threshold_ms == 9
    assert config.open_check_missing_retries == 10
    assert config.open_check_open_only is False
    assert config.max_single_order_queries_per_cycle == 11
    assert config.single_order_query_delay_ms == 12
    assert config.position_check_interval_secs == 13.5
    assert config.position_check_lookback_mins == 14
    assert config.position_check_threshold_ms == 15
    assert config.position_check_retries == 16
    assert config.purge_closed_orders_interval_mins == 17
    assert config.purge_closed_orders_buffer_mins == 18
    assert config.purge_closed_positions_interval_mins == 19
    assert config.purge_closed_positions_buffer_mins == 20
    assert config.purge_account_events_interval_mins == 21
    assert config.purge_account_events_lookback_mins == 22
    assert config.own_books_audit_interval_secs == 23.5
    assert config.debug is True
    assert config.snapshot_positions is True


def test_live_exec_engine_config_rejects_unsupported_args() -> None:
    """
    Test live exec engine config rejects unsupported args.
    """
    with pytest.raises(TypeError, match="snapshot_orders"):
        LiveExecutionEngineConfig(snapshot_orders=True)

    with pytest.raises(TypeError, match="purge_from_database"):
        LiveExecutionEngineConfig(purge_from_database=True)

    with pytest.raises(TypeError, match="qsize"):
        LiveExecutionEngineConfig(qsize=1)


def test_live_exec_engine_config_rejects_invalid_reconciliation_instrument_ids() -> None:
    """
    Test live exec engine config rejects invalid reconciliation instrument ids.
    """
    expected_err = (
        "invalid LiveExecutionEngineConfig.reconciliation_instrument_ids[0] reference instrument ID: "
        "invalid `InstrumentId` value 'INVALID': "
        "missing '.' separator between symbol and venue components"
    )

    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        LiveExecutionEngineConfig(reconciliation_instrument_ids=["INVALID"])

    assert str(exc_info.value) == expected_err


@pytest.mark.parametrize("value", [-1.0, float("nan"), float("inf"), float("-inf")])
def test_live_exec_engine_config_rejects_hostile_startup_delay(value: object) -> None:
    """
    Test live exec engine config rejects hostile startup delay.
    """
    with pytest.raises(ValueError, match="reconciliation_startup_delay_secs"):
        LiveExecutionEngineConfig(reconciliation_startup_delay_secs=value)


@pytest.mark.parametrize(
    "field",
    [
        "snapshot_positions_interval_secs",
        "open_check_interval_secs",
        "position_check_interval_secs",
        "own_books_audit_interval_secs",
    ],
)
@pytest.mark.parametrize(
    "value",
    [0.0, 0.5e-9, -1.0, float("nan"), float("inf"), float("-inf"), 1e300],
)
def test_live_exec_engine_config_rejects_invalid_intervals(field: object, value: object) -> None:
    """
    Test live exec engine config rejects invalid intervals.
    """
    with pytest.raises(ValueError, match=field):
        LiveExecutionEngineConfig(**{field: value})


def test_live_node_config_defaults() -> None:
    """
    Test live node config defaults.
    """
    config = LiveNodeConfig()

    assert isinstance(config, LiveNodeConfig)
    assert config.load_state is False
    assert config.save_state is False
    assert config.shutdown_on_error is False
    assert config.timeout_connection_secs == 60.0
    assert config.queue_monitor is None


def test_live_node_config_accepts_queue_monitor_config_argument() -> None:
    """
    Test live node config accepts queue monitor config argument.
    """
    queue_monitor = QueueMonitorConfig(
        queue_depth_trigger=1_000,
        queue_depth_clear=500,
        mean_dispatch_ns_trigger=250_000,
        mean_dispatch_ns_clear=150_000,
    )
    config = LiveNodeConfig(queue_monitor=queue_monitor)

    resolved = config.queue_monitor
    assert isinstance(resolved, QueueMonitorConfig)
    assert resolved.queue_depth_trigger == 1_000
    assert resolved.queue_depth_clear == 500
    assert resolved.mean_dispatch_ns_trigger == 250_000
    assert resolved.mean_dispatch_ns_clear == 150_000


def test_live_node_config_rejects_invalid_timeout_duration() -> None:
    """
    Test live node config rejects invalid timeout duration.
    """
    expected_err = (
        "invalid timeout_connection_secs: -1 (must be finite, non-negative, and <= 86400)"
    )

    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        LiveNodeConfig(timeout_connection_secs=-1.0)

    assert str(exc_info.value) == expected_err


def test_live_node_config_accepts_portfolio_config_argument() -> None:
    """
    Test live node config accepts portfolio config argument.
    """
    portfolio = PortfolioConfig()
    cache = CacheConfig()
    msgbus = MessageBusConfig()
    logging = LoggerConfig(print_config=True)
    instance_id = UUID4()
    data_engine = LiveDataEngineConfig(debug=True)
    risk_engine = LiveRiskEngineConfig(bypass=True)
    exec_engine = LiveExecutionEngineConfig(load_cache=False)
    config = LiveNodeConfig(
        logging=logging,
        instance_id=instance_id,
        cache=cache,
        msgbus=msgbus,
        portfolio=portfolio,
        loop_debug=True,
        data_engine=data_engine,
        risk_engine=risk_engine,
        exec_engine=exec_engine,
    )

    assert config.logging.print_config is True
    assert config.instance_id == instance_id
    assert isinstance(config.cache, CacheConfig)
    assert isinstance(config.msgbus, MessageBusConfig)
    assert isinstance(config.portfolio, PortfolioConfig)
    assert config.loop_debug is True
    assert config.data_engine.debug is True
    assert config.risk_engine.bypass is True
    assert config.exec_engine.load_cache is False


def test_live_risk_engine_config_defaults() -> None:
    """
    Test live risk engine config defaults.
    """
    config = LiveRiskEngineConfig()

    assert isinstance(config, LiveRiskEngineConfig)
    assert config.full_position_exit_venues == []


def test_live_risk_engine_config_explicit() -> None:
    """
    Test live risk engine config explicit.
    """
    config = LiveRiskEngineConfig(
        bypass=True,
        max_order_submit_rate="10/00:00:01",
        max_order_modify_rate="20/00:00:02",
        max_notional_per_order={"BTCUSDT.BINANCE": 100_000},
        full_position_exit_venues=[Venue("BINANCE")],
        debug=True,
    )

    assert config.bypass is True
    assert config.max_order_submit_rate == "10/00:00:01"
    assert config.max_order_modify_rate == "20/00:00:02"
    assert config.max_notional_per_order == {"BTCUSDT.BINANCE": "100000"}
    assert config.full_position_exit_venues == [Venue("BINANCE")]
    assert config.debug is True


def test_live_risk_engine_config_rejects_unsupported_args() -> None:
    """
    Test live risk engine config rejects unsupported args.
    """
    with pytest.raises(TypeError, match="qsize"):
        LiveRiskEngineConfig(qsize=25_000)


def test_live_risk_engine_config_rejects_invalid_rate_limit() -> None:
    """
    Test live risk engine config rejects invalid rate limit.
    """
    expected_err = "invalid LiveRiskEngineConfig.max_order_submit_rate: expected 'limit/HH:MM:SS'"

    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        LiveRiskEngineConfig(max_order_submit_rate="bad-rate")

    assert str(exc_info.value) == expected_err


def test_live_risk_engine_config_rejects_zero_rate_limit_values() -> None:
    """
    Test live risk engine config rejects zero rate limit values.
    """
    with pytest.raises(ValueError, match="Invalid limit: 0"):
        LiveRiskEngineConfig(max_order_submit_rate="0/00:00:01")

    with pytest.raises(ValueError, match="Invalid interval_ns: 0"):
        LiveRiskEngineConfig(max_order_modify_rate="100/00:00:00")


def test_live_risk_engine_config_accepts_int_notional_values() -> None:
    """
    Test live risk engine config accepts int notional values.
    """
    config = LiveRiskEngineConfig(max_notional_per_order={"ETHUSDT.BINANCE": 100_000})

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_accepts_str_notional_values() -> None:
    """
    Test live risk engine config accepts str notional values.
    """
    config = LiveRiskEngineConfig(max_notional_per_order={"ETHUSDT.BINANCE": "100000.50"})

    assert isinstance(config, LiveRiskEngineConfig)


def test_live_risk_engine_config_rejects_invalid_max_notional_per_order() -> None:
    """
    Test live risk engine config rejects invalid max notional per order.
    """
    expected_err = (
        "invalid LiveRiskEngineConfig.max_notional_per_order[INVALID] reference instrument ID: "
        "invalid `InstrumentId` value 'INVALID': "
        "missing '.' separator between symbol and venue components"
    )

    with pytest.raises(ValueError, match=re.escape(expected_err)) as exc_info:
        LiveRiskEngineConfig(max_notional_per_order={"INVALID": "1000"})

    assert str(exc_info.value) == expected_err


def test_portfolio_config_defaults() -> None:
    """
    Test portfolio config defaults.
    """
    config = PortfolioConfig()

    assert config.bar_updates is True


def test_portfolio_config_properties() -> None:
    """
    Test portfolio config properties.
    """
    config = PortfolioConfig()

    assert config.convert_to_account_base_currency is True
    assert config.use_mark_prices is True
    assert config.use_mark_xrates is False
    assert config.debug is False
    assert config.min_account_state_logging_interval_ms is None
