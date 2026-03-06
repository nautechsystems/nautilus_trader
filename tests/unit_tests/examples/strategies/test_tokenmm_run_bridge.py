from __future__ import annotations

from argparse import Namespace
from pathlib import Path

import pytest

from flux.runners.tokenmm.run_bridge import _load_config
from flux.runners.tokenmm.run_bridge import _parse_args
from flux.runners.tokenmm.run_bridge import _resolve_strategy_scope


def test_resolve_strategy_scope_prefers_cli_strategy_id() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id="cli_strategy", all_strategies=False)

    resolved = _resolve_strategy_scope(config, args)

    assert resolved == "cli_strategy"


def test_resolve_strategy_scope_uses_config_strategy_id_when_cli_missing() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=None, all_strategies=False)

    resolved = _resolve_strategy_scope(config, args)

    assert resolved == "config_strategy"


def test_resolve_strategy_scope_requires_strategy_id_without_all_strategies() -> None:
    config: dict[str, dict[str, str]] = {"identity": {}}
    args = Namespace(strategy_id=None, all_strategies=False)

    with pytest.raises(ValueError, match="strategy_id"):
        _resolve_strategy_scope(config, args)


def test_resolve_strategy_scope_rejects_strategy_id_with_all_strategies() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id="cli_strategy", all_strategies=True)

    with pytest.raises(ValueError, match="all-strategies"):
        _resolve_strategy_scope(config, args)


def test_resolve_strategy_scope_all_strategies_returns_none() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=None, all_strategies=True)

    resolved = _resolve_strategy_scope(config, args)

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
        "TOKENMM_REDIS_HOST",
        "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com",
    )
    monkeypatch.setenv("TOKENMM_REDIS_PORT", "6379")
    monkeypatch.setenv("TOKENMM_REDIS_SSL", "true")

    config = _load_config(config_path)

    assert (
        config["redis"]["host"]
        == "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com"
    )
    assert config["redis"]["port"] == 6379
    assert config["redis"]["ssl"] is True
