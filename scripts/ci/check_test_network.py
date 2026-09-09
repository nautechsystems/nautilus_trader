#!/usr/bin/env python3
# Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
# Licensed under the GNU Lesser General Public License Version 3.0.
# See https://www.gnu.org/licenses/lgpl-3.0.en.html for license details.
"""
Flag common signs of live network access in Rust and Python tests.
"""

import ipaddress
import re
import sys
from pathlib import Path
from urllib.parse import urlsplit


CALLS = re.compile(
    r"\b(?:get|post|put|patch|delete|head|options|urlopen|connect(?:_async(?:_with_config)?|_ex)?"
    r"|create_connection)\s*\(\s*(?:\(\s*)?[&rfb]*[\"']"
    r"(?P<address>(?:https?|wss?)://[^\"'\s]+)",
)
SOCKETS = re.compile(
    r"\b(?:connect|connect_ex|create_connection)\s*\(\s*(?:\(\s*)?[&]*[\"']"
    r"(?P<address>[^\"'\s]*[.:][^\"'\s]*)",
)
LIVE = re.compile(r"[\"'][A-Z_]*(?:RUN_LIVE|LIVE_TEST|RUN_NETWORK_TEST|FORK_TESTS)[A-Z_]*[\"']")
FORK = re.compile(
    r"--fork-(?:url|rpc-url)\b|\bfork_(?:url|rpc_url)\s*\(|[\"'][A-Z_]*FORK_RPC_URL[A-Z_]*[\"']",
)
TEST_MODULE = re.compile(r"^\s*(?:pub\s+)?mod\s+(?:tests?|test_\w+|\w+_tests)\s*\{", re.MULTILINE)


def findings(text: str) -> list[tuple[int, str]]:
    """
    Find direct literal destinations and explicit live-test opt-ins.
    """
    # Ignore whole-line comments while preserving diagnostic line numbers
    text = "\n".join(
        "" if line.lstrip().startswith(("//", "#")) else line for line in text.splitlines()
    )
    found = set()
    for pattern in (CALLS, SOCKETS):
        for match in pattern.finditer(text):
            if not _local_address(match["address"]):
                found.add((text.count("\n", 0, match.start()) + 1, "non-local network call"))
    for rule, pattern in (("live-test switch", LIVE), ("fork-RPC option", FORK)):
        for match in pattern.finditer(text):
            found.add((text.count("\n", 0, match.start()) + 1, rule))
    return sorted(found)


def _local_address(address: str) -> bool:
    try:
        host = (
            address
            if address.count(":") > 1 and "://" not in address and "[" not in address
            else urlsplit(address if "://" in address else "//" + address).hostname
        )
    except ValueError:
        return False
    if not host or "{" in host:
        return True  # Dynamic destinations need code review
    if host in {"localhost", "example.com", "example.net", "example.org"} or host.endswith(
        (
            ".localhost",
            ".invalid",
            ".test",
            ".example",
            ".example.com",
            ".example.net",
            ".example.org",
        ),
    ):
        return True
    try:
        ip = ipaddress.ip_address(host)
        return (getattr(ip, "ipv4_mapped", None) or ip).is_loopback
    except ValueError:
        return False


def check(root: Path) -> list[str]:
    """
    Check test directories and the tail of Rust files containing a test module.
    """
    paths = sorted((root / "crates").rglob("*.rs"))
    for directory in ("tests", "memray_tests"):
        paths.extend(sorted((root / "python" / directory).rglob("*.py")))
    errors = []
    for path in paths:
        relative = path.relative_to(root)
        if "test_data" in relative.parts:
            continue
        text = path.read_text(encoding="utf-8")
        offset = 0
        if path.suffix == ".rs" and "tests" not in relative.parts and path.name != "tests.rs":
            module = TEST_MODULE.search(text)
            if module is None:
                continue
            offset = text.count("\n", 0, module.start())
            text = text[module.start() :]
        errors.extend(f"{relative}:{line + offset}: {rule}" for line, rule in findings(text))
    return errors


if __name__ == "__main__":
    errors = check(Path(__file__).resolve().parents[2])
    for error in errors:
        sys.stderr.write(f"{error}\n")
    sys.exit(bool(errors))
