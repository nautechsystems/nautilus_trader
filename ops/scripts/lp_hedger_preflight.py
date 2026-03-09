#!/usr/bin/env python3
from __future__ import annotations

import argparse
import configparser
import ipaddress
import json
import sys
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit


EXPECTED_LP_API_PORT = 5025
EXPECTED_PUBLIC_PORT = 5022
REQUIRED_SYSTEM_SECTIONS = (
    "redis",
    "plume",
    "bybit",
    "bybit_hedger",
    "bybit_hedger_band2",
)


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _default_band1_config() -> Path:
    return _repo_root() / "deploy/lp/hedgers/eth_plume_lp_hedger.ini"


def _default_band2_config() -> Path:
    return _repo_root() / "deploy/lp/hedgers/eth_plume_lp_hedger_band2.ini"


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate shared-host LP hedger rollout prerequisites.",
    )
    parser.add_argument(
        "--common-env",
        type=Path,
        default=Path("/etc/flux/common.env"),
        help="Path to the shared-host common.env file.",
    )
    parser.add_argument(
        "--system-ini",
        type=Path,
        default=Path("/etc/flux/lp-system.ini"),
        help="Path to the LP system INI file.",
    )
    parser.add_argument(
        "--band1-config",
        type=Path,
        default=_default_band1_config(),
        help="Path to the Band1 hedger INI file.",
    )
    parser.add_argument(
        "--band2-config",
        type=Path,
        default=_default_band2_config(),
        help="Path to the Band2 hedger INI file.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit machine-readable JSON instead of a human-readable summary.",
    )
    return parser.parse_args()


def _parse_env_file(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    with path.open(encoding="utf-8") as handle:
        for raw_line in handle:
            line = raw_line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, value = line.split("=", 1)
            values[key.strip()] = value.strip()
    return values


def _is_loopback_host(host: str) -> bool:
    text = host.strip().lower()
    if text == "localhost":
        return True
    try:
        return ipaddress.ip_address(text).is_loopback
    except ValueError:
        return False


def _validate_readable_file(path: Path, *, label: str, errors: list[str]) -> None:
    if not path.is_file():
        errors.append(f"{label} is missing: {path}")
        return
    try:
        path.read_text(encoding="utf-8")
    except OSError as exc:
        errors.append(f"{label} is not readable: {path} ({exc})")


def _validate_backend_url(raw_url: str | None, *, errors: list[str], warnings: list[str]) -> None:
    if not raw_url:
        errors.append("common.env is missing LP_API_BACKEND_URL")
        return

    parsed = urlsplit(raw_url)
    if parsed.scheme not in {"http", "https"}:
        errors.append(f"LP_API_BACKEND_URL must use http or https: {raw_url}")
    if not parsed.hostname or not _is_loopback_host(parsed.hostname):
        errors.append(f"LP_API_BACKEND_URL must target a loopback host: {raw_url}")
    if parsed.port != EXPECTED_LP_API_PORT:
        errors.append(
            f"LP_API_BACKEND_URL must use port {EXPECTED_LP_API_PORT}: {raw_url}",
        )
    if parsed.port == EXPECTED_PUBLIC_PORT:
        errors.append(
            "LP_API_BACKEND_URL must not reuse the shared public host port 5022",
        )
    if parsed.scheme == "https":
        warnings.append("LP_API_BACKEND_URL uses https on loopback; verify local TLS termination")


def _validate_system_ini(path: Path, *, errors: list[str]) -> None:
    parser = configparser.ConfigParser()
    try:
        with path.open(encoding="utf-8") as handle:
            parser.read_file(handle)
    except OSError as exc:
        errors.append(f"lp-system.ini is not readable: {path} ({exc})")
        return
    except configparser.Error as exc:
        errors.append(f"lp-system.ini is invalid: {path} ({exc})")
        return

    missing = [section for section in REQUIRED_SYSTEM_SECTIONS if not parser.has_section(section)]
    if missing:
        missing_sections = ", ".join(f"[{section}]" for section in missing)
        errors.append(f"lp-system.ini is missing required sections: {missing_sections}")


def _build_report(args: argparse.Namespace) -> dict[str, Any]:
    errors: list[str] = []
    warnings: list[str] = []

    _validate_readable_file(args.common_env, label="common.env", errors=errors)
    _validate_readable_file(args.system_ini, label="lp-system.ini", errors=errors)
    _validate_readable_file(args.band1_config, label="Band1 config", errors=errors)
    _validate_readable_file(args.band2_config, label="Band2 config", errors=errors)

    common_env_values: dict[str, str] = {}
    if not errors:
        common_env_values = _parse_env_file(args.common_env)
    elif args.common_env.is_file():
        common_env_values = _parse_env_file(args.common_env)

    _validate_backend_url(
        common_env_values.get("LP_API_BACKEND_URL"),
        errors=errors,
        warnings=warnings,
    )
    if args.system_ini.is_file():
        _validate_system_ini(args.system_ini, errors=errors)

    return {
        "ok": not errors,
        "errors": errors,
        "warnings": warnings,
        "checks": {
            "common_env": str(args.common_env),
            "system_ini": str(args.system_ini),
            "band1_config": str(args.band1_config),
            "band2_config": str(args.band2_config),
            "required_system_sections": list(REQUIRED_SYSTEM_SECTIONS),
            "expected_lp_api_port": EXPECTED_LP_API_PORT,
            "expected_public_port": EXPECTED_PUBLIC_PORT,
            "lp_api_backend_url": common_env_values.get("LP_API_BACKEND_URL"),
        },
    }


def _print_human_report(report: dict[str, Any]) -> None:
    status = "OK" if report["ok"] else "FAILED"
    print(f"LP hedger preflight: {status}")
    print(f"  common.env: {report['checks']['common_env']}")
    print(f"  lp-system.ini: {report['checks']['system_ini']}")
    print(f"  Band1 config: {report['checks']['band1_config']}")
    print(f"  Band2 config: {report['checks']['band2_config']}")
    print(f"  LP_API_BACKEND_URL: {report['checks']['lp_api_backend_url'] or '<missing>'}")
    if report["warnings"]:
        print("Warnings:")
        for warning in report["warnings"]:
            print(f"  - {warning}")
    if report["errors"]:
        print("Errors:")
        for error in report["errors"]:
            print(f"  - {error}")


def main() -> int:
    args = _parse_args()
    report = _build_report(args)
    if args.json:
        json.dump(report, sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write("\n")
    else:
        _print_human_report(report)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
