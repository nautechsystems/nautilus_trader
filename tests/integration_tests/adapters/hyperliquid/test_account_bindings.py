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

import pytest

from nautilus_trader.core import nautilus_pyo3


def test_resolve_execution_account_address_binding_prefers_explicit_account():
    # Arrange
    account_address = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

    # Act
    resolved = nautilus_pyo3.hyperliquid_resolve_execution_account_address(
        private_key=None,
        vault_address=" 0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ",
        account_address=f" {account_address} ",
        environment=nautilus_pyo3.HyperliquidEnvironment.MAINNET,
    )

    # Assert
    assert resolved == account_address


def test_resolve_execution_account_address_binding_uses_vault_fallback():
    # Arrange
    vault_address = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

    # Act
    resolved = nautilus_pyo3.hyperliquid_resolve_execution_account_address(
        private_key=None,
        vault_address=f" {vault_address} ",
        account_address=None,
        environment=nautilus_pyo3.HyperliquidEnvironment.MAINNET,
    )

    # Assert
    assert resolved == vault_address


def test_resolve_execution_account_address_binding_rejects_invalid_vault():
    # Arrange, Act, Assert
    with pytest.raises(ValueError, match="Vault address must be 20 bytes of valid hex"):
        nautilus_pyo3.hyperliquid_resolve_execution_account_address(
            private_key=None,
            vault_address="0xinvalid",
            account_address=None,
            environment=nautilus_pyo3.HyperliquidEnvironment.MAINNET,
        )
