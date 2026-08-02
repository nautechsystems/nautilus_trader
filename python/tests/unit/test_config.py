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

import ast
import importlib
from pathlib import Path

from nautilus_trader import config


CONFIG_MODULE_NAMES = (
    "analysis",
    "backtest",
    "common",
    "data",
    "execution",
    "live",
    "portfolio",
    "risk",
    "trading",
)
CONFIG_NAMES_EXCLUDED = frozenset(
    {
        "BookImbalanceActorConfig",
        "CompositeMarketMakerConfig",
        "DeltaNeutralVolConfig",
        "EmaCrossConfig",
        "GridMarketMakerConfig",
        "HurstVpinDirectionalConfig",
    },
)


def test_config_reexports_curated_core_surface() -> None:
    expected = {}

    for module_name in CONFIG_MODULE_NAMES:
        module = importlib.import_module(f"nautilus_trader.{module_name}")
        expected.update(
            {
                name: value
                for name, value in vars(module).items()
                if name.endswith("Config") and name not in CONFIG_NAMES_EXCLUDED
            },
        )

    actual = {name: getattr(config, name) for name in config.__all__}

    assert config.__all__ == sorted(expected)
    assert actual == expected


def test_config_stub_matches_runtime_exports() -> None:
    stub_path = Path(config.__file__).with_suffix(".pyi")
    tree = ast.parse(stub_path.read_text())
    stub_imports = {
        alias.name for node in tree.body if isinstance(node, ast.ImportFrom) for alias in node.names
    }
    stub_exports = next(
        ast.literal_eval(node.value)
        for node in tree.body
        if isinstance(node, ast.Assign)
        and any(isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets)
    )

    assert stub_exports == config.__all__
    assert stub_imports == set(config.__all__)
