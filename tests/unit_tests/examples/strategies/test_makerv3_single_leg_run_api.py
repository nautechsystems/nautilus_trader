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

from examples.live.makerv3_single_leg.run_api import DEFAULT_CONFIG_PATH
from examples.live.makerv3_single_leg.run_api import _build_flux_config
from examples.live.makerv3_single_leg.run_api import _load_config
from examples.live.makerv3_single_leg.run_api import _resolve_bind_host


def test_default_config_builds_flux_config_with_strategy_identity_uniqueness() -> None:
    config = _load_config(DEFAULT_CONFIG_PATH)

    flux_config = _build_flux_config(config, mode="paper", confirm_live=True)

    assert flux_config.identity.strategy_instance_id == flux_config.identity.strategy_id


def test_default_config_api_host_is_localhost() -> None:
    config = _load_config(DEFAULT_CONFIG_PATH)

    host = _resolve_bind_host(config, Namespace(host=None))

    assert host == "127.0.0.1"


def test_resolve_bind_host_defaults_to_localhost_when_missing() -> None:
    host = _resolve_bind_host({"api": {}}, Namespace(host=None))

    assert host == "127.0.0.1"


def test_resolve_bind_host_prefers_cli_override() -> None:
    host = _resolve_bind_host({"api": {"host": "127.0.0.1"}}, Namespace(host="0.0.0.0"))

    assert host == "0.0.0.0"
