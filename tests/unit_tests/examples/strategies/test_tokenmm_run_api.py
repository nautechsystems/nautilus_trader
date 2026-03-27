from __future__ import annotations

from argparse import Namespace
from pathlib import Path
from urllib.error import HTTPError

import pytest
from flask import Flask

from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.tokenmm.run_api import _attach_fluxboard_tokenmm_routes
from flux.runners.tokenmm.run_api import _attach_profile_router_proxy
from flux.runners.tokenmm.run_api import _attach_pulse_routes
from flux.runners.tokenmm.run_api import _attach_tokenmm_readiness_route
from flux.runners.tokenmm.run_api import _build_flux_config
from flux.runners.tokenmm.run_api import _build_profile_strategy_maps
from flux.runners.tokenmm.run_api import _load_config
from flux.runners.tokenmm.run_api import _parse_args
from flux.runners.tokenmm.run_api import _resolve_bind_host
from flux.runners.tokenmm.run_api import _should_enable_pulse_routes
from flux.runners.tokenmm.run_api import _tokenmm_profile_summary


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
    assert (
        _resolve_bind_host({"api": {"host": "127.0.0.1"}}, Namespace(host="10.0.0.8")) == "10.0.0.8"
    )


def test_build_profile_strategy_maps_reads_tokenmm_allowlist_and_required_subset() -> None:
    strategy_map, required_map = _build_profile_strategy_maps(
        {
            "tokenmm_strategy_ids": ["strategy_a", "strategy_b"],
            "tokenmm_required_strategy_ids": ["strategy_a"],
        },
    )

    assert strategy_map == {"tokenmm": ["strategy_a", "strategy_b"]}
    assert required_map == {"tokenmm": ["strategy_a"]}


def test_tokenmm_descriptor_exposes_stable_profile_contract() -> None:
    descriptor = get_strategy_set_descriptor("tokenmm")

    assert descriptor.profile == "tokenmm"
    assert descriptor.aliases == ("tokenmm", "tokenm")
    assert descriptor.base_path == "/tokenmm"
    assert descriptor.route_aliases == ("/tokenm",)
    assert descriptor.strategy_ids_field == "tokenmm_strategy_ids"
    assert descriptor.required_strategy_ids_field == "tokenmm_required_strategy_ids"


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


def test_parse_args_describes_shared_fluxboard_static_contract(monkeypatch, capsys) -> None:
    monkeypatch.setattr("sys.argv", ["run_api.py", "--help"])

    with pytest.raises(SystemExit, match="0"):
        _parse_args()

    captured = capsys.readouterr()
    assert "/static/fluxboard/*" in captured.out
    assert "/tokenmm" in captured.out
    assert "/lp" in captured.out


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


def test_attach_fluxboard_routes_serve_neutral_shared_asset_prefix(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    assets_dir = dist_dir / "assets"
    assets_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>fluxboard</html>", encoding="utf-8")
    (assets_dir / "app.js").write_text("console.log('shared')", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_tokenmm_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/lp")
    assert response.status_code == 200
    assert "fluxboard" in response.get_data(as_text=True)

    response = client.get("/tokenmm")
    assert response.status_code == 200
    assert "fluxboard" in response.get_data(as_text=True)

    response = client.get("/lp/hedger")
    assert response.status_code == 200
    assert "fluxboard" in response.get_data(as_text=True)

    response = client.get("/static/fluxboard/assets/app.js")
    assert response.status_code == 200
    assert "console.log('shared')" in response.get_data(as_text=True)

    response = client.get("/lp/assets/app.js")
    assert response.status_code == 404

    response = client.get("/tokenmm/assets/app.js")
    assert response.status_code == 404


def test_attach_fluxboard_routes_keep_spa_paths_from_serving_dist_root_files(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    dist_dir.mkdir()
    (dist_dir / "index.html").write_text("<html>fluxboard</html>", encoding="utf-8")
    (dist_dir / "favicon.svg").write_text("<svg>shared-icon</svg>", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_tokenmm_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/static/fluxboard/favicon.svg")
    assert response.status_code == 200
    assert response.get_data(as_text=True) == "<svg>shared-icon</svg>"

    response = client.get("/tokenmm/favicon.svg")
    assert response.status_code == 200
    assert response.get_data(as_text=True) == "<html>fluxboard</html>"

    response = client.get("/lp/favicon.svg")
    assert response.status_code == 200
    assert response.get_data(as_text=True) == "<html>fluxboard</html>"


def test_attach_fluxboard_routes_proxy_missing_shared_assets_to_equities_backend(
    monkeypatch,
    tmp_path: Path,
) -> None:
    dist_dir = tmp_path / "dist"
    dist_dir.mkdir()
    (dist_dir / "index.html").write_text("<html>fluxboard</html>", encoding="utf-8")
    captured: dict[str, object] = {}

    def _fake_proxy_request_to_backend(backend_url: str):
        captured["backend_url"] = backend_url
        from flask import Response

        return Response("console.log('equities')", status=200, content_type="text/javascript")

    monkeypatch.setattr(
        "flux.runners.tokenmm.run_api._proxy_request_to_backend",
        _fake_proxy_request_to_backend,
    )

    app = Flask(__name__)
    _attach_fluxboard_tokenmm_routes(
        app,
        dist_dir=dist_dir,
        surface_backends={"equities": "http://127.0.0.1:5024"},
    )
    client = app.test_client()

    response = client.get("/static/fluxboard/assets/index-equities.js")

    assert response.status_code == 200
    assert response.get_data(as_text=True) == "console.log('equities')"
    assert captured["backend_url"] == "http://127.0.0.1:5024"


def test_attach_pulse_routes_serves_index_assets_and_spa_fallback(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    assets_dir = dist_dir / "assets"
    assets_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>pulse</html>", encoding="utf-8")
    (assets_dir / "pulse.js").write_text("console.log('pulse')", encoding="utf-8")

    app = Flask(__name__)
    _attach_pulse_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    response = client.get("/pulse")
    assert response.status_code == 200
    assert "pulse" in response.get_data(as_text=True)

    response = client.get("/pulse/assets/pulse.js")
    assert response.status_code == 200
    assert "console.log('pulse')" in response.get_data(as_text=True)

    response = client.get("/pulse/jobs/tokenmm-api")
    assert response.status_code == 200
    assert "pulse" in response.get_data(as_text=True)


def test_attach_tokenmm_readiness_route_returns_enveloped_readiness_payload() -> None:
    captured: dict[str, bool] = {}

    def _fake_readiness_loader() -> dict[str, object]:
        captured["called"] = True
        return {
            "ok": False,
            "summary": {
                "failed_checks": ["state_stream_freshness"],
                "stale_state_stream_strategy_ids": ["strategy_a"],
            },
            "checks": {
                "state_stream_freshness": {
                    "ok": False,
                    "summary": "1 strategy has a stale state stream.",
                },
            },
        }

    app = Flask(__name__)
    _attach_tokenmm_readiness_route(app, readiness_loader=_fake_readiness_loader)
    client = app.test_client()

    response = client.get("/api/v1/readiness?profile=tokenmm")

    assert response.status_code == 200
    payload = response.get_json()
    assert payload["ok"] is True
    assert payload["data"]["ok"] is False
    assert payload["data"]["summary"]["stale_state_stream_strategy_ids"] == ["strategy_a"]
    assert captured["called"] is True


def test_attach_profile_router_proxy_forwards_equities_page_requests(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _FakeResponse:
        def __init__(self, *, status: int, body: bytes, headers: dict[str, str]) -> None:
            self.status = status
            self._body = body
            self.headers = headers

        def read(self) -> bytes:
            return self._body

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb) -> None:
            return None

    def _fake_urlopen(req, timeout: float):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        return _FakeResponse(
            status=200,
            body=b"<html><head></head><body>equities</body></html>",
            headers={"Content-Type": "text/html"},
        )

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"equities": "http://127.0.0.1:5024"},
    )
    client = app.test_client()

    response = client.get("/equities")

    assert response.status_code == 200
    body = response.get_data(as_text=True)
    assert "equities" in body
    assert "window.__FLUXBOARD_RUNTIME_CONFIG__" in body
    assert '"socketPaths":{"equities":"/socket.io"}' in body
    assert captured["url"] == "http://127.0.0.1:5024/equities"
    assert captured["method"] == "GET"


def test_attach_profile_router_proxy_forwards_equities_profile_api_requests(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _FakeResponse:
        def __init__(self, *, status: int, body: bytes, headers: dict[str, str]) -> None:
            self.status = status
            self._body = body
            self.headers = headers

        def read(self) -> bytes:
            return self._body

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb) -> None:
            return None

    def _fake_urlopen(req, timeout: float):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        return _FakeResponse(
            status=200,
            body=b'{"ok":true}',
            headers={"Content-Type": "application/json"},
        )

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"equities": "http://127.0.0.1:5024"},
    )
    client = app.test_client()

    response = client.get("/api/v1/params?profile=equities")

    assert response.status_code == 200
    assert response.get_json() == {"ok": True}
    assert captured["url"] == "http://127.0.0.1:5024/api/v1/params?profile=equities"
    assert captured["method"] == "GET"


def test_attach_profile_router_proxy_forwards_equities_socketio_requests(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _FakeResponse:
        def __init__(self, *, status: int, body: bytes, headers: dict[str, str]) -> None:
            self.status = status
            self._body = body
            self.headers = headers

        def read(self) -> bytes:
            return self._body

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb) -> None:
            return None

    def _fake_urlopen(req, timeout: float):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        captured["body"] = req.data
        return _FakeResponse(
            status=200,
            body=b"ok",
            headers={"Content-Type": "text/plain"},
        )

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"equities": "http://127.0.0.1:5024"},
    )
    client = app.test_client()

    response = client.post(
        "/socket.io/?profile=equities&EIO=4&transport=polling",
        data=b"40",
        content_type="text/plain",
    )

    assert response.status_code == 200
    assert response.get_data(as_text=True) == "ok"
    assert captured["url"] == "http://127.0.0.1:5024/socket.io/?profile=equities&EIO=4&transport=polling"
    assert captured["method"] == "POST"
    assert captured["body"] == b"40"


def test_attach_profile_router_proxy_forwards_equities_socketio_path_requests(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _FakeResponse:
        def __init__(self, *, status: int, body: bytes, headers: dict[str, str]) -> None:
            self.status = status
            self._body = body
            self.headers = headers

        def read(self) -> bytes:
            return self._body

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb) -> None:
            return None

    def _fake_urlopen(req, timeout: float):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        captured["body"] = req.data
        return _FakeResponse(
            status=200,
            body=b"0{\"sid\":\"abc\"}",
            headers={"Content-Type": "text/plain"},
        )

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"equities": "http://127.0.0.1:5024"},
    )
    client = app.test_client()

    response = client.get("/equities/socket.io/?EIO=4&transport=polling")

    assert response.status_code == 200
    assert response.get_data(as_text=True) == '0{"sid":"abc"}'
    assert captured["url"] == "http://127.0.0.1:5024/socket.io/?EIO=4&transport=polling"
    assert captured["method"] == "GET"


def test_attach_profile_router_proxy_leaves_tokenmm_requests_unhandled() -> None:
    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"equities": "http://127.0.0.1:5024"},
    )
    client = app.test_client()

    response = client.get("/api/v1/params?profile=tokenmm")

    assert response.status_code == 404


def test_attach_profile_router_proxy_returns_bad_gateway_on_backend_error(monkeypatch) -> None:
    def _fake_urlopen(req, timeout: float):
        raise HTTPError(req.full_url, 502, "bad gateway", hdrs=None, fp=None)

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"equities": "http://127.0.0.1:5024"},
    )
    client = app.test_client()

    response = client.get("/equities")

    assert response.status_code == 502


def test_attach_profile_router_proxy_leaves_lp_ui_routes_unhandled(monkeypatch) -> None:
    captured: dict[str, object] = {}

    def _fake_urlopen(req, timeout: float):
        captured["url"] = req.full_url
        raise AssertionError("lp UI routes should not proxy to the hidden backend")

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"lp": "http://127.0.0.1:5025"},
    )
    client = app.test_client()

    response = client.get("/lp")

    assert response.status_code == 404
    assert captured == {}


def test_attach_profile_router_proxy_forwards_lp_hedger_api_requests(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _FakeResponse:
        def __init__(self, *, status: int, body: bytes, headers: dict[str, str]) -> None:
            self.status = status
            self._body = body
            self.headers = headers

        def read(self) -> bytes:
            return self._body

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb) -> None:
            return None

    def _fake_urlopen(req, timeout: float):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        return _FakeResponse(
            status=200,
            body=b'{"ok":true}',
            headers={"Content-Type": "application/json"},
        )

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={"lp": "http://127.0.0.1:5025"},
    )
    client = app.test_client()

    response = client.get("/api/v1/hedgers/instances")

    assert response.status_code == 200
    assert response.get_json() == {"ok": True}
    assert captured["url"] == "http://127.0.0.1:5025/api/v1/hedgers/instances"
    assert captured["method"] == "GET"


def test_attach_profile_router_proxy_prefers_lp_path_over_stale_profile_query(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _FakeResponse:
        def __init__(self, *, status: int, body: bytes, headers: dict[str, str]) -> None:
            self.status = status
            self._body = body
            self.headers = headers

        def read(self) -> bytes:
            return self._body

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb) -> None:
            return None

    def _fake_urlopen(req, timeout: float):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        return _FakeResponse(
            status=200,
            body=b'{"surface":"lp"}',
            headers={"Content-Type": "application/json"},
        )

    monkeypatch.setattr("flux.runners.tokenmm.run_api.urllib_request.urlopen", _fake_urlopen)

    app = Flask(__name__)
    _attach_profile_router_proxy(
        app,
        surface_backends={
            "equities": "http://127.0.0.1:5015",
            "lp": "http://127.0.0.1:5025",
        },
    )
    client = app.test_client()

    response = client.get("/api/v1/hedgers/instances?profile=equities")

    assert response.status_code == 200
    assert response.get_json() == {"surface": "lp"}
    assert captured["url"] == "http://127.0.0.1:5025/api/v1/hedgers/instances?profile=equities"
    assert captured["method"] == "GET"


def test_should_enable_pulse_routes_defaults_to_disabled() -> None:
    assert _should_enable_pulse_routes(Namespace(serve_pulse=False), {}) is False


def test_should_enable_pulse_routes_honors_cli_or_config_enablement() -> None:
    assert _should_enable_pulse_routes(Namespace(serve_pulse=True), {}) is True
    assert _should_enable_pulse_routes(Namespace(serve_pulse=False), {"enable_pulse_routes": True}) is True


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
    monkeypatch.setenv(
        "TOKENMM_REDIS_HOST",
        "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com",
    )
    monkeypatch.setenv("TOKENMM_REDIS_PORT", "6379")
    monkeypatch.setenv("TOKENMM_REDIS_USERNAME", "default")
    monkeypatch.setenv("TOKENMM_REDIS_PASSWORD", "secret")
    monkeypatch.setenv("TOKENMM_REDIS_SSL", "true")

    config = _load_config(config_path)

    assert (
        config["redis"]["host"]
        == "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com"
    )
    assert config["redis"]["port"] == 6379
    assert config["redis"]["username"] == "default"
    assert config["redis"]["password"] == "secret"
    assert config["redis"]["ssl"] is True
