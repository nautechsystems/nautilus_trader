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
Stream CoinAPI market data with the built-in DataTester actor.

Running this example loads the configured instrument from CoinAPI, then streams quotes, trades,
and one-minute bars and requests the last hour of bars, logging all received data. No orders are
placed.

Requires `COINAPI_API_KEY`. Connecting spends one REST credit on filtered symbol metadata, and
each historical request spends credits per 100 returned records within `CREDIT_BUDGET`.

"""

from __future__ import annotations

from nautilus_trader.adapters.coinapi import COINAPI_CLIENT_ID
from nautilus_trader.adapters.coinapi import CoinApiDataClientConfig
from nautilus_trader.adapters.coinapi import CoinApiDataClientFactory
from nautilus_trader.adapters.coinapi import CoinApiInstrumentSelection
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


TRADER_ID = TraderId.from_str("TESTER-001")
EXCHANGE_ID = "COINBASE"
SYMBOL_ID = "COINBASE_SPOT_BTC_USD"
INSTRUMENT_ID = InstrumentId.from_str("BTC-USD.COINBASE")

# Some captured BTC OHLCV prices use three decimals, finer than CoinAPI's two-decimal metadata
PRICE_PRECISION = 3
BAR_TYPE = BarType.from_str(f"{INSTRUMENT_ID}-1-MINUTE-LAST-EXTERNAL")
CREDIT_BUDGET = 10

# A connection streams one book channel, so enable deltas or depth, not both
SUBSCRIBE_BOOK_DELTAS = False
SUBSCRIBE_BOOK_DEPTH = False


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder("COINAPI-DATA-TESTER-001", TRADER_ID, Environment.LIVE)
        .add_data_client(
            None,
            CoinApiDataClientFactory(),
            CoinApiDataClientConfig(
                instruments=[
                    CoinApiInstrumentSelection(
                        EXCHANGE_ID,
                        [INSTRUMENT_ID],
                        filter_symbol_id=SYMBOL_ID,
                    ),
                ],
                price_precisions={INSTRUMENT_ID: PRICE_PRECISION},
                credit_budget=CREDIT_BUDGET,
            ),
        )
        .build()
    )
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=COINAPI_CLIENT_ID,
            instrument_ids=[INSTRUMENT_ID],
            bar_types=[BAR_TYPE],
            subscribe_book_deltas=SUBSCRIBE_BOOK_DELTAS,
            subscribe_book_depth=SUBSCRIBE_BOOK_DEPTH,
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_bars=True,
            request_bars=True,
            manage_book=True,
            log_data=True,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
