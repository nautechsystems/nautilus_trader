from __future__ import annotations

from argparse import Namespace
from pathlib import Path

import pytest
from flask import Flask

from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.equities.run_api import _attach_fluxboard_equities_routes
from flux.runners.equities.run_api import _build_profile_strategy_maps
from flux.runners.equities.run_api import _equities_profile_summary
from flux.runners.equities.run_api import _load_config
from flux.runners.equities.run_api import _parse_args
from flux.runners.equities.run_api import _resolve_strategy_name
from flux.runners.equities.run_api import build_strategy_metadata_for_test


def test_build_profile_strategy_maps_reads_equities_allowlist_and_required_subset() -> None:
    strategy_map, required_map = _build_profile_strategy_maps(
        {
            "equities_strategy_ids": ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"],
            "equities_required_strategy_ids": ["aapl_tradexyz_makerv3"],
        },
    )

    assert strategy_map == {
        "equities": ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"],
    }
    assert required_map == {"equities": ["aapl_tradexyz_makerv3"]}


def test_equities_descriptor_exposes_stable_profile_contract() -> None:
    descriptor = get_strategy_set_descriptor("equities")

    assert descriptor.profile == "equities"
    assert descriptor.aliases == ("equities",)
    assert descriptor.base_path == "/equities"
    assert descriptor.route_aliases == ()
    assert descriptor.strategy_ids_field == "equities_strategy_ids"
    assert descriptor.required_strategy_ids_field == "equities_required_strategy_ids"


def test_build_profile_strategy_maps_requires_non_empty_equities_allowlist() -> None:
    with pytest.raises(ValueError, match="non-empty"):
        _build_profile_strategy_maps({})


def test_build_profile_strategy_maps_rejects_required_ids_outside_allowlist() -> None:
    with pytest.raises(ValueError, match="subset"):
        _build_profile_strategy_maps(
            {
                "equities_strategy_ids": ["aapl_tradexyz_makerv3"],
                "equities_required_strategy_ids": ["msft_tradexyz_makerv3"],
            },
        )


def test_equities_profile_summary_reports_effective_strategy_sets() -> None:
    summary = _equities_profile_summary(
        {"equities": ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"]},
        {"equities": ["aapl_tradexyz_makerv3"]},
    )

    assert "equities_strategy_count=2" in summary
    assert "equities_strategy_ids=['aapl_tradexyz_makerv3', 'msft_tradexyz_makerv3']" in summary
    assert "equities_required_strategy_ids=['aapl_tradexyz_makerv3']" in summary


def test_equities_run_api_uses_makerv4_metadata_when_strategy_spec_is_makerv4() -> None:
    metadata = build_strategy_metadata_for_test("makerv4")

    assert metadata.strategy_class == "maker_v4"
    assert metadata.param_set == "makerv4"
    assert metadata.strategy_family == "maker_v4"
    assert metadata.strategy_version == "v4"


def test_parse_args_requires_explicit_config(monkeypatch) -> None:
    monkeypatch.setattr("sys.argv", ["run_api.py"])

    with pytest.raises(SystemExit, match="2"):
        _parse_args()


def test_attach_fluxboard_equities_routes_serves_shared_static_assets_and_spa_fallback(
    tmp_path: Path,
) -> None:
    dist_dir = tmp_path / "dist"
    assets_dir = dist_dir / "assets"
    assets_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>equities</html>", encoding="utf-8")
    (assets_dir / "app.js").write_text("console.log(equities)", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_equities_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/equities")
    assert response.status_code == 200
    assert "equities" in response.get_data(as_text=True)

    response = client.get("/static/fluxboard/assets/app.js")
    assert response.status_code == 200
    assert "console.log(equities)" in response.get_data(as_text=True)

    response = client.get("/equities/assets/app.js")
    assert response.status_code == 404

    response = client.get("/equities/signals/aapl")
    assert response.status_code == 200
    assert "equities" in response.get_data(as_text=True)


def test_attach_fluxboard_equities_routes_does_not_answer_tokenm_alias(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    dist_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>equities</html>", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_equities_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/tokenm")

    assert response.status_code == 404


def test_resolve_strategy_name_requires_explicit_supported_strategy_class_for_equities() -> None:
    with pytest.raises(ValueError, match="api.strategy_class"):
        _resolve_strategy_name({})

    with pytest.raises(ValueError, match="api.strategy_class"):
        _resolve_strategy_name({"strategy_class": "maker_v9"})


def test_resolve_strategy_name_rejects_param_set_drift() -> None:
    with pytest.raises(ValueError, match="api.param_set"):
        _resolve_strategy_name(
            {
                "strategy_class": "maker_v4",
                "param_set": "makerv3",
            }
        )


def test_load_config_applies_redis_env_overrides(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "equities.toml"
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
    monkeypatch.setenv("EQUITIES_REDIS_HOST", "redis.internal")
    monkeypatch.setenv("EQUITIES_REDIS_PORT", "6379")
    monkeypatch.setenv("EQUITIES_REDIS_USERNAME", "default")
    monkeypatch.setenv("EQUITIES_REDIS_PASSWORD", "secret")
    monkeypatch.setenv("EQUITIES_REDIS_SSL", "true")

    config = _load_config(config_path)

    assert config["redis"]["host"] == "redis.internal"
    assert config["redis"]["port"] == 6379
    assert config["redis"]["username"] == "default"
    assert config["redis"]["password"] == "secret"
    assert config["redis"]["ssl"] is True
