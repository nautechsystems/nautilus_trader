#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os
import sys
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import add_strategy_from_config
from _common import build_ib_live_node
from _common import default_stock_contracts
from _common import env_int
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop


def main() -> None:
    account_id = os.getenv("TWS_ACCOUNT")
    if account_id is None:
        raise RuntimeError("Set TWS_ACCOUNT to run the IB v2 reconciliation example")

    os.environ.setdefault("IB_V2_RECONCILIATION", "1")
    host, port = resolve_ib_endpoint()
    provider_config = instrument_provider_config(load_contracts=default_stock_contracts())
    node = build_ib_live_node(
        name="IB-V2-RECONCILIATION-001",
        trader_id="IB-V2-RECONCILIATION-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 1501),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 1502),
        account_id=account_id,
        provider_config=provider_config,
    )
    add_strategy_from_config(
        node,
        "ib_v2_order_strategies:IbV2SubscriptionStrategy",
    )

    print(
        f"Running v2 IB reconciliation example against {host}:{port}; press Ctrl+C to stop.",
        flush=True,
    )
    schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 20))
    node.run()


if __name__ == "__main__":
    main()
