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
Tests for FinFeed configuration and LiveNode factory extraction.
"""

import os
import subprocess
import sys

import pytest

from nautilus_trader.adapters.finfeed import FINFEED
from nautilus_trader.adapters.finfeed import FINFEED_CLIENT_ID
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import TraderId


def _build_node(config: FinFeedDataClientConfig) -> LiveNode:
    return (
        LiveNode.builder("FINFEED-PYTEST-001", TraderId("TESTER-001"), Environment.LIVE)
        .add_data_client(None, FinFeedDataClientFactory(), config)
        .build()
    )


def _selection() -> FinFeedInstrumentSelection:
    return FinFeedInstrumentSelection("POLYMARKET", ["market_YES"])


def test_constants_and_factory() -> None:
    """
    Expose provider identity and the registered factory name.
    """
    assert FINFEED == "FINFEED"
    assert ClientId("FINFEED") == FINFEED_CLIENT_ID
    assert FinFeedDataClientFactory().name() == FINFEED


def test_config_defaults() -> None:
    """
    Expose every default without resolving credentials.
    """
    config = FinFeedDataClientConfig()
    expected = {
        "base_url_http": "https://api.prediction-markets.finfeedapi.com/v1/",
        "http_timeout_secs": 30,
        "max_concurrent_requests": 1,
        "attempt_budget": 100,
        "min_request_interval_ms": 10_000,
        "max_retries": 3,
        "retry_delay_initial_ms": 1_000,
        "retry_delay_max_ms": 10_000,
        "retry_backoff_factor": 2.0,
        "retry_jitter_ms": 0,
        "retry_operation_timeout_ms": 30_000,
        "retry_immediate_first": False,
        "retry_max_elapsed_ms": 120_000,
        "poll_interval_ms": 10_000,
        "poll_limit": 100,
        "connect_timeout_secs": 120,
        "instruments": [],
        "has_api_key": False,
        "has_proxy_url": False,
    }
    assert {name: getattr(config, name) for name in expected} == expected


def test_config_distinct_values_and_secret_redaction() -> None:
    """
    Preserve distinct controls and hide secret values.
    """
    expected = {
        "base_url_http": "http://localhost:8123/v1/",
        "http_timeout_secs": 13,
        "max_concurrent_requests": 4,
        "attempt_budget": 17,
        "min_request_interval_ms": 19,
        "max_retries": 2,
        "retry_delay_initial_ms": 29,
        "retry_delay_max_ms": 53,
        "retry_backoff_factor": 1.5,
        "retry_jitter_ms": 7,
        "retry_operation_timeout_ms": 31_000,
        "retry_immediate_first": True,
        "retry_max_elapsed_ms": 71_000,
        "poll_interval_ms": 137,
        "poll_limit": 23,
        "connect_timeout_secs": 43,
    }
    config = FinFeedDataClientConfig(
        api_key="projection-secret",
        proxy_url="http://user:proxy-secret@localhost:8124",
        instruments=[_selection()],
        **expected,
    )
    assert {name: getattr(config, name) for name in expected} == expected
    assert config.has_api_key is True
    assert config.has_proxy_url is True
    assert not hasattr(config, "api_key")
    assert not hasattr(config, "proxy_url")
    assert repr(config) == "FinFeedDataClientConfig"
    assert str(config) == "FinFeedDataClientConfig"
    [loaded] = config.instruments
    assert loaded.exchange_id == "POLYMARKET"
    assert loaded.market_ids == ["market_YES"]
    market_ids = loaded.market_ids
    market_ids.append("mutation_NO")
    assert market_ids == ["market_YES", "mutation_NO"]
    assert loaded.market_ids == ["market_YES"]
    assert config.instruments[0].market_ids == ["market_YES"]


def test_factory_extracts_config_and_builds_without_network() -> None:
    """
    Build the disconnected client through registry extraction.
    """
    node = _build_node(FinFeedDataClientConfig(api_key="test-key", instruments=[_selection()]))
    assert node.trader_id == TraderId("TESTER-001")
    assert node.environment == Environment.LIVE
    node.dispose()


@pytest.mark.parametrize("limit", [1_000, 1_001, 10_000])
def test_polling_capacity_reaches_client_construction(limit: int) -> None:
    """
    Allow the provider's activity capacity through Python configuration.
    """
    config = FinFeedDataClientConfig(
        api_key="test-key",
        instruments=[_selection()],
        poll_limit=limit,
    )
    assert config.poll_limit == limit
    node = _build_node(config)
    assert node.trader_id == TraderId("TESTER-001")
    node.dispose()


@pytest.mark.parametrize(
    ("values", "message"),
    [
        ({"instruments": []}, "instrument selection is empty"),
        (
            {"instruments": [FinFeedInstrumentSelection("UNKNOWN", ["market_YES"])]},
            "unknown exchange",
        ),
        ({"poll_interval_ms": 99}, "invalid FinFeed polling controls"),
        ({"poll_limit": 0}, "invalid FinFeed polling controls"),
        ({"poll_limit": 10_001}, "invalid FinFeed polling controls"),
        ({"poll_limit": 4_294_967_295}, "invalid FinFeed polling controls"),
        ({"connect_timeout_secs": 0}, "invalid FinFeed connect deadline"),
        ({"http_timeout_secs": 0}, "request controls must be positive"),
        ({"max_concurrent_requests": 0}, "request controls must be positive"),
        ({"attempt_budget": 0}, "request controls must be positive"),
        ({"min_request_interval_ms": 0}, "request controls must be positive"),
        ({"retry_operation_timeout_ms": 0}, "request controls must be positive"),
        ({"retry_max_elapsed_ms": 0}, "request controls must be positive"),
        ({"max_retries": 4_294_967_295}, "request controls must be positive"),
        ({"retry_delay_max_ms": 86_400_001}, "request controls must be positive"),
        ({"retry_delay_initial_ms": 0}, "invalid retry backoff"),
        ({"retry_backoff_factor": 0.0}, "invalid retry backoff"),
        ({"base_url_http": "http://example.com/"}, "base URL must be HTTPS"),
        ({"proxy_url": "bad proxy"}, "invalid proxy URL"),
    ],
)
def test_invalid_values_fail_at_client_construction(values: dict, message: str) -> None:
    """
    Reject semantic errors when the factory builds the client.
    """
    kwargs = {"api_key": "test-key", "instruments": [_selection()]} | values
    config = FinFeedDataClientConfig(**kwargs)
    with pytest.raises(RuntimeError, match=message):
        _build_node(config)


@pytest.mark.parametrize(
    "values",
    [{"unknown": 1}, {"poll_limit": "bad"}, {"instruments": ["bad"]}],
)
def test_unknown_keywords_and_wrong_types_fail_at_config_constructor(values: dict) -> None:
    """
    Reject unknown keywords and incompatible Python types.
    """
    with pytest.raises(TypeError):
        FinFeedDataClientConfig(**values)


def test_credentials_resolve_only_at_client_construction(monkeypatch: pytest.MonkeyPatch) -> None:
    """
    Resolve the environment key when building the client.
    """
    monkeypatch.delenv("FINFEED_API_KEY", raising=False)
    config = FinFeedDataClientConfig(instruments=[_selection()])
    assert config.has_api_key is False
    with pytest.raises(RuntimeError, match="FINFEED_API_KEY"):
        _build_node(config)
    monkeypatch.setenv("FINFEED_API_KEY", "test-env-key")
    config_with_env = FinFeedDataClientConfig(instruments=[_selection()])
    assert config_with_env.has_api_key is False
    node = _build_node(config)
    assert node.trader_id == TraderId("TESTER-001")
    node.dispose()


def test_package_import_without_credentials() -> None:
    """
    Import the package without any credentials.
    """
    env = {
        name: value
        for name, value in os.environ.items()
        if name not in {"FINFEED_API_KEY", "COINAPI_API_KEY"}
    }
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "from nautilus_trader.adapters.finfeed import *; assert FinFeedDataClientConfig().has_api_key is False",
        ],
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=20,
        check=False,
    )
    assert result.returncode == 0, result.stderr
