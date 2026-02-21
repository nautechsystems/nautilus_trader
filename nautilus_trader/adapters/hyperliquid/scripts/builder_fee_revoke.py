#!/usr/bin/env python3
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
Script to revoke Nautilus builder fee approval for Hyperliquid trading.

WARNING: After revoking, you will not be able to trade on Hyperliquid via
NautilusTrader until you re-approve the builder fee.

Prerequisites:
- Set environment variable: HYPERLIQUID_PK (mainnet) or HYPERLIQUID_TESTNET_PK (testnet)

Usage:
    # Mainnet (interactive)
    python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_revoke.py

    # Mainnet (non-interactive)
    python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_revoke.py --yes

    # Testnet
    HYPERLIQUID_TESTNET=true python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_revoke.py

See: https://hyperliquid.gitbook.io/hyperliquid-docs/trading/builder-codes

"""

import sys

from nautilus_trader.core.nautilus_pyo3 import revoke_hyperliquid_builder_fee


if __name__ == "__main__":
    non_interactive = "--yes" in sys.argv or "-y" in sys.argv
    success = revoke_hyperliquid_builder_fee(non_interactive=non_interactive)
    if not success:
        sys.exit(1)
