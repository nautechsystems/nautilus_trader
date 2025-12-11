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

from __future__ import annotations

import asyncio
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter.execution import LighterExecutionClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.common.providers import InstrumentProviderConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


class _DummyInstrumentProvider(InstrumentProvider):
    def __init__(self, instrument, market_index: int = 1) -> None:
        super().__init__(config=InstrumentProviderConfig())
        self._instrument = instrument
        self._market_index = market_index
        self._market_index_by_instrument = {instrument.id.value: market_index}
        self.add(instrument)

    def market_index_for(self, instrument_id: InstrumentId) -> int | None:
        return self._market_index if instrument_id == self._instrument.id else None


@pytest.mark.asyncio
async def test_submit_cancel_lifecycle_with_fixtures(btc_instrument):
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TestExecStubs.limit_order().trader_id, clock=clock)
    cache = TestComponentStubs.cache()
    provider = _DummyInstrumentProvider(btc_instrument)

    with open("tests/test_data/lighter/http/mainnet_next_nonce.json") as f:
        next_nonce = json.load(f)["response"]["body"]
    with open("tests/test_data/lighter/http/mainnet_sendtx_create_btc.json") as f:
        create_resp = json.load(f)["response"]["body"]
    with open("tests/test_data/lighter/http/mainnet_sendtx_cancel_btc.json") as f:
        cancel_resp = json.load(f)["response"]["body"]

    http = AsyncMock()
    http.next_nonce = AsyncMock(return_value=next_nonce)
    http.send_tx = AsyncMock(side_effect=[create_resp, cancel_resp])
    http.account_active_orders = AsyncMock(return_value={"orders": []})

    signer = MagicMock()
    signer.auth_token.return_value = "token"
    signer.sign_create_order.return_value = SimpleNamespace(
        tx_type=14,
        tx_info="{}",
        tx_hash=create_resp["tx_hash"],
    )
    signer.sign_cancel_order.return_value = SimpleNamespace(
        tx_type=15,
        tx_info="{}",
        tx_hash=cancel_resp["tx_hash"],
    )

    config = LighterExecClientConfig(
        account_index=1,
        api_key_private_key="deadbeef",
        testnet=False,
    )
    client = LighterExecutionClient(
        loop=asyncio.get_event_loop(),
        http_client=http,
        ws_client=MagicMock(),
        signer=signer,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=provider,
        config=config,
        name="TEST",
    )

    order = TestExecStubs.limit_order(
        instrument=btc_instrument,
        price=Price.from_str("82957.5"),
        quantity=Quantity.from_str("0.0010"),
    )
    submit = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=UUID4(),
        ts_init=clock.timestamp_ns(),
        client_id=None,
    )

    await client._submit_order(submit)

    cancel = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        command_id=UUID4(),
        ts_init=clock.timestamp_ns(),
    )

    await client._cancel_order(cancel)

    assert http.next_nonce.call_count == 2
    assert http.send_tx.call_count == 2
    signer.sign_create_order.assert_called()
    signer.sign_cancel_order.assert_called()
