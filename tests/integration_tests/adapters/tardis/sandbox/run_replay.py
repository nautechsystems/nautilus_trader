"""
Utility script to launch a local Tardis-Machine replay from Python.

The location of the example JSON configuration file changed when the Rust
crates were moved out of the Python package directory in April-2025.  The
script now references the new path directly (``crates/adapters/tardis/bin``).

To start a server first run (example):

    docker run -p 8000:8000 -p 8001:8001 \
        -e "TM_API_KEY=YOUR_API_KEY" \
        -d tardisdev/tardis-machine

Then execute this file:

    python tests/integration_tests/adapters/tardis/sandbox/run_replay.py

Export ``NAUTILUS_LOG=debug`` for verbose Rust logging (defaults to INFO).

"""

from __future__ import annotations

import asyncio
import os

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core import nautilus_pyo3


def _resolve_config_filepath() -> str:
    config_filepath = os.path.join(
        str(PACKAGE_ROOT),
        "crates",
        "adapters",
        "tardis",
        "bin",
        "example_config.json",
    )
    if not os.path.isfile(config_filepath):
        raise FileNotFoundError(f"Unable to locate example_config.json at {config_filepath}")
    return config_filepath


async def run() -> None:
    config_filepath = _resolve_config_filepath()
    await nautilus_pyo3.run_tardis_machine_replay(config_filepath)


if __name__ == "__main__":
    asyncio.run(run())
