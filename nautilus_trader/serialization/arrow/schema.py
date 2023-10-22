# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec
import pyarrow as pa

from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.core.nautilus_pyo3 import Bar as RustBar
from nautilus_trader.core.nautilus_pyo3 import OrderBookDelta as RustOrderBookDelta
from nautilus_trader.core.nautilus_pyo3 import QuoteTick as RustQuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick as RustTradeTick
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import Ticker
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import VenueStatus
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingUpdate
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdated


NAUTILUS_ARROW_SCHEMA = {
    OrderBookDelta: pa.schema(
        [
            pa.field(k, pa.type_for_alias(v), False)
            for k, v in RustOrderBookDelta.get_fields().items()
        ],
    ),
    QuoteTick: pa.schema(
        [pa.field(k, pa.type_for_alias(v), False) for k, v in RustQuoteTick.get_fields().items()],
    ),
    TradeTick: pa.schema(
        [pa.field(k, pa.type_for_alias(v), False) for k, v in RustTradeTick.get_fields().items()],
    ),
    Bar: pa.schema(
        [pa.field(k, pa.type_for_alias(v), False) for k, v in RustBar.get_fields().items()],
    ),
    Ticker: pa.schema(
        [
            pa.field("instrument_id", pa.dictionary(pa.int16(), pa.string()), False),
            pa.field("ts_event", pa.uint64(), False),
            pa.field("ts_init", pa.uint64(), False),
        ],
    ),
    VenueStatus: pa.schema(
        {
            "venue": pa.dictionary(pa.int16(), pa.string()),
            "status": pa.dictionary(pa.int8(), pa.string()),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "InstrumentStatus"},
    ),
    InstrumentClose: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "close_type": pa.dictionary(pa.int8(), pa.string()),
            "close_price": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "InstrumentClose"},
    ),
    InstrumentStatus: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "status": pa.dictionary(pa.int8(), pa.string()),
            "trading_session": pa.string(),
            "halt_reason": pa.dictionary(pa.int8(), pa.string()),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "InstrumentStatus"},
    ),
    ComponentStateChanged: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "component_id": pa.dictionary(pa.int16(), pa.string()),
            "component_type": pa.dictionary(pa.int8(), pa.string()),
            "state": pa.string(),
            "config": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "ComponentStateChanged"},
    ),
    TradingStateChanged: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "state": pa.string(),
            "config": pa.binary(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "TradingStateChanged"},
    ),
    OrderInitialized: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "order_side": pa.dictionary(pa.int8(), pa.string()),
            "order_type": pa.dictionary(pa.int8(), pa.string()),
            "quantity": pa.string(),
            "time_in_force": pa.dictionary(pa.int8(), pa.string()),
            "post_only": pa.bool_(),
            "reduce_only": pa.bool_(),
            # -- Options fields -- #
            "price": pa.string(),
            "trigger_price": pa.string(),
            "trigger_type": pa.dictionary(pa.int8(), pa.string()),
            "limit_offset": pa.string(),
            "trailing_offset": pa.string(),
            "trailing_offset_type": pa.dictionary(pa.int8(), pa.string()),
            "expire_time_ns": pa.uint64(),
            "display_qty": pa.string(),
            "quote_quantity": pa.bool_(),
            "options": pa.string(),
            # --------------------- #
            "emulation_trigger": pa.string(),
            "trigger_instrument_id": pa.string(),
            "contingency_type": pa.string(),
            "order_list_id": pa.string(),
            "linked_order_ids": pa.string(),
            "parent_order_id": pa.string(),
            "exec_algorithm_id": pa.string(),
            "exec_algorithm_params": pa.binary(),
            "exec_spawn_id": pa.binary(),
            "tags": pa.string(),
            "event_id": pa.string(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
        metadata={
            "options_fields": msgspec.json.encode(
                [
                    "price",
                    "trigger_price",
                    "trigger_type",
                    "limit_offset",
                    "trailing_offset",
                    "trailing_offset_type",
                    "display_qty",
                    "expire_time_ns",
                ],
            ),
        },
    ),
    OrderDenied: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "reason": pa.dictionary(pa.int16(), pa.string()),
            "event_id": pa.string(),
            "ts_init": pa.uint64(),
        },
    ),
    OrderSubmitted: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    OrderAccepted: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderRejected: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "reason": pa.dictionary(pa.int16(), pa.string()),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderPendingCancel: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderCanceled: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderCancelRejected: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "reason": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderExpired: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderTriggered: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderPendingUpdate: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderModifyRejected: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "reason": pa.dictionary(pa.int16(), pa.string()),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderUpdated: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "price": pa.string(),
            "quantity": pa.string(),
            "trigger_price": pa.float64(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "reconciliation": pa.bool_(),
        },
    ),
    OrderFilled: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "trade_id": pa.string(),
            "position_id": pa.string(),
            "order_side": pa.dictionary(pa.int8(), pa.string()),
            "order_type": pa.dictionary(pa.int8(), pa.string()),
            "last_qty": pa.string(),
            "last_px": pa.string(),
            "currency": pa.string(),
            "commission": pa.string(),
            "liquidity_side": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "info": pa.binary(),
            "reconciliation": pa.bool_(),
        },
    ),
    BinanceBar: pa.schema(
        {
            "bar_type": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "open": pa.string(),
            "high": pa.string(),
            "low": pa.string(),
            "close": pa.string(),
            "volume": pa.string(),
            "quote_volume": pa.string(),
            "count": pa.uint64(),
            "taker_buy_base_volume": pa.string(),
            "taker_buy_quote_volume": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
}
