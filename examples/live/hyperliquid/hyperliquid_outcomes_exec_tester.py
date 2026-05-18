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

from decimal import Decimal

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveExecClientFactory
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


# Environment variables required:
# Mainnet: HYPERLIQUID_PK (and optionally HYPERLIQUID_VAULT)
# Testnet: HYPERLIQUID_TESTNET_PK (and optionally HYPERLIQUID_TESTNET_VAULT)
#
# Agent / API wallets: if your private key is an agent wallet approved under a
# master account, also set HYPERLIQUID_ACCOUNT_ADDRESS to the master account
# address. See docs: integrations/hyperliquid.md#agent-wallets


# HIP-4 outcome targeted exec tester. The BTC daily binary
# `+{10*outcome+side}.HYPERLIQUID` is the venue's most active outcome market
# and settles each day at 06:00 UTC. Outcomes are fully-collateralized in
# [0, 1]: the maximum loss per contract on the Yes side is the limit price
# paid.
#
# The outcome universe cycles: the index in `outcomeMeta` advances with each
# new settlement. The default below points at the current BTC daily at the
# time of writing; switch `outcome_index` / `outcome_side` to target a
# different outcome from the current snapshot. Run
# `curl -s -X POST https://api.hyperliquid.xyz/info -d '{"type":"outcomeMeta"}'`
# to inspect the live universe.
#
# Outcome-specific ExecTester tuning:
# - `tob_offset_ticks` defaults to 500 (works for high-priced perps), which
#   becomes a $0.05 offset on a 0.0001-tick outcome and pushes buy prices
#   below zero. Use a small offset (5 ticks => $0.0005) so passive buys
#   stay positive and rest as makers.
# - The venue enforces a minimum order notional (10 USDH); pick `order_qty`
#   such that `order_qty * limit_price >= 10`. At a Yes-side mid near 0.02
#   that needs ~500 contracts.

testnet = False  # Set to True for testnet, False for mainnet
outcome_index = 25  # Outcome index from outcomeMeta
outcome_side = 0  # 0 = Yes, 1 = No

# HIP-4 encoding: 10 * outcome + side
encoding = 10 * outcome_index + outcome_side
symbol = f"+{encoding}"
instrument_id = InstrumentId.from_str(f"{symbol}.{HYPERLIQUID}")

# Default sized for the BTC daily Yes side (`+50`). Notional must clear the
# venue-enforced 10 USDH minimum after `tob_offset_ticks` shifts the limit
# below the bid; sizing for ~3x the minimum at the prevailing mid leaves
# headroom for intraday drift (a 0.02 mid with the 5-tick offset lands at
# 0.0195, so 2000 contracts give ~39 USDH and stay above the minimum even
# if the mid halves). Adjust to `ceil(10 / target_price) + headroom` when
# targeting a different market. Outcomes do not support reduce-only or
# trigger orders; the validator rejects those at submission.
order_qty = Decimal(2000)


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
        open_check_interval_secs=15.0,
        open_check_threshold_ms=10_000,
        open_check_open_only=False,
        open_check_lookback_mins=60,
        purge_closed_orders_interval_mins=15,
        purge_closed_orders_buffer_mins=60,
        purge_closed_positions_interval_mins=15,
        purge_closed_positions_buffer_mins=60,
        purge_account_events_interval_mins=15,
        purge_account_events_lookback_mins=60,
        graceful_shutdown_on_exception=True,
    ),
    data_clients={
        HYPERLIQUID: HyperliquidDataClientConfig(
            environment=HyperliquidEnvironment.TESTNET
            if testnet
            else HyperliquidEnvironment.MAINNET,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=(
                HyperliquidProductType.SPOT,
                HyperliquidProductType.PERP,
                HyperliquidProductType.OUTCOME,
            ),
        ),
    },
    exec_clients={
        HYPERLIQUID: HyperliquidExecClientConfig(
            environment=HyperliquidEnvironment.TESTNET
            if testnet
            else HyperliquidEnvironment.MAINNET,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=(
                HyperliquidProductType.SPOT,
                HyperliquidProductType.PERP,
                HyperliquidProductType.OUTCOME,
            ),
            normalize_prices=True,  # Rounds prices to 5 significant figures (required for HL)
        ),
    },
    timeout_connection=30.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=10.0,
)

node = TradingNode(config=config_node)

# Outcome side tokens behave like spot: no reduce_only, no triggers, no
# inventory if we haven't bought yet. enable_limit_sells stays False so the
# tester only places passive buys; if a fill arrives, the resulting long
# position can be flattened manually or held to settlement.
strat_config = ExecTesterConfig(
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    order_qty=order_qty,
    tob_offset_ticks=5,
    enable_limit_buys=True,
    enable_limit_sells=False,
    use_post_only=True,
    reduce_only_on_stop=False,
    manage_stop=False,
    market_exit_reduce_only=False,
    cancel_orders_on_stop=True,
    log_data=False,
)

strategy = ExecTester(config=strat_config)

node.trader.add_strategy(strategy)

node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
node.add_exec_client_factory(HYPERLIQUID, HyperliquidLiveExecClientFactory)
node.build()


if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
