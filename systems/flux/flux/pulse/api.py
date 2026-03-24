from __future__ import annotations

import json
import os
import re
import subprocess
import threading
import time
from collections.abc import Callable
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from typing import Protocol
from typing import cast
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.request import urlopen

from flask import Blueprint
from flask import Response
from flask import jsonify
from flask import request


ERROR_MESSAGE_PATTERN = re.compile(
    r"\b(ERROR|CRITICAL|EXCEPTION|TRACEBACK|FAILED TO START|FAILED WITH RESULT|[A-Z][A-Z0-9_]*ERROR|[A-Z][A-Za-z0-9]+Error)\b",
    re.IGNORECASE,
)
JOURNAL_SHORT_ISO_TIMESTAMP_PATTERN = re.compile(
    r"^(?P<timestamp>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2}))\s+",
)
BENIGN_LOG_PATTERNS = (
    re.compile(
        r"Invalid session\s+\S+\s+\(further occurrences of this error will be logged with level INFO\)",
        re.IGNORECASE,
    ),
)
ACTION_PAST_TENSE = {
    "start": "started",
    "stop": "stopped",
    "restart": "restarted",
}
SHELL_LINK_SUFFIXES = (
    ("Dashboard", ""),
    ("Signal", "signal"),
    ("Params", "params"),
    ("Balances", "balances"),
    ("Trades", "trades"),
    ("Alerts", "alerts"),
)
SHELL_SURFACE_LABELS = {
    "tokenmm": "TokenMM",
    "equities": "Equities",
}
TOKENMM_READINESS_PATH = "/api/v1/readiness?profile=tokenmm"
READINESS_TIMEOUT_SECS = 2.0


class CommandRunner(Protocol):
    def __call__(self, cmd: list[str], **kwargs: Any) -> Any: ...


@dataclass(frozen=True)
class PulseService:
    job_id: str
    description: str
    group_key: str
    group_label: str
    group_order: int
    cmd: str | None
    port: int | None

    @property
    def prefix(self) -> str:
        return self.job_id.split("-", 1)[0] if "-" in self.job_id else self.job_id


def _parse_env_lines(lines: Sequence[str]) -> dict[str, str]:
    values: dict[str, str] = {}
    for raw_line in lines:
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if not key:
            continue
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
            value = value[1:-1]
        values[key] = value
    return values


def _coerce_port(raw_value: str | None) -> int | None:
    if raw_value is None:
        return None
    text = raw_value.strip()
    if not text:
        return None
    try:
        return int(text)
    except ValueError:
        return None


def _coerce_group_order(raw_value: str | None, *, default: int = 999) -> int:
    if raw_value is None:
        return default
    text = raw_value.strip()
    if not text:
        return default
    try:
        return int(text)
    except ValueError:
        return default


def _normalize_status(output: str) -> str:
    if "Active: active (running)" in output:
        return "active"
    if "Active: activating (start)" in output or "Active: reloading" in output:
        return "restarting"
    if "Active: failed" in output or "Active: activating (auto-restart)" in output:
        return "failed"
    if "Active: deactivating" in output:
        return "stopping"
    return "inactive"


def _extract_uptime(output: str, *, status: str) -> str | None:
    if status != "active":
        return None
    match = re.search(r"since [^;]+;\s*(.+?)\s+ago", output)
    if match is None:
        return None
    return match.group(1).strip() or None


def _extract_active_since(output: str, *, status: str) -> str | None:
    if status != "active":
        return None
    match = re.search(r"Active:\s+active(?:\s+\([^)]+\))?\s+since\s+([^;]+);", output)
    if match is None:
        return None
    return match.group(1).strip() or None


def _extract_error_info(logs_text: str) -> dict[str, Any]:
    matches: list[tuple[str | None, str]] = []
    for raw_line in logs_text.splitlines():
        line = raw_line.strip()
        if not line:
            continue

        timestamp_match = JOURNAL_SHORT_ISO_TIMESTAMP_PATTERN.match(line)
        timestamp = timestamp_match.group("timestamp") if timestamp_match else None
        _, separator, message = line.partition(": ")
        normalized_message = message.strip() if separator else line

        if not ERROR_MESSAGE_PATTERN.search(normalized_message):
            continue
        if any(pattern.search(normalized_message) for pattern in BENIGN_LOG_PATTERNS):
            continue

        matches.append((timestamp, normalized_message))

    preview = matches[-1][1] if matches else None
    last_seen = matches[-1][0] if matches else None
    return {
        "count": len(matches),
        "last_seen": last_seen,
        "preview": preview,
    }


def _service_readiness_url(service: PulseService) -> str | None:
    command = (service.cmd or "").strip()
    if service.port is None or "flux.runners.tokenmm.run_api" not in command:
        return None
    return f"http://127.0.0.1:{service.port}{TOKENMM_READINESS_PATH}"


def _fetch_job_readiness(service: PulseService) -> dict[str, Any] | None:
    url = _service_readiness_url(service)
    if url is None:
        return None
    try:
        with urlopen(url, timeout=READINESS_TIMEOUT_SECS) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except HTTPError as exc:
        return {
            "ok": False,
            "summary": {
                "failed_checks": ["readiness_fetch"],
                "error": f"HTTP {exc.code}",
            },
        }
    except (URLError, OSError, ValueError, json.JSONDecodeError) as exc:
        return {
            "ok": False,
            "summary": {
                "failed_checks": ["readiness_fetch"],
                "error": type(exc).__name__,
            },
        }
    if not isinstance(payload, dict):
        return {
            "ok": False,
            "summary": {
                "failed_checks": ["readiness_fetch"],
                "error": "invalid_payload",
            },
        }
    data = payload.get("data")
    if payload.get("ok") is not True or not isinstance(data, dict):
        return {
            "ok": False,
            "summary": {
                "failed_checks": ["readiness_fetch"],
                "error": "invalid_envelope",
            },
        }
    return data


def _past_tense(action: str) -> str:
    return ACTION_PAST_TENSE[action]


class PulseControlPlane:
    def __init__(
        self,
        *,
        env_dir: Path | str | None = None,
        unit_prefix: str | None = None,
        command_runner: CommandRunner | None = None,
        sudo_prefix: Sequence[str] = ("sudo",),
        self_service_id: str | None = None,
        defer_action: Callable[[Callable[[], None]], None] | None = None,
    ) -> None:
        resolved_env_dir = env_dir or os.getenv("PULSE_ENV_DIR") or "/etc/flux"
        resolved_unit_prefix = unit_prefix or os.getenv("PULSE_UNIT_PREFIX") or "flux"
        self._env_dir = Path(resolved_env_dir)
        self._unit_prefix = resolved_unit_prefix
        self._run = command_runner or cast(CommandRunner, subprocess.run)
        self._sudo_prefix = list(sudo_prefix)
        self._self_service_id = self_service_id or os.getenv("PULSE_SELF_SERVICE_ID")
        self._defer_action = defer_action or self._spawn_background_action

    def discover_services(self) -> list[PulseService]:
        if not self._env_dir.exists():
            return []

        services: list[PulseService] = []
        for env_path in sorted(self._env_dir.glob("*.env")):
            try:
                values = _parse_env_lines(env_path.read_text(encoding="utf-8").splitlines())
            except OSError:
                continue
            if values.get("PULSE_ENABLED") != "1":
                continue
            job_id = env_path.stem
            services.append(
                PulseService(
                    job_id=job_id,
                    description=values.get("PULSE_DESCRIPTION", f"Flux service {job_id}"),
                    group_key=values.get("PULSE_GROUP_KEY", job_id.split("-", 1)[0]),
                    group_label=values.get("PULSE_GROUP_LABEL", job_id.split("-", 1)[0].title()),
                    group_order=_coerce_group_order(values.get("PULSE_GROUP_ORDER")),
                    cmd=values.get("CMD"),
                    port=_coerce_port(values.get("PORT")),
                ),
            )
        return services

    def register_routes(self, app: Any) -> None:
        blueprint = Blueprint("pulse_api", __name__, url_prefix="/api/pulse")

        @blueprint.get("/jobs")
        def list_jobs() -> Any:
            services = self.discover_services()
            jobs = [self._service_payload(service) for service in services]
            return jsonify(
                {
                    "jobs": jobs,
                    "shell_links": self._shell_links(services),
                    "total": len(jobs),
                    "active": sum(1 for job in jobs if job["status"] == "active"),
                    "failed": sum(1 for job in jobs if job["status"] == "failed"),
                },
            )

        @blueprint.post("/jobs/<job_id>/<action>")
        def run_job_action(job_id: str, action: str) -> Any:
            if action not in {"start", "stop", "restart"}:
                return jsonify({"success": False, "error": f"Unsupported action: {action}"}), 404
            service = self._service_by_id(job_id)
            if service is None:
                return jsonify({"success": False, "error": f"Unknown Pulse job: {job_id}"}), 404
            return self._dispatch_action(service, action)

        @blueprint.get("/jobs/<job_id>/logs")
        def get_job_logs(job_id: str) -> Response | tuple[str, int, dict[str, str]]:
            service = self._service_by_id(job_id)
            if service is None:
                return jsonify({"success": False, "error": f"Unknown Pulse job: {job_id}"}), 404
            lines = request.args.get("lines", default=300, type=int)
            try:
                result = self._run(
                    [
                        *self._sudo_prefix,
                        "journalctl",
                        "-u",
                        self._unit_name(service.job_id),
                        "-n",
                        str(lines),
                        "--no-pager",
                    ],
                    check=True,
                    capture_output=True,
                    text=True,
                )
            except subprocess.CalledProcessError as exc:
                text = exc.stderr or exc.stdout or str(exc)
                return text, 500, {"Content-Type": "text/plain"}
            return result.stdout, 200, {"Content-Type": "text/plain"}

        @blueprint.post("/jobs/group/<group_key>/<action>")
        def run_group_action(group_key: str, action: str) -> Any:
            if action not in {"start", "stop", "restart"}:
                return jsonify({"success": False, "error": f"Unsupported action: {action}"}), 404
            services = [service for service in self.discover_services() if service.group_key == group_key]
            if not services:
                return jsonify({"success": False, "error": f"No jobs found for group '{group_key}'"}), 404
            past_tense = _past_tense(action)
            results: list[str] = []
            deferred: list[str] = []
            deferred_actions: list[Callable[[], None]] = []
            errors: list[str] = []
            for service in services:
                if self._is_self_action(service.job_id, action):
                    deferred_actions.append(self._systemctl_action(service.job_id, action))
                    deferred.append(service.job_id)
                    continue
                try:
                    self._run_systemctl_action(service.job_id, action)
                    results.append(service.job_id)
                except subprocess.CalledProcessError as exc:
                    errors.append(f"{service.job_id}: {exc.stderr or exc.stdout or str(exc)}")
            status_code = 207 if errors else (202 if deferred else 200)
            return self._json_response(
                {
                    "success": not errors,
                    "message": f"{past_tense} {len(results)} jobs in group '{group_key}'",
                    past_tense: results,
                    "pending": bool(deferred),
                    "deferred": deferred,
                    "errors": errors,
                },
                status_code,
                deferred_actions=deferred_actions,
            )

        app.register_blueprint(blueprint)

    def get_job_status(self, job_id: str) -> str:
        service = self._service_by_id(job_id)
        if service is None:
            return "unknown"
        result = self._run(
            ["systemctl", "status", self._unit_name(service.job_id)],
            capture_output=True,
            text=True,
            check=False,
        )
        return _normalize_status(result.stdout or "")

    def get_job_snapshot(self, job_id: str) -> dict[str, Any] | None:
        service = self._service_by_id(job_id)
        if service is None:
            return None
        return self._service_payload(service)

    def control_job(self, job_id: str, action: str) -> str:
        if action not in {"start", "stop", "restart"}:
            raise ValueError(f"Unsupported action: {action}")
        service = self._service_by_id(job_id)
        if service is None:
            return "unknown"
        self._run_systemctl_action(service.job_id, action)
        return self.get_job_status(service.job_id)

    def _service_by_id(self, job_id: str) -> PulseService | None:
        for service in self.discover_services():
            if service.job_id == job_id:
                return service
        return None

    def _unit_name(self, job_id: str) -> str:
        return f"{self._unit_prefix}@{job_id}"

    def _read_env_registry(self) -> dict[str, dict[str, str]]:
        registry: dict[str, dict[str, str]] = {}
        if not self._env_dir.exists():
            return registry

        for env_path in sorted(self._env_dir.glob("*.env")):
            try:
                registry[env_path.stem] = _parse_env_lines(env_path.read_text(encoding="utf-8").splitlines())
            except OSError:
                continue
        return registry

    def _shell_surfaces(self, services: Sequence[PulseService]) -> list[str]:
        surfaces: list[str] = []
        seen: set[str] = set()

        for service in services:
            surface = service.group_key.strip().lower()
            if surface not in SHELL_SURFACE_LABELS or surface in seen:
                continue
            seen.add(surface)
            surfaces.append(surface)

        env_registry = self._read_env_registry()
        common_env = env_registry.get("common", {})
        if common_env.get("EQUITIES_API_BACKEND_URL") and "equities" not in seen:
            surfaces.append("equities")

        return surfaces

    def _shell_links(self, services: Sequence[PulseService]) -> list[dict[str, str]]:
        links: list[dict[str, str]] = []
        for surface in self._shell_surfaces(services):
            surface_label = SHELL_SURFACE_LABELS[surface]
            for link_label, suffix in SHELL_LINK_SUFFIXES:
                path = surface if not suffix else f"{surface}/{suffix}"
                links.append(
                    {
                        "label": f"{surface_label} {link_label}",
                        "path": path,
                        "surface": surface,
                    },
                )
        return links

    def _service_payload(self, service: PulseService) -> dict[str, Any]:
        result = self._run(
            ["systemctl", "status", self._unit_name(service.job_id)],
            capture_output=True,
            text=True,
            check=False,
        )
        output = result.stdout or ""
        status = _normalize_status(output)
        active_since = _extract_active_since(output, status=status)
        pid_match = re.search(r"Main PID:\s+(\d+)", output)
        memory_match = re.search(r"Memory:\s+([^\s]+)", output)
        journal_cmd = [
            *self._sudo_prefix,
            "journalctl",
            "-u",
            self._unit_name(service.job_id),
        ]
        if active_since:
            journal_cmd.extend(["--since", active_since])
        journal_cmd.extend(
            [
                "-o",
                "short-iso",
                "-n",
                "300",
                "--no-pager",
            ],
        )
        logs_result = self._run(
            journal_cmd,
            capture_output=True,
            text=True,
            check=False,
        )
        readiness = _fetch_job_readiness(service) if status == "active" else None
        return {
            "id": service.job_id,
            "name": service.job_id,
            "status": status,
            "pid": int(pid_match.group(1)) if pid_match else None,
            "memory": memory_match.group(1) if memory_match else None,
            "uptime": _extract_uptime(output, status=status),
            "prefix": service.prefix,
            "group_key": service.group_key,
            "group_label": service.group_label,
            "group_order": service.group_order,
            "description": service.description,
            "cmd": service.cmd,
            "unit": self._unit_name(service.job_id),
            "errors": _extract_error_info(logs_result.stdout or ""),
            "readiness": readiness,
        }

    def _dispatch_action(self, service: PulseService, action: str) -> Any:
        if self._is_self_action(service.job_id, action):
            return self._json_response(
                {
                    "success": True,
                    "message": f"Job {service.job_id} {action} scheduled",
                    "pending": True,
                },
                202,
                deferred_actions=[self._systemctl_action(service.job_id, action)],
            )
        try:
            self._run_systemctl_action(service.job_id, action)
        except subprocess.CalledProcessError as exc:
            return (
                jsonify(
                    {
                        "success": False,
                        "error": exc.stderr or exc.stdout or str(exc),
                    },
                ),
                500,
            )
        return jsonify({"success": True, "message": f"Job {service.job_id} {_past_tense(action)}"})

    def _run_systemctl_action(self, job_id: str, action: str) -> None:
        self._run(
            [
                *self._sudo_prefix,
                "systemctl",
                action,
                self._unit_name(job_id),
            ],
            check=True,
            capture_output=True,
            text=True,
        )

    def _is_self_action(self, job_id: str, action: str) -> bool:
        return action in {"stop", "restart"} and self._self_service_id == job_id

    def _json_response(
        self,
        payload: dict[str, Any],
        status_code: int = 200,
        *,
        deferred_actions: Sequence[Callable[[], None]] = (),
    ) -> Response:
        response = jsonify(payload)
        response.status_code = status_code
        for action in deferred_actions:
            response.call_on_close(lambda deferred_action=action: self._defer_action(deferred_action))
        return response

    def _systemctl_action(self, job_id: str, action: str) -> Callable[[], None]:
        def _action() -> None:
            self._run_systemctl_action(job_id, action)

        return _action

    def _spawn_background_action(self, func: Callable[[], None]) -> None:
        def _wrapped() -> None:
            time.sleep(0.1)
            func()

        thread = threading.Thread(target=_wrapped, daemon=True)
        thread.start()
