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
Script to verify builder fee approval status for Hyperliquid.

Queries Hyperliquid to check if your wallet has approved the Nautilus builder fee.

Prerequisites:
- Set environment variable: HYPERLIQUID_PK (mainnet) or HYPERLIQUID_TESTNET_PK (testnet)
- Or provide a wallet address as an argument

Usage:
    # Check using private key from environment
    python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_verify.py

    # Check a specific wallet address
    python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_verify.py 0x1234...

    # Testnet
    HYPERLIQUID_TESTNET=true python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_verify.py

"""

import sys

from nautilus_trader.core.nautilus_pyo3 import verify_hyperliquid_builder_fee


if __name__ == "__main__":
    wallet_address = sys.argv[1] if len(sys.argv) > 1 else None
    is_approved = verify_hyperliquid_builder_fee(wallet_address=wallet_address)
    if not is_approved:
        sys.exit(1)
