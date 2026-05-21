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

import os
import platform
import shutil
import subprocess
from pathlib import Path

import pytest

from nautilus_trader.common import Environment
from nautilus_trader.live import LiveExecEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import PluginConfig
from nautilus_trader.model import TraderId


def _workspace_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _cdylib_filename(name: str) -> str:
    system = platform.system()
    if system == "Windows":
        return f"{name}.dll"
    if system == "Darwin":
        return f"lib{name}.dylib"
    return f"lib{name}.so"


def _build_plugin_example(name: str) -> Path:
    root = _workspace_root()
    cargo = os.environ.get("CARGO", "cargo")
    if shutil.which(cargo) is None:
        pytest.skip("cargo is required for the Rust-native plug-in smoke test")

    subprocess.run(
        [
            cargo,
            "build",
            "-p",
            "nautilus-plugin",
            "--example",
            name,
        ],
        cwd=root,
        check=True,
    )
    artifact = _cargo_target_dir(root) / "debug" / "examples" / _cdylib_filename(name)
    assert artifact.exists()
    return artifact


def _cargo_target_dir(root: Path) -> Path:
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    if target_dir.is_absolute():
        return target_dir
    return root / target_dir


@pytest.mark.skipif(platform.system() == "Windows", reason="cdylib smoke test is Unix-only today")
def test_live_node_loads_rust_native_plugin_actor_from_python(tmp_path):
    artifact = _build_plugin_example("runtime_smoke_plugin")
    marker = tmp_path / "plugin-started.txt"
    config = LiveNodeConfig(
        environment=Environment.SANDBOX,
        trader_id=TraderId("PLUGIN-001"),
        delay_post_stop_secs=0.0,
        exec_engine=LiveExecEngineConfig(reconciliation=False),
        plugins=[
            PluginConfig(
                path=str(artifact),
                type_name="RuntimeSmokeActor",
                config={
                    "actor_id": "RuntimeSmokeActor-001",
                    "callback_path": str(marker),
                    "label": "python",
                },
            ),
        ],
    )
    node = LiveNode.build("PluginPythonSmoke", config)

    node.start()
    try:
        assert marker.read_text() == "python:on_start\n"
    finally:
        if node.is_running:
            node.stop()
