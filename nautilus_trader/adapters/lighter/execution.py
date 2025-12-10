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
import hashlib
from collections import defaultdict
from typing import TYPE_CHECKING, Any, Optional

from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter.signer import LighterSigner, SignerError
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.execution.messages import (
    CancelOrder,
    CancelAllOrders,
    GenerateOrderStatusReport,
    GenerateOrderStatusReports,
    SubmitOrder,
)
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import OmsType, OrderSide, OrderStatus, OrderType, TimeInForce
from nautilus_trader.model.identifiers import ClientId, Venue
from nautilus_trader.model.orders import Order


if TYPE_CHECKING:
    from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider


class LighterExecutionClient(LiveExecutionClient):
    """
    Minimal execution client for Lighter (submit + cancel only).

    Uses the native signer to produce tx_info payloads and the Rust HTTP client
    (via PyO3) to post `sendTx`. Private WS order streams are not yet wired.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: Any,
        ws_client: Any,
        signer: LighterSigner,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: LighterInstrumentProvider,
        config: LighterExecClientConfig,
        name: str,
    ) -> None:
        self._http_client = http_client
        self._ws_client = ws_client
        self._signer = signer
        self._config = config
        self._instrument_provider = instrument_provider
        self._client_order_indices: dict[str, int] = {}
        self._strategy_order_ids: dict[str, set[str]] = defaultdict(set)

        super().__init__(
            loop=loop,
            client_id=ClientId(name),
            venue=LIGHTER_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=config.account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

    # ---------------------------------------------------------------------------------------------
    # Connection lifecycle
    # ---------------------------------------------------------------------------------------------

    async def _connect(self) -> None:
        # No persistent connection required for HTTP-based submit; WS private stream pending.
        return

    async def _disconnect(self) -> None:
        return

    # ---------------------------------------------------------------------------------------------
    # Command handlers
    # ---------------------------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order
        instrument = self._require_instrument(order.instrument_id)

        market_index = self._instrument_provider.market_index_for(order.instrument_id)
        if market_index is None:
            raise ValueError(f"Missing market index for {order.instrument_id}")

        price_int = instrument.price_to_int(order.price)
        size_int = instrument.size_to_int(order.quantity)
        coi = self._client_order_index(order.client_order_id.value)

        nonce = await self._fetch_nonce()
        signed = self._signer.sign_create_order(
            market_index=market_index,
            client_order_index=coi,
            base_amount_int=size_int,
            price_int=price_int,
            is_ask=order.side.is_sell,
            order_type=0,
            time_in_force=1,
            nonce=nonce,
        )

        self._strategy_order_ids[order.strategy_id.value].add(order.client_order_id.value)
        await self._post_send_tx(signed.tx_type, signed.tx_info)
        self._log.info(f"Submitted order {order.client_order_id} tx={signed.tx_hash}")

    async def _cancel_order(self, command: CancelOrder) -> None:
        order_id = command.order_id
        instrument = self._require_instrument(order_id.instrument_id)
        market_index = self._instrument_provider.market_index_for(instrument.id)
        if market_index is None:
            raise ValueError(f"Missing market index for {instrument.id}")

        coi = self._client_order_index(order_id.client_order_id.value)
        nonce = await self._fetch_nonce()
        signed = self._signer.sign_cancel_order(
            market_index=market_index,
            order_index=coi,
            nonce=nonce,
        )

        await self._post_send_tx(signed.tx_type, signed.tx_info)
        self._strategy_order_ids[order_id.strategy_id.value].discard(order_id.client_order_id.value)
        self._log.info(f"Canceled order {order_id.client_order_id} tx={signed.tx_hash}")

    # ---------------------------------------------------------------------------------------------
    # Reports (stubbed for now)
    # ---------------------------------------------------------------------------------------------

    async def generate_order_status_report(self, command):  # pragma: no cover - stub
        return None

    async def generate_order_status_reports(self, command):  # pragma: no cover - stub
        return []

    async def generate_fill_reports(self, command):  # pragma: no cover - stub
        return []

    async def generate_position_status_reports(self, command):  # pragma: no cover - stub
        return []

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        # Placeholder: loop through active orders via REST until WS schema is known.
        token = self._maybe_auth_token()
        if not token:
            self._log.warning("Cannot cancel all orders without auth token")
            return

        strategy_id = command.strategy_id.value if command.strategy_id else None
        strategy_orders = self._strategy_order_ids.get(strategy_id, set()) if strategy_id else None
        if strategy_orders is not None and not strategy_orders:
            # After restart we may not have local state; fall back to canceling all for the instrument.
            self._log.warning("No tracked orders for strategy; cancel-all will not filter by strategy")
            strategy_orders = None
        instrument_ids = [command.instrument_id] if command.instrument_id else []
        if not instrument_ids:
            # Fallback: cancel per loaded instrument.
            instrument_ids = list(self._instrument_provider._market_index_by_instrument.keys())  # type: ignore[attr-defined]

        for instrument_id in instrument_ids:
            market_index = self._instrument_provider.market_index_for(instrument_id)  # type: ignore[arg-type]
            if market_index is None:
                continue

            resp = await self._http_client.account_active_orders(  # type: ignore[attr-defined]
                account_index=self._config.resolved_account_index or 0,
                market_id=market_index,
                auth_token=token,
            )
            orders = resp["orders"] if isinstance(resp, dict) else resp.orders
            for order in orders or []:
                client_order_id = None
                try:
                    is_ask = order.get("is_ask") if isinstance(order, dict) else getattr(order, "is_ask", None)
                    if command.order_side and is_ask is not None:
                        if command.order_side == OrderSide.BUY and is_ask is True:
                            continue
                        if command.order_side == OrderSide.SELL and is_ask is False:
                            continue

                    client_order_id = str(
                        order.get("client_order_id")
                        if isinstance(order, dict)
                        else getattr(order, "client_order_id", "")
                    )
                    if not client_order_id:
                        continue
                    if strategy_orders is not None and client_order_id not in strategy_orders:
                        # Respect per-strategy scope; skip untracked orders.
                        continue
                    coi = self._client_order_index(client_order_id)
                    nonce = await self._fetch_nonce()
                    signed = self._signer.sign_cancel_order(
                        market_index=market_index,
                        order_index=coi,
                        nonce=nonce,
                    )
                    await self._post_send_tx(signed.tx_type, signed.tx_info)
                except Exception as exc:  # pragma: no cover - best-effort
                    cid = client_order_id or "<unknown>"
                    self._log.warning(f"cancel_all_orders failed for {cid}: {exc}")

    # ---------------------------------------------------------------------------------------------
    # Helpers
    # ---------------------------------------------------------------------------------------------

    async def _fetch_nonce(self) -> int:
        # Token is optional for nextNonce but provided if available
        token = self._maybe_auth_token()
        resp = await self._http_client.next_nonce(  # type: ignore[attr-defined]
            account_index=self._config.resolved_account_index or 0,
            api_key_index=self._config.api_key_index,
            auth_token=token,
        )
        return resp["nonce"] if isinstance(resp, dict) else resp.nonce  # PyO3 returns pyobj

    async def _post_send_tx(self, tx_type: int, tx_info: str) -> None:
        resp = await self._http_client.send_tx(  # type: ignore[attr-defined]
            tx_type=tx_type,
            tx_info=tx_info,
            price_protection=True,
        )
        _ = resp  # TODO: map response to events/logging

    def _maybe_auth_token(self) -> Optional[str]:
        try:
            return self._signer.auth_token()
        except SignerError:
            return None

    def _client_order_index(self, client_order_id: str) -> int:
        """
        Convert an arbitrary client order ID into a deterministic int64 the signer accepts.

        Prefers numeric IDs; otherwise uses a stable blake2b hash (64-bit) derived from the string.
        """
        if client_order_id in self._client_order_indices:
            return self._client_order_indices[client_order_id]

        if client_order_id.isdigit():
            value = int(client_order_id)
        else:
            digest = hashlib.blake2b(client_order_id.encode("utf-8"), digest_size=8).digest()
            # Clamp to signed 63-bit range (positive) to keep signer happy.
            value = int.from_bytes(digest, "big") & ((1 << 63) - 1)
        if value == 0:
            value = 1

        self._client_order_indices[client_order_id] = value
        return value

    def _require_instrument(self, instrument_id) -> Any:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            raise ValueError(f"Instrument not loaded: {instrument_id}")
        return instrument
