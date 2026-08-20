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
Stream OKX market data with the built-in DataTester actor.

Connects to the OKX live environment and subscribes to the full data matrix for
one instrument: book deltas, quotes, trades, bars, mark price, index price, and
funding rates, plus historical requests. No orders are placed.

"""

from __future__ import annotations

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXDataClientFactory
from nautilus_trader.adapters.okx import OKXEnvironment
from nautilus_trader.adapters.okx import OKXInstrumentType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


OKX_ENVIRONMENT = OKXEnvironment.LIVE
TRADER_ID = TraderId.from_str("TESTER-001")
INSTRUMENT_TYPES = [OKXInstrumentType.SWAP]
INSTRUMENT_ID = InstrumentId.from_str(f"ETH-USDT-SWAP.{OKX}")
BAR_TYPE = BarType.from_str(f"{INSTRUMENT_ID}-1-MINUTE-LAST-EXTERNAL")


def main() -> None:
    node = (
        LiveNode.builder("OKX-DATA-TESTER-001", TRADER_ID, Environment.LIVE)
        .add_data_client(
            None,
            OKXDataClientFactory(),
            OKXDataClientConfig(
                instrument_types=INSTRUMENT_TYPES,
                environment=OKX_ENVIRONMENT,
            ),
        )
        .build()
    )
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(OKX),
            instrument_ids=[INSTRUMENT_ID],
            bar_types=[BAR_TYPE],
            subscribe_book_deltas=True,
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_mark_prices=True,
            subscribe_index_prices=True,
            subscribe_funding_rates=True,
            subscribe_bars=True,
            request_instruments=True,
            request_trades=True,
            request_bars=True,
            request_book_snapshot=True,
            request_funding_rates=True,
            manage_book=True,
            log_data=True,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
