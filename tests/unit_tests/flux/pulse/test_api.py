from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any

from flask import Flask

from flux.pulse.api import PulseControlPlane
from flux.pulse.api import _extract_active_since
from flux.pulse.api import _extract_error_info


@dataclass
class FakeCompletedProcess:
    args: list[str]
    returncode: int = 0
    stdout: str = ""
    stderr: str = ""


def _status_output(*, state: str, pid: int | None = None, memory: str | None = None) -> str:
    pid_line = f" Main PID: {pid} (python)\n" if pid is not None else ""
    memory_line = f"   Memory: {memory}\n" if memory is not None else ""
    return (
        "● flux@test.service - Test\n"
        f"   Active: {state} since Fri 2026-03-06 10:00:00 UTC; 15min ago\n"
        f"{pid_line}"
        f"{memory_line}"
    )


def _expected_shell_links(*surfaces: str) -> list[dict[str, str]]:
    labels = {
        "tokenmm": "TokenMM",
        "equities": "Equities",
    }
    suffixes = (
        ("Dashboard", ""),
        ("Signal", "signal"),
        ("Params", "params"),
        ("Balances", "balances"),
        ("Trades", "trades"),
        ("Alerts", "alerts"),
    )

    links: list[dict[str, str]] = []
    for surface in surfaces:
        surface_label = labels[surface]
        for link_label, suffix in suffixes:
            path = surface if not suffix else f"{surface}/{suffix}"
            links.append(
                {
                    "label": f"{surface_label} {link_label}",
                    "path": path,
                    "surface": surface,
                },
            )
    return links


def test_discover_services_filters_to_pulse_enrolled_env_files(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "common.env").write_text("BYBIT_API_KEY=secret\n", encoding="utf-8")
    (env_dir / "tokenmm-api.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM API",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_api"',
            ],
        ),
        encoding="utf-8",
    )
    (env_dir / "tokenmm-hidden.env").write_text(
        'CMD="python -m somewhere"\n',
        encoding="utf-8",
    )

    control_plane = PulseControlPlane(env_dir=env_dir)

    services = control_plane.discover_services()

    assert [service.job_id for service in services] == ["tokenmm-api"]
    assert services[0].description == "TokenMM API"


def test_list_jobs_preserves_custom_group_metadata_from_env_files(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "tg-bot-lan-rogue-trader-alert.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=Lan Rogue Trader Alert",
                "PULSE_GROUP_KEY=tg-bots",
                "PULSE_GROUP_LABEL=TG Bots",
                "PULSE_GROUP_ORDER=60",
                'CMD="python -m flux.runners.tg_bots.run_lan_rogue_trader_alert"',
            ],
        ),
        encoding="utf-8",
    )

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        if cmd[:3] == ["systemctl", "status", "flux@tg-bot-lan-rogue-trader-alert"]:
            return FakeCompletedProcess(cmd, stdout=_status_output(state="inactive (dead)"))
        if cmd[:4] == ["sudo", "journalctl", "-u", "flux@tg-bot-lan-rogue-trader-alert"]:
            return FakeCompletedProcess(cmd, stdout="")
        raise AssertionError(f"unexpected command: {cmd}")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
    )
    app = Flask(__name__)
    control_plane.register_routes(app)

    response = app.test_client().get("/api/pulse/jobs")

    assert response.status_code == 200
    payload = response.get_json()
    assert payload["jobs"][0]["group_key"] == "tg-bots"
    assert payload["jobs"][0]["group_label"] == "TG Bots"
    assert payload["jobs"][0]["group_order"] == 60


def test_extract_error_info_ignores_socketio_invalid_session_restart_noise() -> None:
    logs_text = "\n".join(
        [
            "Mar 06 19:20:48 host flux-tokenmm-api[237388]: Invalid session pBNTI8S-rFGf97YpAAAA (further occurrences of this error will be logged with level INFO)",
            'Mar 06 19:20:48 host flux-tokenmm-api[237388]: 1.2.3.4 - - [06/Mar/2026 19:20:48] "POST /socket.io/?profile=tokenmm&EIO=4&transport=polling&t=a52fo79t&sid=pBNTI8S-rFGf97YpAAAA HTTP/1.1" 400 -',
            "Mar 06 19:20:49 host flux-tokenmm-api[237388]: INFO connected",
        ],
    )

    assert _extract_error_info(logs_text) == {
        "count": 0,
        "last_seen": None,
        "preview": None,
    }


def test_extract_error_info_returns_last_seen_for_latest_matching_line() -> None:
    logs_text = "\n".join(
        [
            "2026-03-06T19:20:48+00:00 host flux-tokenmm-api[237388]: INFO healthy",
            "2026-03-06T19:20:49+00:00 host flux-tokenmm-api[237388]: ERROR first failure",
            "2026-03-06T19:20:50+00:00 host flux-tokenmm-api[237388]: INFO retrying",
            "2026-03-06T19:20:51+00:00 host flux-tokenmm-api[237388]: CRITICAL latest failure",
        ],
    )

    assert _extract_error_info(logs_text) == {
        "count": 2,
        "last_seen": "2026-03-06T19:20:51+00:00",
        "preview": "CRITICAL latest failure",
    }


def test_extract_active_since_reads_current_systemd_activation_timestamp() -> None:
    output = _status_output(state="active (running)", pid=1234, memory="45.2M")

    assert _extract_active_since(output, status="active") == "Fri 2026-03-06 10:00:00 UTC"


def test_list_jobs_returns_shell_links_and_jobs_payload_with_grouping_and_error_counts(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "tokenmm-api.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM API",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                "PORT=5022",
                'CMD="python -m flux.runners.tokenmm.run_api"',
            ],
        ),
        encoding="utf-8",
    )
    (env_dir / "tokenmm-bridge.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM Bridge",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_bridge"',
            ],
        ),
        encoding="utf-8",
    )

    calls: list[list[str]] = []

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        calls.append(cmd)
        if cmd[:3] == ["systemctl", "status", "flux@tokenmm-api"]:
            return FakeCompletedProcess(cmd, stdout=_status_output(state="active (running)", pid=1234, memory="45.2M"))
        if cmd[:3] == ["systemctl", "status", "flux@tokenmm-bridge"]:
            return FakeCompletedProcess(cmd, stdout=_status_output(state="failed", pid=None, memory=None))
        if cmd[:4] == ["sudo", "journalctl", "-u", "flux@tokenmm-api"]:
            return FakeCompletedProcess(
                cmd,
                stdout=(
                    "2026-03-06T19:20:48+00:00 host flux-tokenmm-api[237388]: INFO healthy\n"
                    "2026-03-06T19:20:49+00:00 host flux-tokenmm-api[237388]: ERROR something bad\n"
                ),
            )
        if cmd[:4] == ["sudo", "journalctl", "-u", "flux@tokenmm-bridge"]:
            return FakeCompletedProcess(
                cmd,
                stdout="2026-03-06T19:20:48+00:00 host flux-tokenmm-bridge[237388]: INFO idle\n",
            )
        raise AssertionError(f"unexpected command: {cmd}")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
    )
    app = Flask(__name__)
    control_plane.register_routes(app)

    response = app.test_client().get("/api/pulse/jobs")

    assert response.status_code == 200
    payload = response.get_json()
    assert payload == {
        "jobs": [
            {
                "cmd": "python -m flux.runners.tokenmm.run_api",
                "description": "TokenMM API",
                "errors": {
                    "count": 1,
                    "last_seen": "2026-03-06T19:20:49+00:00",
                    "preview": "ERROR something bad",
                },
                "group_key": "tokenmm",
                "group_label": "TokenMM",
                "group_order": 10,
                "id": "tokenmm-api",
                "memory": "45.2M",
                "name": "tokenmm-api",
                "pid": 1234,
                "prefix": "tokenmm",
                "status": "active",
                "unit": "flux@tokenmm-api",
                "uptime": "15min",
            },
            {
                "cmd": "python -m flux.runners.tokenmm.run_bridge",
                "description": "TokenMM Bridge",
                "errors": {
                    "count": 0,
                    "last_seen": None,
                    "preview": None,
                },
                "group_key": "tokenmm",
                "group_label": "TokenMM",
                "group_order": 10,
                "id": "tokenmm-bridge",
                "memory": None,
                "name": "tokenmm-bridge",
                "pid": None,
                "prefix": "tokenmm",
                "status": "failed",
                "unit": "flux@tokenmm-bridge",
                "uptime": None,
            },
        ],
        "shell_links": _expected_shell_links("tokenmm"),
        "total": 2,
        "active": 1,
        "failed": 1,
    }
    assert calls


def test_list_jobs_returns_shared_host_shell_links_when_equities_proxy_is_configured(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "common.env").write_text(
        "EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024\n",
        encoding="utf-8",
    )
    (env_dir / "tokenmm-api.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM API",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                "PORT=5022",
                'CMD="python -m flux.runners.tokenmm.run_api"',
            ],
        ),
        encoding="utf-8",
    )

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        if cmd[:3] == ["systemctl", "status", "flux@tokenmm-api"]:
            return FakeCompletedProcess(cmd, stdout=_status_output(state="active (running)", pid=1234, memory="45.2M"))
        if cmd[:4] == ["sudo", "journalctl", "-u", "flux@tokenmm-api"]:
            return FakeCompletedProcess(cmd, stdout="INFO healthy\n")
        raise AssertionError(f"unexpected command: {cmd}")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
    )
    app = Flask(__name__)
    control_plane.register_routes(app)

    response = app.test_client().get("/api/pulse/jobs")

    assert response.status_code == 200
    payload = response.get_json()
    assert payload["shell_links"] == _expected_shell_links("tokenmm", "equities")


def test_list_jobs_scopes_active_job_errors_to_current_activation(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "tokenmm-api.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM API",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                "PORT=5022",
                'CMD="python -m flux.runners.tokenmm.run_api"',
            ],
        ),
        encoding="utf-8",
    )

    journal_commands: list[list[str]] = []

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        if cmd[:3] == ["systemctl", "status", "flux@tokenmm-api"]:
            return FakeCompletedProcess(cmd, stdout=_status_output(state="active (running)", pid=1234, memory="45.2M"))
        if cmd[:4] == ["sudo", "journalctl", "-u", "flux@tokenmm-api"]:
            journal_commands.append(cmd)
            assert "--since" in cmd
            since_index = cmd.index("--since")
            assert cmd[since_index + 1] == "Fri 2026-03-06 10:00:00 UTC"
            assert "-o" in cmd
            format_index = cmd.index("-o")
            assert cmd[format_index + 1] == "short-iso"
            return FakeCompletedProcess(
                cmd,
                stdout="2026-03-06T19:20:48+00:00 host flux-tokenmm-api[237388]: INFO healthy\n",
            )
        raise AssertionError(f"unexpected command: {cmd}")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
    )
    app = Flask(__name__)
    control_plane.register_routes(app)

    response = app.test_client().get("/api/pulse/jobs")

    assert response.status_code == 200
    payload = response.get_json()
    assert payload["jobs"][0]["status"] == "active"
    assert payload["jobs"][0]["errors"] == {
        "count": 0,
        "last_seen": None,
        "preview": None,
    }
    assert journal_commands


def test_job_actions_and_logs_use_systemd_units_and_sudo_prefix(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "tokenmm-bridge.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM Bridge",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_bridge"',
            ],
        ),
        encoding="utf-8",
    )

    calls: list[list[str]] = []

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        calls.append(cmd)
        return FakeCompletedProcess(cmd, stdout="ok")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
    )
    app = Flask(__name__)
    control_plane.register_routes(app)
    client = app.test_client()

    assert client.post("/api/pulse/jobs/tokenmm-bridge/start").status_code == 200
    assert client.post("/api/pulse/jobs/tokenmm-bridge/stop").status_code == 200
    assert client.post("/api/pulse/jobs/tokenmm-bridge/restart").status_code == 200
    logs_response = client.get("/api/pulse/jobs/tokenmm-bridge/logs?lines=50")

    assert logs_response.status_code == 200
    assert logs_response.get_data(as_text=True) == "ok"
    assert calls == [
        ["sudo", "systemctl", "start", "flux@tokenmm-bridge"],
        ["sudo", "systemctl", "stop", "flux@tokenmm-bridge"],
        ["sudo", "systemctl", "restart", "flux@tokenmm-bridge"],
        ["sudo", "journalctl", "-u", "flux@tokenmm-bridge", "-n", "50", "--no-pager"],
    ]


def test_self_stop_is_deferred_and_returns_accepted(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "tokenmm-api.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM API",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_api"',
            ],
        ),
        encoding="utf-8",
    )

    calls: list[list[str]] = []
    deferred: list[Any] = []

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        calls.append(cmd)
        return FakeCompletedProcess(cmd, stdout="ok")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
        self_service_id="tokenmm-api",
        defer_action=deferred.append,
    )
    app = Flask(__name__)
    control_plane.register_routes(app)

    response = app.test_client().post("/api/pulse/jobs/tokenmm-api/stop")

    assert response.status_code == 202
    assert response.get_json()["pending"] is True
    assert calls == []
    assert deferred == []

    response.close()

    assert len(deferred) == 1


def test_group_stop_uses_correct_past_tense_and_defers_self_actions(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "tokenmm-api.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM API",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_api"',
            ],
        ),
        encoding="utf-8",
    )
    (env_dir / "tokenmm-bridge.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM Bridge",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_bridge"',
            ],
        ),
        encoding="utf-8",
    )

    calls: list[list[str]] = []
    deferred: list[Any] = []

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        calls.append(cmd)
        return FakeCompletedProcess(cmd, stdout="ok")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
        self_service_id="tokenmm-api",
        defer_action=deferred.append,
    )
    app = Flask(__name__)
    control_plane.register_routes(app)

    response = app.test_client().post("/api/pulse/jobs/group/tokenmm/stop")

    assert response.status_code == 202
    assert response.get_json() == {
        "success": True,
        "message": "stopped 1 jobs in group 'tokenmm'",
        "stopped": ["tokenmm-bridge"],
        "pending": True,
        "deferred": ["tokenmm-api"],
        "errors": [],
    }
    assert calls == [["sudo", "systemctl", "stop", "flux@tokenmm-bridge"]]
    assert deferred == []

    response.close()

    assert len(deferred) == 1


def test_group_restart_defers_self_restart_until_response_closes(tmp_path: Path) -> None:
    env_dir = tmp_path / "etc" / "flux"
    env_dir.mkdir(parents=True)
    (env_dir / "tokenmm-api.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM API",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_api"',
            ],
        ),
        encoding="utf-8",
    )
    (env_dir / "tokenmm-bridge.env").write_text(
        "\n".join(
            [
                "PULSE_ENABLED=1",
                "PULSE_DESCRIPTION=TokenMM Bridge",
                "PULSE_GROUP_KEY=tokenmm",
                "PULSE_GROUP_LABEL=TokenMM",
                "PULSE_GROUP_ORDER=10",
                'CMD="python -m flux.runners.tokenmm.run_bridge"',
            ],
        ),
        encoding="utf-8",
    )

    calls: list[list[str]] = []
    deferred: list[Any] = []

    def runner(cmd: list[str], **_: Any) -> FakeCompletedProcess:
        calls.append(cmd)
        return FakeCompletedProcess(cmd, stdout="ok")

    control_plane = PulseControlPlane(
        env_dir=env_dir,
        command_runner=runner,
        sudo_prefix=["sudo"],
        self_service_id="tokenmm-api",
        defer_action=deferred.append,
    )
    app = Flask(__name__)
    control_plane.register_routes(app)

    response = app.test_client().post("/api/pulse/jobs/group/tokenmm/restart")

    assert response.status_code == 202
    assert response.get_json() == {
        "success": True,
        "message": "restarted 1 jobs in group 'tokenmm'",
        "restarted": ["tokenmm-bridge"],
        "pending": True,
        "deferred": ["tokenmm-api"],
        "errors": [],
    }
    assert calls == [["sudo", "systemctl", "restart", "flux@tokenmm-bridge"]]
    assert deferred == []

    response.close()

    assert len(deferred) == 1
