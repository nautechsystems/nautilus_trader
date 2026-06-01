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

import importlib.util
import sys
from pathlib import Path
from types import ModuleType
from typing import Any
from typing import ClassVar


_EXAMPLES_DIR = Path(__file__).resolve().parents[3] / "examples"


def load_example_module(adapter: str, module: str) -> ModuleType:
    module_name = f"{adapter}_{module}_example"
    module_path = _EXAMPLES_DIR / adapter / f"{module}.py"
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    assert spec is not None
    assert spec.loader is not None
    loaded_module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = loaded_module
    spec.loader.exec_module(loaded_module)
    return loaded_module


class _CaptureNode:
    def __init__(self, captured: dict[str, object]) -> None:
        self._captured = captured

    def add_native_actor(self, config: object) -> None:
        self._captured["actor_config"] = config

    def add_native_strategy(self, config: object) -> None:
        self._captured["strategy_config"] = config

    def run(self) -> None:
        self._captured["run_called"] = True


class _CaptureBuilder:
    def __init__(self, captured: dict[str, object]) -> None:
        self._captured = captured

    def with_reconciliation(self, reconciliation: bool) -> "_CaptureBuilder":
        self._captured["reconciliation"] = reconciliation
        return self

    def with_risk_engine_config(self, config: object) -> "_CaptureBuilder":
        self._captured["risk_engine_config"] = config
        return self

    def add_data_client(self, *args: object) -> "_CaptureBuilder":
        self._captured["data_client_args"] = args
        return self

    def add_exec_client(self, *args: object) -> "_CaptureBuilder":
        self._captured["exec_client_args"] = args
        return self

    def add_simulated_exec_client(self, *args: object) -> "_CaptureBuilder":
        self._captured["simulated_exec_client_args"] = args
        return self

    def build(self) -> _CaptureNode:
        return _CaptureNode(self._captured)


class _CaptureLiveNode:
    captured: ClassVar[dict[str, object]]

    @staticmethod
    def builder(name: str, trader_id: object, environment: object) -> _CaptureBuilder:
        _CaptureLiveNode.captured["builder_args"] = (name, trader_id, environment)
        return _CaptureBuilder(_CaptureLiveNode.captured)


class _CaptureExecTesterConfig:
    captured: ClassVar[dict[str, object]]

    def __init__(self, **kwargs: object) -> None:
        self.captured["exec_tester_kwargs"] = kwargs


class _CaptureDataTesterConfig:
    captured: ClassVar[dict[str, object]]

    def __init__(self, **kwargs: object) -> None:
        self.captured["data_tester_kwargs"] = kwargs


def capture_exec_tester_main(
    monkeypatch: Any,
    module: ModuleType,
    extra_args: list[str],
) -> dict[str, object]:
    captured: dict[str, object] = {}
    _CaptureExecTesterConfig.captured = captured
    _CaptureLiveNode.captured = captured

    monkeypatch.setattr(sys, "argv", ["exec_tester.py", *extra_args])
    monkeypatch.setattr(module, "ExecTesterConfig", _CaptureExecTesterConfig)
    monkeypatch.setattr(module, "LiveNode", _CaptureLiveNode)

    module.main()
    return captured


def capture_data_tester_main(
    monkeypatch: Any,
    module: ModuleType,
    extra_args: list[str],
) -> dict[str, object]:
    captured: dict[str, object] = {}
    _CaptureDataTesterConfig.captured = captured
    _CaptureLiveNode.captured = captured

    monkeypatch.setattr(sys, "argv", ["data_tester.py", *extra_args])
    monkeypatch.setattr(module, "DataTesterConfig", _CaptureDataTesterConfig)
    monkeypatch.setattr(module, "LiveNode", _CaptureLiveNode)

    module.main()
    return captured
