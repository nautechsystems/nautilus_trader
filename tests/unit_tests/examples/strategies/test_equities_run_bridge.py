from __future__ import annotations

from argparse import Namespace
from pathlib import Path

import pytest

from flux.runners.equities.run_bridge import FULL_TO_SUFFIX_TOPICS
from flux.runners.equities.run_bridge import _build_handlers
from flux.runners.equities.run_bridge import _load_config
from flux.runners.equities.run_bridge import _parse_args
from flux.runners.equities.run_bridge import _resolve_strategy_ids
from nautilus_trader.flux.events import TOPIC_EXECUTION_ALERT


def test_resolve_strategy_ids_prefers_cli_strategy_ids() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=["cli_strategy", "cli_strategy_2"], all_strategies=False)

    resolved = _resolve_strategy_ids(config, args)

    assert resolved == ["cli_strategy", "cli_strategy_2"]


def test_resolve_strategy_ids_uses_api_equities_allowlist_when_cli_missing() -> None:
    config = {
        "identity": {"strategy_id": "config_strategy"},
        "api": {"equities_strategy_ids": ["strategy_a", "strategy_b"]},
    }
    args = Namespace(strategy_id=None, all_strategies=False)

    resolved = _resolve_strategy_ids(config, args)

    assert resolved == ["strategy_a", "strategy_b"]


def test_resolve_strategy_ids_requires_explicit_equities_allowlist_when_cli_missing() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=None, all_strategies=False)

    with pytest.raises(ValueError, match="api.equities_strategy_ids"):
        _resolve_strategy_ids(config, args)


def test_resolve_strategy_ids_requires_strategy_id_without_all_strategies() -> None:
    config: dict[str, dict[str, str]] = {"identity": {}}
    args = Namespace(strategy_id=None, all_strategies=False)

    with pytest.raises(ValueError, match="strategy_id"):
        _resolve_strategy_ids(config, args)


def test_resolve_strategy_ids_rejects_strategy_id_with_all_strategies() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=["cli_strategy"], all_strategies=True)

    with pytest.raises(ValueError, match="all-strategies"):
        _resolve_strategy_ids(config, args)


def test_resolve_strategy_ids_all_strategies_returns_none() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=None, all_strategies=True)

    resolved = _resolve_strategy_ids(config, args)

    assert resolved is None


def test_parse_args_requires_explicit_config(monkeypatch) -> None:
    monkeypatch.setattr("sys.argv", ["run_bridge.py"])

    with pytest.raises(SystemExit, match="2"):
        _parse_args()


def test_load_config_applies_redis_env_overrides(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "bridge.toml"
    config_path.write_text(
        """
[redis]
host = "127.0.0.1"
port = 6380
db = 0
ssl = false
""".strip(),
        encoding="utf-8",
    )
    monkeypatch.setenv(
        "EQUITIES_REDIS_HOST",
        "master.equities-redis-prod.wapqos.apse1.cache.amazonaws.com",
    )
    monkeypatch.setenv("EQUITIES_REDIS_PORT", "6379")
    monkeypatch.setenv("EQUITIES_REDIS_SSL", "true")

    config = _load_config(config_path)

    assert config["redis"]["host"] == "master.equities-redis-prod.wapqos.apse1.cache.amazonaws.com"
    assert config["redis"]["port"] == 6379
    assert config["redis"]["ssl"] is True


def test_build_handlers_routes_execution_alert_topic_to_alert_handler() -> None:
    handlers = _build_handlers()

    assert FULL_TO_SUFFIX_TOPICS[TOPIC_EXECUTION_ALERT] == "alert"
    assert handlers[TOPIC_EXECUTION_ALERT] is handlers["alert"]
