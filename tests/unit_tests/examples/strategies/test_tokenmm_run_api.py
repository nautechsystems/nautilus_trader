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

from __future__ import annotations

from argparse import Namespace
from pathlib import Path

import pytest
from flask import Flask

from nautilus_trader.flux.runners.tokenmm.run_api import _attach_fluxboard_tokenmm_routes
from nautilus_trader.flux.runners.tokenmm.run_api import _build_flux_config
from nautilus_trader.flux.runners.tokenmm.run_api import _build_profile_strategy_maps
from nautilus_trader.flux.runners.tokenmm.run_api import _load_config
from nautilus_trader.flux.runners.tokenmm.run_api import _parse_args
from nautilus_trader.flux.runners.tokenmm.run_api import _resolve_bind_host
from nautilus_trader.flux.runners.tokenmm.run_api import _tokenmm_profile_summary


def _example_config_path() -> Path:
    return Path(__file__).resolve().parents[4] / "examples/live/makerv3/config/makerv3.toml"


def test_example_config_builds_flux_config_with_strategy_identity_uniqueness() -> None:
    config = _load_config(_example_config_path())

    flux_config = _build_flux_config(config, mode="paper", confirm_live=True)

    assert flux_config.identity.strategy_instance_id == flux_config.identity.strategy_id


def test_build_flux_config_reads_redis_ssl_flag() -> None:
    flux_config = _build_flux_config(
        {
            "flux": {"mode": "paper"},
            "identity": {"strategy_id": "tokenmm_api"},
            "redis": {"host": "example", "port": 6379, "db": 0, "ssl": True},
            "venues": {},
        },
        mode="paper",
        confirm_live=True,
    )

    assert flux_config.redis.ssl is True


def test_example_config_api_host_is_localhost() -> None:
    config = _load_config(_example_config_path())

    host = _resolve_bind_host(config, Namespace(host=None))

    assert host == "127.0.0.1"


def test_resolve_bind_host_defaults_to_localhost_when_missing() -> None:
    host = _resolve_bind_host({"api": {}}, Namespace(host=None))

    assert host == "127.0.0.1"


def test_resolve_bind_host_prefers_cli_override() -> None:
    host = _resolve_bind_host({"api": {"host": "127.0.0.1"}}, Namespace(host="localhost"))

    assert host == "localhost"


def test_resolve_bind_host_allows_public_bind_targets_for_production_deploys() -> None:
    assert _resolve_bind_host({"api": {"host": "0.0.0.0"}}, Namespace(host=None)) == "0.0.0.0"  # noqa: S104
    assert _resolve_bind_host({"api": {"host": "127.0.0.1"}}, Namespace(host="10.0.0.8")) == "10.0.0.8"  # noqa: S104


def test_build_profile_strategy_maps_reads_tokenmm_allowlist_and_required_subset() -> None:
    strategy_map, required_map = _build_profile_strategy_maps(
        {
            "tokenmm_strategy_ids": ["strategy_a", "strategy_b"],
            "tokenmm_required_strategy_ids": ["strategy_a"],
        },
    )

    assert strategy_map == {"tokenmm": ["strategy_a", "strategy_b"]}
    assert required_map == {"tokenmm": ["strategy_a"]}


def test_build_profile_strategy_maps_requires_non_empty_tokenmm_allowlist() -> None:
    with pytest.raises(ValueError, match="non-empty"):
        _build_profile_strategy_maps({})


def test_build_profile_strategy_maps_rejects_required_ids_outside_allowlist() -> None:
    with pytest.raises(ValueError, match="subset"):
        _build_profile_strategy_maps(
            {
                "tokenmm_strategy_ids": ["strategy_a"],
                "tokenmm_required_strategy_ids": ["strategy_b"],
            },
        )


def test_tokenmm_profile_summary_reports_effective_strategy_sets() -> None:
    summary = _tokenmm_profile_summary(
        {"tokenmm": ["strategy_a", "strategy_b"]},
        {"tokenmm": ["strategy_a"]},
    )

    assert "tokenmm_strategy_count=2" in summary
    assert "tokenmm_strategy_ids=['strategy_a', 'strategy_b']" in summary
    assert "tokenmm_required_strategy_ids=['strategy_a']" in summary


def test_parse_args_requires_explicit_config(monkeypatch) -> None:
    monkeypatch.setattr("sys.argv", ["run_api.py"])

    with pytest.raises(SystemExit, match="2"):
        _parse_args()


def test_attach_fluxboard_tokenmm_routes_redirects_tokenm_aliases(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    dist_dir.mkdir()
    (dist_dir / "index.html").write_text("<html>tokenmm</html>", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_tokenmm_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/tokenm")
    assert response.status_code == 302
    assert response.headers["Location"] == "/tokenmm"

    response = client.get("/tokenm/alerts?foo=1")
    assert response.status_code == 302
    assert response.headers["Location"] == "/tokenmm/alerts?foo=1"


def test_load_config_applies_redis_env_overrides(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "tokenmm.toml"
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
    monkeypatch.setenv("TOKENMM_REDIS_HOST", "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com")
    monkeypatch.setenv("TOKENMM_REDIS_PORT", "6379")
    monkeypatch.setenv("TOKENMM_REDIS_USERNAME", "default")
    monkeypatch.setenv("TOKENMM_REDIS_PASSWORD", "secret")
    monkeypatch.setenv("TOKENMM_REDIS_SSL", "true")

    config = _load_config(config_path)

    assert config["redis"]["host"] == "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com"
    assert config["redis"]["port"] == 6379
    assert config["redis"]["username"] == "default"
    assert config["redis"]["password"] == "secret"
    assert config["redis"]["ssl"] is True
