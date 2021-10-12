# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pyarrow as pa

from nautilus_trader.common.events.risk import TradingStateChanged
from nautilus_trader.common.events.system import ComponentStateChanged
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderCancelRejected
from nautilus_trader.model.events.order import OrderDenied
from nautilus_trader.model.events.order import OrderExpired
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.events.order import OrderModifyRejected
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderTriggered
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.events.position import PositionChanged
from nautilus_trader.model.events.position import PositionClosed
from nautilus_trader.model.events.position import PositionOpened
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.future import Future
from nautilus_trader.model.instruments.option import Option
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.serialization.arrow.serializer import register_parquet


NAUTILUS_PARQUET_SCHEMA = {
    OrderBookData: pa.schema(
        {
            "instrument_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
            "action": pa.string(),
            "order_side": pa.string(),
            "order_price": pa.float64(),
            "order_size": pa.float64(),
            "order_id": pa.string(),
            "book_type": pa.string(),
            # Track grouped OrderBookDeltas
            "_type": pa.dictionary(pa.int8(), pa.string()),
            "_last": pa.bool_(),
        },
        metadata={"type": "OrderBookDelta"},
    ),
    Ticker: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "info": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
        metadata={"type": "Ticker"},
    ),
    QuoteTick: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "bid": pa.string(),
            "bid_size": pa.string(),
            "ask": pa.string(),
            "ask_size": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
        metadata={"type": "QuoteTick"},
    ),
    TradeTick: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "price": pa.string(),
            "size": pa.string(),
            "aggressor_side": pa.dictionary(pa.int8(), pa.string()),
            "match_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
        metadata={"type": "TradeTick"},
    ),
    InstrumentClosePrice: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "close_type": pa.dictionary(pa.int8(), pa.string()),
            "close_price": pa.float64(),
        }
    ),
    InstrumentStatusUpdate: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "status": pa.dictionary(pa.int8(), pa.string()),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
        metadata={"type": "InstrumentStatusUpdate"},
    ),
    ComponentStateChanged: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "component_id": pa.dictionary(pa.int8(), pa.string()),
            "component_type": pa.dictionary(pa.int8(), pa.string()),
            "state": pa.string(),
            "config": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
        metadata={"type": "ComponentStateChanged"},
    ),
    TradingStateChanged: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "state": pa.string(),
            "config": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
        metadata={"type": "TradingStateChanged"},
    ),
    AccountState: pa.schema(
        {
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "account_type": pa.dictionary(pa.int8(), pa.string()),
            "base_currency": pa.dictionary(pa.int8(), pa.string()),
            "balance_currency": pa.dictionary(pa.int8(), pa.string()),
            "balance_total": pa.float64(),
            "balance_locked": pa.float64(),
            "balance_free": pa.float64(),
            "reported": pa.bool_(),
            "info": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
        metadata={"type": "AccountState"},
    ),
    OrderInitialized: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "order_side": pa.dictionary(pa.int8(), pa.string()),
            "order_type": pa.dictionary(pa.int8(), pa.string()),
            "quantity": pa.float64(),
            "time_in_force": pa.dictionary(pa.int8(), pa.string()),
            "reduce_only": pa.bool_(),
            # -- Options fields -- #
            "post_only": pa.bool_(),
            "hidden": pa.bool_(),
            "price": pa.float64(),
            "trigger": pa.bool_(),
            # --------------------- #
            "order_list_id": pa.string(),
            "parent_order_id": pa.string(),
            "child_order_ids": pa.string(),
            "contingency": pa.string(),
            "contingency_ids": pa.string(),
            "tags": pa.string(),
            "event_id": pa.string(),
            "ts_init": pa.int64(),
        },
    ),
    OrderDenied: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "reason": pa.dictionary(pa.int8(), pa.string()),
            "event_id": pa.string(),
            "ts_init": pa.int64(),
        }
    ),
    OrderSubmitted: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderAccepted: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderRejected: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "reason": pa.dictionary(pa.int8(), pa.string()),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderPendingCancel: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderCanceled: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderCancelRejected: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "reason": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderExpired: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderTriggered: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderPendingUpdate: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderModifyRejected: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "reason": pa.dictionary(pa.int8(), pa.string()),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderUpdated: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "price": pa.float64(),
            "trigger": pa.float64(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    OrderFilled: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "client_order_id": pa.string(),
            "venue_order_id": pa.string(),
            "execution_id": pa.string(),
            "position_id": pa.string(),
            "order_side": pa.dictionary(pa.int8(), pa.string()),
            "order_type": pa.dictionary(pa.int8(), pa.string()),
            "last_qty": pa.float64(),
            "last_px": pa.float64(),
            "currency": pa.string(),
            "commission": pa.string(),
            "liquidity_side": pa.string(),
            "event_id": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
            "info": pa.string(),
        }
    ),
    PositionOpened: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int8(), pa.string()),
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "position_id": pa.string(),
            "account_id": pa.dictionary(pa.int8(), pa.string()),
            "from_order": pa.string(),
            "entry": pa.string(),
            "side": pa.string(),
            "net_qty": pa.float64(),
            "quantity": pa.float64(),
            "peak_qty": pa.float64(),
            "last_qty": pa.float64(),
            "last_px": pa.float64(),
            "currency": pa.string(),
            "avg_px_open": pa.float64(),
            "realized_pnl": pa.float64(),
            "event_id": pa.string(),
            "duration_ns": pa.int64(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    PositionChanged: pa.schema(
        {
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "position_id": pa.string(),
            "entry": pa.string(),
            "side": pa.string(),
            "net_qty": pa.float64(),
            "quantity": pa.float64(),
            "peak_qty": pa.float64(),
            "avg_px_open": pa.float64(),
            "event_id": pa.string(),
            "ts_opened": pa.int64(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    PositionClosed: pa.schema(
        {
            "strategy_id": pa.dictionary(pa.int8(), pa.string()),
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "position_id": pa.string(),
            "entry": pa.string(),
            "side": pa.string(),
            "net_qty": pa.float64(),
            "quantity": pa.float64(),
            "peak_qty": pa.float64(),
            "avg_px_open": pa.float64(),
            "avg_px_close": pa.float64(),
            "realized_pnl": pa.float64(),
            "event_id": pa.string(),
            "ts_opened": pa.int64(),
            "ts_closed": pa.int64(),
            "ts_init": pa.int64(),
        }
    ),
    BettingInstrument: pa.schema(
        {
            "venue_name": pa.string(),
            "currency": pa.string(),
            "id": pa.string(),
            "event_type_id": pa.string(),
            "event_type_name": pa.string(),
            "competition_id": pa.string(),
            "competition_name": pa.string(),
            "event_id": pa.string(),
            "event_name": pa.string(),
            "event_country_code": pa.string(),
            "event_open_date": pa.string(),
            "betting_type": pa.string(),
            "market_id": pa.string(),
            "market_name": pa.string(),
            "market_start_time": pa.string(),
            "market_type": pa.string(),
            "selection_id": pa.string(),
            "selection_name": pa.string(),
            "selection_handicap": pa.string(),
            "ts_init": pa.int64(),
            "ts_event": pa.int64(),
        },
        metadata={"type": "BettingInstrument"},
    ),
    CurrencySpot: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "base_currency": pa.dictionary(pa.int8(), pa.string()),
            "quote_currency": pa.dictionary(pa.int8(), pa.string()),
            "price_precision": pa.int64(),
            "size_precision": pa.int64(),
            "price_increment": pa.dictionary(pa.int8(), pa.string()),
            "size_increment": pa.dictionary(pa.int8(), pa.string()),
            "lot_size": pa.dictionary(pa.int8(), pa.string()),
            "max_quantity": pa.dictionary(pa.int8(), pa.string()),
            "min_quantity": pa.dictionary(pa.int8(), pa.string()),
            "max_notional": pa.dictionary(pa.int8(), pa.string()),
            "min_notional": pa.dictionary(pa.int8(), pa.string()),
            "max_price": pa.dictionary(pa.int8(), pa.string()),
            "min_price": pa.dictionary(pa.int8(), pa.string()),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.string(),
            "ts_init": pa.int64(),
            "ts_event": pa.int64(),
        }
    ),
    Equity: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "currency": pa.dictionary(pa.int8(), pa.string()),
            "price_precision": pa.int64(),
            "size_precision": pa.int64(),
            "price_increment": pa.dictionary(pa.int8(), pa.string()),
            "size_increment": pa.dictionary(pa.int8(), pa.string()),
            "multiplier": pa.dictionary(pa.int8(), pa.string()),
            "lot_size": pa.dictionary(pa.int8(), pa.string()),
            "isin": pa.string(),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "ts_init": pa.int64(),
            "ts_event": pa.int64(),
        }
    ),
    Future: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "underlying": pa.dictionary(pa.int8(), pa.string()),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "currency": pa.dictionary(pa.int8(), pa.string()),
            "price_precision": pa.int64(),
            "size_precision": pa.int64(),
            "price_increment": pa.dictionary(pa.int8(), pa.string()),
            "size_increment": pa.dictionary(pa.int8(), pa.string()),
            "multiplier": pa.dictionary(pa.int8(), pa.string()),
            "lot_size": pa.dictionary(pa.int8(), pa.string()),
            "expiry_date": pa.dictionary(pa.int8(), pa.string()),
            "ts_init": pa.int64(),
            "ts_event": pa.int64(),
        }
    ),
    Option: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "underlying": pa.dictionary(pa.int8(), pa.string()),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "currency": pa.dictionary(pa.int8(), pa.string()),
            "price_precision": pa.int64(),
            "size_precision": pa.int64(),
            "price_increment": pa.dictionary(pa.int8(), pa.string()),
            "size_increment": pa.dictionary(pa.int8(), pa.string()),
            "multiplier": pa.dictionary(pa.int8(), pa.string()),
            "lot_size": pa.dictionary(pa.int8(), pa.string()),
            "expiry_date": pa.dictionary(pa.int8(), pa.string()),
            "strike_price": pa.dictionary(pa.int16(), pa.string()),
            "ts_init": pa.int64(),
            "ts_event": pa.int64(),
        }
    ),
}


# default schemas
for cls, schema in NAUTILUS_PARQUET_SCHEMA.items():
    register_parquet(cls, schema=schema)
