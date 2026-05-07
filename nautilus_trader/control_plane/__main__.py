# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import click
import msgspec

from nautilus_trader.control_plane.service import ControlPlaneRuntimeState
from nautilus_trader.control_plane.service import TraderControlPlaneService
from nautilus_trader.control_plane.service import snapshot_to_json


def _service_from_raw(raw: str | None) -> TraderControlPlaneService:
    if raw is None:
        return TraderControlPlaneService()

    runtime_state = msgspec.json.decode(raw.encode(), type=ControlPlaneRuntimeState)
    return TraderControlPlaneService(runtime_state_provider=lambda: runtime_state)


@click.group()
def main() -> None:
    """Read-only trader control-plane snapshot commands."""


@main.group("control-plane")
def control_plane() -> None:
    """Build trader-facing dashboard snapshots from read-only telemetry."""


@control_plane.command()
@click.option("--raw", help="Optional JSON encoded ControlPlaneRuntimeState telemetry.")
def dashboard(raw: str | None = None) -> None:
    """Print the complete trader dashboard snapshot as JSON."""
    click.echo(snapshot_to_json(_service_from_raw(raw).snapshot()))


@control_plane.command("source-health")
@click.option("--raw", help="Optional JSON encoded ControlPlaneRuntimeState telemetry.")
def source_health(raw: str | None = None) -> None:
    """Print source-health entries as JSON."""
    click.echo(msgspec.json.encode(_service_from_raw(raw).snapshot().source_health).decode())


@control_plane.command()
@click.option("--raw", help="Optional JSON encoded ControlPlaneRuntimeState telemetry.")
def risk(raw: str | None = None) -> None:
    """Print the risk cockpit state as JSON."""
    click.echo(msgspec.json.encode(_service_from_raw(raw).snapshot().risk_cockpit).decode())


@control_plane.command("ai-context")
@click.option("--raw", help="Optional JSON encoded ControlPlaneRuntimeState telemetry.")
def ai_context(raw: str | None = None) -> None:
    """Print deterministic AI assistant context as JSON without calling an LLM."""
    click.echo(msgspec.json.encode(_service_from_raw(raw).snapshot().ai_assistant_context).decode())


if __name__ == "__main__":
    main()
