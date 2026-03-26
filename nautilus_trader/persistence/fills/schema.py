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

EXECUTION_FILL_COLUMN_NAMES = (
    "trader_id",
    "event_id",
    "strategy_id",
    "account_id",
    "instrument_id",
    "trade_id",
    "client_order_id",
    "venue_order_id",
    "position_id",
    "order_side",
    "order_type",
    "last_qty",
    "last_px",
    "currency",
    "commission",
    "liquidity_side",
    "ts_event",
    "ts_init",
    "reconciliation",
    "info_json",
    "run_id",
    "quote_cycle_id",
    "reason_code",
    "level_index",
    "target_px",
    "cancel_px",
    "match_tol",
    "ts_market_data_event_ns",
    "ts_market_data_recv_ns",
    "ts_decision_ns",
    "ts_submit_local_ns",
    "ts_command_init_ns",
    "ts_risk_recv_ns",
    "ts_risk_forward_ns",
    "ts_exec_recv_ns",
    "ts_exec_forward_ns",
    "ts_client_submit_ns",
    "ts_adapter_submit_start_ns",
    "ts_ingest_ns",
    "ts_submit_gateway_send_ns",
    "ts_cancel_gateway_send_ns",
    "ts_open_order_recv_ns",
    "ts_order_status_recv_ns",
    "ts_exec_details_recv_ns",
    "last_qty_base",
    "last_qty_venue",
    "qty_conversion_status",
    "qty_conversion_source",
    "created_at",
)


EXECUTION_FILL_TABLE_SQL = """\
CREATE TABLE IF NOT EXISTS execution_fill (
  trader_id TEXT NOT NULL,
  event_id TEXT NOT NULL,

  strategy_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  instrument_id TEXT NOT NULL,
  trade_id TEXT NOT NULL,
  client_order_id TEXT NOT NULL,
  venue_order_id TEXT NOT NULL,
  position_id TEXT,
  order_side TEXT NOT NULL,
  order_type TEXT NOT NULL,
  last_qty TEXT NOT NULL,
  last_px TEXT NOT NULL,
  currency TEXT NOT NULL,
  commission TEXT NOT NULL,
  liquidity_side TEXT NOT NULL,
  ts_event INTEGER NOT NULL,
  ts_init INTEGER NOT NULL,
  reconciliation INTEGER NOT NULL DEFAULT 0,
  info_json TEXT NOT NULL DEFAULT '{}',
  run_id TEXT,
  quote_cycle_id TEXT,
  reason_code TEXT,
  level_index INTEGER,
  target_px TEXT,
  cancel_px TEXT,
  match_tol TEXT,
  ts_market_data_event_ns INTEGER,
  ts_market_data_recv_ns INTEGER,
  ts_decision_ns INTEGER,
  ts_submit_local_ns INTEGER,
  ts_command_init_ns INTEGER,
  ts_risk_recv_ns INTEGER,
  ts_risk_forward_ns INTEGER,
  ts_exec_recv_ns INTEGER,
  ts_exec_forward_ns INTEGER,
  ts_client_submit_ns INTEGER,
  ts_adapter_submit_start_ns INTEGER,
  ts_ingest_ns INTEGER NOT NULL DEFAULT 0,
  ts_submit_gateway_send_ns INTEGER,
  ts_cancel_gateway_send_ns INTEGER,
  ts_open_order_recv_ns INTEGER,
  ts_order_status_recv_ns INTEGER,
  ts_exec_details_recv_ns INTEGER,
  last_qty_base TEXT,
  last_qty_venue TEXT,
  qty_conversion_status TEXT,
  qty_conversion_source TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id)
);
"""

EXECUTION_FILL_INDEXES_SQL = """\
CREATE INDEX IF NOT EXISTS execution_fill_ts_event_idx
  ON execution_fill (ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_strategy_ts_event_idx
  ON execution_fill (strategy_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_instrument_ts_event_idx
  ON execution_fill (instrument_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_account_ts_event_idx
  ON execution_fill (account_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_trade_id_idx
  ON execution_fill (trade_id);

CREATE INDEX IF NOT EXISTS execution_fill_client_order_id_idx
  ON execution_fill (client_order_id);

CREATE INDEX IF NOT EXISTS execution_fill_venue_order_id_idx
  ON execution_fill (venue_order_id);

CREATE INDEX IF NOT EXISTS execution_fill_quote_cycle_id_idx
  ON execution_fill (quote_cycle_id);
"""


EXECUTION_FILL_SCHEMA_SQL = EXECUTION_FILL_TABLE_SQL + "\n" + EXECUTION_FILL_INDEXES_SQL


INSERT_EXECUTION_FILL_SQL = """\
INSERT INTO execution_fill (
  trader_id,
  event_id,
  strategy_id,
  account_id,
  instrument_id,
  trade_id,
  client_order_id,
  venue_order_id,
  position_id,
  order_side,
  order_type,
  last_qty,
  last_px,
  currency,
  commission,
  liquidity_side,
  ts_event,
  ts_init,
  reconciliation,
  info_json,
  run_id,
  quote_cycle_id,
  reason_code,
  level_index,
  target_px,
  cancel_px,
  match_tol,
  ts_market_data_event_ns,
  ts_market_data_recv_ns,
  ts_decision_ns,
  ts_submit_local_ns,
  ts_command_init_ns,
  ts_risk_recv_ns,
  ts_risk_forward_ns,
  ts_exec_recv_ns,
  ts_exec_forward_ns,
  ts_client_submit_ns,
  ts_adapter_submit_start_ns,
  ts_ingest_ns,
  ts_submit_gateway_send_ns,
  ts_cancel_gateway_send_ns,
  ts_open_order_recv_ns,
  ts_order_status_recv_ns,
  ts_exec_details_recv_ns,
  last_qty_base,
  last_qty_venue,
  qty_conversion_status,
  qty_conversion_source
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, event_id) DO NOTHING
"""
