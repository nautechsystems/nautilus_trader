from __future__ import annotations

import json
import threading
import subprocess
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(relative_path: str) -> str:
    return (_repo_root() / relative_path).read_text(encoding="utf-8")


class _RolloutHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/lp":
            body = b"<!doctype html><html><body>lp</body></html>"
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path in {"/api/v1/hedgers/instances", "/api/v1/hedgers/eth_plume_lp"}:
            body = json.dumps({"ok": True}).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/api/pulse/jobs":
            body = json.dumps(
                {
                    "jobs": [],
                    "shell_links": [],
                    "total": 0,
                    "active": 0,
                    "failed": 0,
                },
            ).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_response(404)
        self.end_headers()

    def log_message(self, format: str, *args: object) -> None:  # noqa: A003
        return


def _run_rollout_check(base_url: str) -> subprocess.CompletedProcess[str]:
    script = _repo_root() / "ops/scripts/deploy/check_lp_rollout.sh"
    return subprocess.run(  # noqa: S603
        ["bash", str(script), "--base-url", base_url],
        check=False,
        capture_output=True,
        text=True,
    )


def _run_preflight(
    *,
    common_env: Path,
    system_ini: Path,
    band1_config: Path,
    band2_config: Path,
    service_user: str | None = None,
) -> dict[str, Any]:
    script = _repo_root() / "ops/scripts/lp_hedger_preflight.py"
    command = [
        sys.executable,
        str(script),
        "--json",
        "--common-env",
        str(common_env),
        "--system-ini",
        str(system_ini),
        "--band1-config",
        str(band1_config),
        "--band2-config",
        str(band2_config),
    ]
    if service_user is not None:
        command.extend(["--service-user", service_user])
    result = subprocess.run(  # noqa: S603
        command,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode not in {0, 1}:
        raise RuntimeError(result.stderr or result.stdout)
    return json.loads(result.stdout)


def test_lp_prod_runbook_documents_shared_host_topology() -> None:
    text = _read("docs/runbooks/lp-hedger-production-rollout.md")

    assert "/lp" in text
    assert "/api/v1/hedgers/*" in text
    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in text
    assert "flux-lp.target" in text
    assert "service-eth-plume-lp-hedger" in text
    assert "service-eth-plume-lp-hedger-band2" in text
    assert "service-hedger3" in text
    assert "service-hedger4" in text
    assert "rollback" in text.lower()


def test_lp_prod_docs_distinguish_live_pair_from_staged_generic_instances() -> None:
    text = _read("deploy/lp/README.md")

    assert "Band1 and Band2" in text
    assert "hype_usdt_lp_hedger.ini" in text
    assert "plume_weth_lp_hedger.ini" in text
    assert "third_lp_hedger.ini.disabled" in text
    assert "/api/v1/hedgers/instances" in text
    assert "staged" in text.lower()
    assert "service-hedger3" in text
    assert "service-hedger4" in text


def test_lp_preflight_requires_loopback_backend_url_and_system_ini_sections(tmp_path: Path) -> None:
    common_env = tmp_path / "common.env"
    common_env.write_text("LP_API_BACKEND_URL=http://127.0.0.1:5025\n", encoding="utf-8")
    system_ini = tmp_path / "lp-system.ini"
    system_ini.write_text("[redis]\nurl=redis://example\n[plume]\nrpc_url=http://rpc\n", encoding="utf-8")
    band1_config = tmp_path / "band1.ini"
    band1_config.write_text("[identity]\nid=band1\n", encoding="utf-8")
    band2_config = tmp_path / "band2.ini"
    band2_config.write_text("[identity]\nid=band2\n", encoding="utf-8")

    result = _run_preflight(
        common_env=common_env,
        system_ini=system_ini,
        band1_config=band1_config,
        band2_config=band2_config,
    )

    assert result["ok"] is False
    errors = result["errors"]
    assert any("bybit_hedger" in error for error in errors)
    assert any("bybit_hedger_band2" in error for error in errors)


def test_lp_preflight_accepts_band1_band2_config_contract(tmp_path: Path) -> None:
    common_env = tmp_path / "common.env"
    common_env.write_text("LP_API_BACKEND_URL=http://127.0.0.1:5025\n", encoding="utf-8")
    system_ini = tmp_path / "lp-system.ini"
    system_ini.write_text(
        "\n".join(
            [
                "[redis]",
                "url=redis://example",
                "[plume]",
                "rpc_url=http://rpc",
                "[bybit]",
                "api_domain=example",
                "[bybit_hedger]",
                "enabled=true",
                "[bybit_hedger_band2]",
                "enabled=true",
            ],
        )
        + "\n",
        encoding="utf-8",
    )
    band1_config = tmp_path / "band1.ini"
    band1_config.write_text("[identity]\nid=band1\n", encoding="utf-8")
    band2_config = tmp_path / "band2.ini"
    band2_config.write_text("[identity]\nid=band2\n", encoding="utf-8")

    result = _run_preflight(
        common_env=common_env,
        system_ini=system_ini,
        band1_config=band1_config,
        band2_config=band2_config,
    )

    assert result["ok"] is True
    assert result["errors"] == []


def test_lp_preflight_requires_system_ini_readable_by_service_user(tmp_path: Path) -> None:
    common_env = tmp_path / "common.env"
    common_env.write_text("LP_API_BACKEND_URL=http://127.0.0.1:5025\n", encoding="utf-8")
    system_ini = tmp_path / "lp-system.ini"
    system_ini.write_text(
        "\n".join(
            [
                "[redis]",
                "url=redis://example",
                "[plume]",
                "rpc_url=http://rpc",
                "[bybit]",
                "api_key=key",
                "secret=secret",
                "[bybit_hedger]",
                "api_key=key",
                "secret=secret",
                "[bybit_hedger_band2]",
                "api_key=key",
                "secret=secret",
            ],
        )
        + "\n",
        encoding="utf-8",
    )
    system_ini.chmod(0o600)
    band1_config = tmp_path / "band1.ini"
    band1_config.write_text("[identity]\nid=band1\n", encoding="utf-8")
    band2_config = tmp_path / "band2.ini"
    band2_config.write_text("[identity]\nid=band2\n", encoding="utf-8")

    result = _run_preflight(
        common_env=common_env,
        system_ini=system_ini,
        band1_config=band1_config,
        band2_config=band2_config,
        service_user="nobody",
    )

    assert result["ok"] is False
    assert any("service user" in error.lower() for error in result["errors"])


def test_lp_systemd_contract_documents_shared_host_env_requirements() -> None:
    text = _read("deploy/lp/systemd/common.env.example")

    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in text
    assert "LP_SYSTEM_CONFIG=/etc/flux/lp-system.ini" in text


def test_lp_readme_documents_shared_host_restart_order() -> None:
    text = _read("deploy/lp/README.md")

    assert "/etc/flux/common.env" in text
    assert "flux@tokenmm-api.service" in text
    assert "flux-lp.target" in text


def test_lp_rollout_check_script_covers_ui_api_and_pulse() -> None:
    script = _read("ops/scripts/deploy/check_lp_rollout.sh")

    assert "/lp" in script
    assert "/api/v1/hedgers/instances" in script
    assert "/api/v1/hedgers/eth_plume_lp" in script
    assert "/api/pulse/jobs" in script


def test_lp_rollout_check_accepts_real_pulse_jobs_payload_shape() -> None:
    server = ThreadingHTTPServer(("127.0.0.1", 0), _RolloutHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        result = _run_rollout_check(f"http://127.0.0.1:{server.server_port}")
    finally:
        server.shutdown()
        thread.join()
        server.server_close()

    assert result.returncode == 0, result.stderr or result.stdout


def test_lp_prod_runbook_documents_go_no_go_and_rollback() -> None:
    text = _read("docs/runbooks/lp-hedger-production-rollout.md")

    assert "go/no-go" in text.lower()
    assert "rollback" in text.lower()
    assert "chainsaw" in text.lower()


def test_lp_prod_runbook_documents_shared_frontend_build_prereqs() -> None:
    text = _read("docs/runbooks/lp-hedger-production-rollout.md")

    assert "pnpm --dir fluxboard build" in text
    assert "pnpm --dir pulse-ui build" in text


def test_lp_rollout_review_template_captures_restart_times_and_residual_risks() -> None:
    text = _read("docs/reviews/2026-03-09-lp-hedger-prod-rollout.md")

    assert "restart" in text.lower()
    assert "residual risks" in text.lower()
    assert "/lp" in text
    assert "/api/v1/hedgers/instances" in text
    assert "/api/pulse/jobs" in text
    assert "rollback" in text.lower()
