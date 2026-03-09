#!/usr/bin/env python3
"""
Script to revoke Nautilus builder fee approval for Hyperliquid trading.

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
