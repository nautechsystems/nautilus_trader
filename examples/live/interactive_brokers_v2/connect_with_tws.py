#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os

from _common import add_strategy_from_config
from _common import build_ib_live_node
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop


def main() -> None:
    host, port = resolve_ib_endpoint()
    account_id = os.getenv("TWS_ACCOUNT") if env_bool("IB_V2_ENABLE_EXECUTION") else None
    os.environ.setdefault("IB_V2_SUBSCRIBE_INDEX_PRICES", "1")
    subscription_id = os.getenv("IB_V2_SUBSCRIPTION_INSTRUMENT_ID", "^SPX.CBOE")
    provider_config = instrument_provider_config(
        load_ids=[
            subscription_id,
        ],
    )

    node = build_ib_live_node(
        name="IB-V2-TWS-001",
        trader_id="IB-V2-TWS-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 1301),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 1302),
        account_id=account_id,
        provider_config=provider_config,
    )
    add_strategy_from_config(
        node,
        "ib_v2_order_strategies:IbV2SubscriptionStrategy",
    )

    print(f"Running v2 IB node against {host}:{port}; press Ctrl+C to stop.", flush=True)
    schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 0))
    node.run()


if __name__ == "__main__":
    main()
