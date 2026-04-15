#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os
import socket


LOCALHOSTS = frozenset({"127.0.0.1", "localhost"})
DEFAULT_IB_PORT_CANDIDATES = (7497, 4002, 7496, 4001)


def is_ib_endpoint_reachable(host: str, port: int, timeout: float = 0.25) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def resolve_ib_endpoint(
    host_env_var: str,
    port_env_var: str,
    *,
    default_host: str = "127.0.0.1",
    default_port: int = 7497,
    candidate_ports: tuple[int, ...] = DEFAULT_IB_PORT_CANDIDATES,
) -> tuple[str, int]:
    host = os.getenv(host_env_var, default_host)

    port_value = os.getenv(port_env_var)
    if port_value is not None:
        return host, int(port_value)

    if host not in LOCALHOSTS:
        return host, default_port

    for port in candidate_ports:
        if is_ib_endpoint_reachable(host, port):
            return host, port

    return host, default_port
