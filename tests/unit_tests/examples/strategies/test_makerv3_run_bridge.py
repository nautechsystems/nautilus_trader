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

from __future__ import annotations

from argparse import Namespace

import pytest

from examples.live.makerv3.run_bridge import _resolve_strategy_scope


def test_resolve_strategy_scope_prefers_cli_strategy_id() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id="cli_strategy", all_strategies=False)

    resolved = _resolve_strategy_scope(config, args)

    assert resolved == "cli_strategy"


def test_resolve_strategy_scope_uses_config_strategy_id_when_cli_missing() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=None, all_strategies=False)

    resolved = _resolve_strategy_scope(config, args)

    assert resolved == "config_strategy"


def test_resolve_strategy_scope_requires_strategy_id_without_all_strategies() -> None:
    config: dict[str, dict[str, str]] = {"identity": {}}
    args = Namespace(strategy_id=None, all_strategies=False)

    with pytest.raises(ValueError, match="strategy_id"):
        _resolve_strategy_scope(config, args)


def test_resolve_strategy_scope_rejects_strategy_id_with_all_strategies() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id="cli_strategy", all_strategies=True)

    with pytest.raises(ValueError, match="all-strategies"):
        _resolve_strategy_scope(config, args)


def test_resolve_strategy_scope_all_strategies_returns_none() -> None:
    config = {"identity": {"strategy_id": "config_strategy"}}
    args = Namespace(strategy_id=None, all_strategies=True)

    resolved = _resolve_strategy_scope(config, args)

    assert resolved is None
