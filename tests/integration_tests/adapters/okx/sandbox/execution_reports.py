#!/usr/bin/env python3
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
"""
Sandbox script for requesting execution reports from OKX.
"""

import asyncio

import pandas as pd

from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3


async def main():
    # Setup logging (to see Rust logs run `export RUST_LOG=debug,h2=off`)
    nautilus_pyo3.init_tracing()
    _guard = init_logging(level_stdout=LogLevel.TRACE)
    logger = Logger("okx-sandbox")

    # Setup client: we must cache all instruments we intend on using for requests
    http_client = nautilus_pyo3.OKXHttpClient.from_env()

    # Instrument type must match the symbol for the bar type
    okx_account_id = nautilus_pyo3.AccountId("OKX-master")
    okx_instrument_type = nautilus_pyo3.OKXInstrumentType.SWAP
    instruments = await http_client.request_instruments(okx_instrument_type)

    logger.info(f"Received {len(instruments)} instruments")

    for inst in instruments:
        http_client.cache_instrument(inst)

    logger.info("Cached instruments for HTTP client")

    # Request params (use the correct types for PyO3)
    start_time = pd.Timestamp("2025-07-25T00:00:00Z")  # noqa: F841 (never used)
    end_time = pd.Timestamp("2025-07-27T00:00:00Z")  # noqa: F841 (never used)

    account_state = await http_client.request_account_state(
        account_id=okx_account_id,
    )
    logger.info(f"Received {account_state}")

    order_reports = await http_client.request_order_status_reports(
        account_id=okx_account_id,
        instrument_type=okx_instrument_type,
    )
    logger.info(f"Received {len(order_reports)} order reports")

    fill_reports = await http_client.request_fill_reports(
        account_id=okx_account_id,
        instrument_type=okx_instrument_type,
    )
    logger.info(f"Received {len(fill_reports)} fill reports")

    position_reports = await http_client.request_position_status_reports(
        account_id=okx_account_id,
        instrument_type=okx_instrument_type,
    )
    logger.info(f"Received {len(position_reports)} position reports")


if __name__ == "__main__":
    asyncio.run(main())
