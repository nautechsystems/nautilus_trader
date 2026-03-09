from __future__ import annotations

from pathlib import Path

from flask import Flask

import lp.api as lp_api
from lp.runners import run_api
from lp.runners.run_api import parse_args
from lp.runners.run_api import resolve_bind_port


def test_lp_api_runner_binds_hidden_backend_default_port() -> None:
    args = parse_args(["--config", str(Path("deploy/lp/lp.live.toml"))])

    assert resolve_bind_port(args, {}) == 5025


def test_lp_api_runner_accepts_fluxboard_flag() -> None:
    args = parse_args(["--config", str(Path("deploy/lp/lp.live.toml")), "--serve-fluxboard"])

    assert args.serve_fluxboard is True


def test_create_app_wires_real_redis_and_pulse_dependencies(monkeypatch) -> None:
    fake_redis = object()
    captured: dict[str, object] = {}

    class FakePulse:
        def __init__(self) -> None:
            self.actions: list[tuple[str, str]] = []

        def get_job_status(self, job_id: str) -> str:
            return f"status:{job_id}"

        def control_job(self, job_id: str, action: str) -> str:
            self.actions.append((job_id, action))
            return f"after:{action}:{job_id}"

    fake_pulse = FakePulse()

    def fake_get_redis_client(*, decode_responses: bool):
        captured["decode_responses"] = decode_responses
        return fake_redis

    def fake_create_lp_api_app(**kwargs):
        captured["kwargs"] = kwargs
        return "app-sentinel"

    monkeypatch.setattr(run_api, "get_redis_client", fake_get_redis_client, raising=False)
    monkeypatch.setattr(run_api, "PulseControlPlane", lambda: fake_pulse, raising=False)
    monkeypatch.setattr(lp_api, "create_lp_api_app", fake_create_lp_api_app)

    app = run_api.create_app()

    assert app == "app-sentinel"
    assert captured["decode_responses"] is False
    assert captured["kwargs"]["redis_client"] is fake_redis
    assert captured["kwargs"]["get_job_status"]("service-eth-plume-lp-hedger") == "status:service-eth-plume-lp-hedger"
    assert (
        captured["kwargs"]["control_job"]("service-eth-plume-lp-hedger", "restart")
        == "after:restart:service-eth-plume-lp-hedger"
    )
    assert fake_pulse.actions == [("service-eth-plume-lp-hedger", "restart")]


def test_create_app_adapts_generic_pulse_control_plane(monkeypatch) -> None:
    fake_redis = object()
    captured: dict[str, object] = {}

    class FakePulse:
        def __init__(self) -> None:
            self.actions: list[tuple[str, str]] = []

        def _service_by_id(self, job_id: str):
            return {"job_id": job_id}

        def _service_payload(self, service):
            return {"status": f"status:{service['job_id']}"}

        def _is_self_action(self, job_id: str, action: str) -> bool:
            return False

        def _run_systemctl_action(self, job_id: str, action: str) -> None:
            self.actions.append((job_id, action))

    fake_pulse = FakePulse()

    def fake_get_redis_client(*, decode_responses: bool):
        captured["decode_responses"] = decode_responses
        return fake_redis

    def fake_create_lp_api_app(**kwargs):
        captured["kwargs"] = kwargs
        return "app-sentinel"

    monkeypatch.setattr(run_api, "get_redis_client", fake_get_redis_client, raising=False)
    monkeypatch.setattr(run_api, "PulseControlPlane", lambda: fake_pulse, raising=False)
    monkeypatch.setattr(lp_api, "create_lp_api_app", fake_create_lp_api_app)

    app = run_api.create_app()

    assert app == "app-sentinel"
    assert captured["decode_responses"] is False
    assert captured["kwargs"]["get_job_status"]("service-eth-plume-lp-hedger") == "status:service-eth-plume-lp-hedger"
    assert captured["kwargs"]["control_job"]("service-eth-plume-lp-hedger", "restart") == (
        "status:service-eth-plume-lp-hedger"
    )
    assert fake_pulse.actions == [("service-eth-plume-lp-hedger", "restart")]


def test_attach_fluxboard_lp_routes_serves_lp_spa(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    assets_dir = dist_dir / "assets"
    assets_dir.mkdir(parents=True)
    (dist_dir / "index.html").write_text("<html>lp-shell</html>", encoding="utf-8")
    (assets_dir / "app.js").write_text("console.log('lp')", encoding="utf-8")

    app = Flask(__name__)
    run_api._attach_fluxboard_lp_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    assert client.get("/lp").get_data(as_text=True) == "<html>lp-shell</html>"
    assert client.get("/lp/hedger").get_data(as_text=True) == "<html>lp-shell</html>"
    assert client.get("/lp/assets/app.js").get_data(as_text=True) == "console.log('lp')"
