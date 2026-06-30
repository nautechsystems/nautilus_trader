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
"""
Datadog dashboard publishing helpers.

Usage
-----
Publish the bundled dashboard using environment credentials:

``python -m nautilus_trader.datadog.dashboard``

Required environment variables are ``DD_API_KEY`` and ``DD_APP_KEY``. Set
``DD_SITE`` for non-US1 sites, for example ``datadoghq.eu``.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any
from urllib.error import HTTPError
from urllib.request import Request
from urllib.request import urlopen


DEFAULT_DASHBOARD_NAME = "ops"
DEFAULT_DASHBOARD_FILE = "dashboards/nautilus_trading_ops.json"
DEFAULT_SITE = "datadoghq.com"
DASHBOARD_FILES = {
    "ops": DEFAULT_DASHBOARD_FILE,
    "dev": "dashboards/nautilus_trading_ops_dev.json",
}
PACKAGE_DIR = Path(__file__).resolve().parent


def load_dashboard(
    path: str | Path | None = None,
    *,
    name: str = DEFAULT_DASHBOARD_NAME,
) -> dict[str, Any]:
    """
    Load a Datadog dashboard definition.
    """
    if path is not None:
        return json.loads(Path(path).read_text(encoding="utf-8"))

    try:
        dashboard_file = DASHBOARD_FILES[name]
    except KeyError as exc:
        valid_names = ", ".join(sorted(DASHBOARD_FILES))
        raise ValueError(
            f"Unknown dashboard name {name!r}; expected one of: {valid_names}"
        ) from exc

    dashboard = PACKAGE_DIR / dashboard_file
    return json.loads(dashboard.read_text(encoding="utf-8"))


def publish_dashboard(
    dashboard: dict[str, Any] | None = None,
    *,
    api_key: str | None = None,
    app_key: str | None = None,
    site: str | None = None,
    dashboard_id: str | None = None,
    timeout: float = 10.0,
) -> dict[str, Any]:
    """
    Publish or update a dashboard using the Datadog Dashboards API.
    """
    api_key = api_key or os.getenv("DD_API_KEY")
    app_key = app_key or os.getenv("DD_APP_KEY") or os.getenv("DD_APPLICATION_KEY")
    site = site or os.getenv("DD_SITE", DEFAULT_SITE)
    dashboard_id = dashboard_id or os.getenv("DD_DASHBOARD_ID")

    if not api_key:
        raise ValueError("Datadog API key not provided; set DD_API_KEY")
    if not app_key:
        raise ValueError("Datadog application key not provided; set DD_APP_KEY")

    body = dashboard or load_dashboard()
    payload = json.dumps(body).encode("utf-8")
    request = Request(  # noqa: S310
        url=_dashboard_api_url(site, dashboard_id),
        data=payload,
        method="PUT" if dashboard_id else "POST",
        headers={
            "Content-Type": "application/json",
            "DD-API-KEY": api_key,
            "DD-APPLICATION-KEY": app_key,
        },
    )

    try:
        with urlopen(request, timeout=timeout) as response:  # noqa: S310
            return json.loads(response.read().decode("utf-8"))
    except HTTPError as exc:
        details = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"Datadog dashboard publish failed: {exc.code} {details}") from exc


def _dashboard_api_url(site: str, dashboard_id: str | None = None) -> str:
    if site.startswith(("http://", "https://")):
        url = f"{site.rstrip('/')}/api/v1/dashboard"
    else:
        url = f"https://api.{site}/api/v1/dashboard"

    if dashboard_id:
        return f"{url}/{dashboard_id}"

    return url


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Publish the Nautilus Datadog dashboard")
    parser.add_argument(
        "--dashboard",
        type=Path,
        default=None,
        help="Path to a dashboard JSON file; defaults to the bundled trading ops dashboard",
    )
    parser.add_argument(
        "--name",
        choices=sorted(DASHBOARD_FILES),
        default=DEFAULT_DASHBOARD_NAME,
        help="Bundled dashboard to publish when --dashboard is not provided",
    )
    parser.add_argument(
        "--site",
        default=None,
        help="Datadog site, for example datadoghq.com or datadoghq.eu",
    )
    parser.add_argument(
        "--dashboard-id",
        default=None,
        help="Existing Datadog dashboard id to update instead of creating a new dashboard",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the dashboard JSON instead of publishing",
    )
    args = parser.parse_args(argv)

    dashboard = load_dashboard(args.dashboard, name=args.name)
    if args.dry_run:
        print(json.dumps(dashboard, indent=2, sort_keys=True))
        return 0

    result = publish_dashboard(dashboard, site=args.site, dashboard_id=args.dashboard_id)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
