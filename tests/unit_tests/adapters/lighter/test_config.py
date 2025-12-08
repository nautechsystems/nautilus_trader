# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter.constants import (
    ENV_ACCOUNT_INDEX,
    ENV_ACCOUNT_INDEX_TESTNET,
    ENV_API_KEY_PRIVATE_KEY,
    ENV_API_KEY_PRIVATE_KEY_TESTNET,
)


def test_resolved_api_key_prefers_explicit() -> None:
    config = LighterExecClientConfig(api_key_private_key="abc123")
    assert config.resolved_api_key_private_key == "abc123"


def test_resolved_api_key_reads_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ENV_API_KEY_PRIVATE_KEY, "from_env")
    config = LighterExecClientConfig(api_key_private_key=None, testnet=False)
    assert config.resolved_api_key_private_key == "from_env"


def test_resolved_api_key_reads_testnet_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ENV_API_KEY_PRIVATE_KEY_TESTNET, "from_env_testnet")
    config = LighterExecClientConfig(api_key_private_key=None, testnet=True)
    assert config.resolved_api_key_private_key == "from_env_testnet"


def test_resolved_account_index_prefers_explicit() -> None:
    config = LighterDataClientConfig(account_index=7)
    assert config.resolved_account_index == 7


def test_resolved_account_index_reads_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ENV_ACCOUNT_INDEX, "5")
    config = LighterDataClientConfig(account_index=None, testnet=False)
    assert config.resolved_account_index == 5


def test_resolved_account_index_reads_testnet_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ENV_ACCOUNT_INDEX_TESTNET, "6")
    config = LighterDataClientConfig(account_index=None, testnet=True)
    assert config.resolved_account_index == 6


def test_resolved_account_index_raises_on_invalid_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ENV_ACCOUNT_INDEX, "not-an-int")
    config = LighterDataClientConfig(account_index=None, testnet=False)
    with pytest.raises(ValueError):
        _ = config.resolved_account_index
