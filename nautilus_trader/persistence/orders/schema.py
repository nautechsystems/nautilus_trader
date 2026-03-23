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

from nautilus_trader.persistence._action_intent import DECISION_CONTEXT_JSON_DEFAULT_LITERAL


DECISION_CONTEXT_JSON_DEFAULT_SQL = f"'{DECISION_CONTEXT_JSON_DEFAULT_LITERAL}'"
SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL = DECISION_CONTEXT_JSON_DEFAULT_LITERAL

ORDER_ACTION_COLUMN_NAMES = (
    "trader_id",
    "event_id",
    "strategy_id",
    "instrument_id",
    "client_order_id",
    "account_id",
    "venue_order_id",
    "position_id",
    "action_type",
    "action_state",
    "event_type",
    "action_id",
    "action_reason",
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
    "ts_cancel_request_local_ns",
    "decision_context_json",
    "order_side",
    "order_type",
    "time_in_force",
    "post_only",
    "reduce_only",
    "order_qty",
    "order_px",
    "rejection_reason",
    "ts_event",
    "ts_init",
    "ts_ingest",
    "reconciliation",
    "payload_json",
    "order_qty_base",
    "order_qty_venue",
    "qty_conversion_status",
    "qty_conversion_source",
    "created_at",
)

ORDER_ACTION_TABLE_SQL = f"""\
CREATE TABLE IF NOT EXISTS order_action (
  trader_id TEXT NOT NULL,
  event_id TEXT NOT NULL,

  strategy_id TEXT NOT NULL,
  instrument_id TEXT NOT NULL,
  client_order_id TEXT NOT NULL,
  account_id TEXT,
  venue_order_id TEXT,
  position_id TEXT,

  action_type TEXT NOT NULL,
  action_state TEXT NOT NULL,
  event_type TEXT NOT NULL,

  action_id TEXT,
  action_reason TEXT,
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
  ts_cancel_request_local_ns INTEGER,
  decision_context_json TEXT NOT NULL DEFAULT {DECISION_CONTEXT_JSON_DEFAULT_SQL},

  order_side TEXT,
  order_type TEXT,
  time_in_force TEXT,
  post_only INTEGER,
  reduce_only INTEGER,
  order_qty TEXT,
  order_px TEXT,

  rejection_reason TEXT,

  ts_event INTEGER NOT NULL,
  ts_init INTEGER NOT NULL,
  ts_ingest INTEGER NOT NULL,
  reconciliation INTEGER NOT NULL DEFAULT 0,
  payload_json TEXT NOT NULL DEFAULT '{{}}',
  order_qty_base TEXT,
  order_qty_venue TEXT,
  qty_conversion_status TEXT,
  qty_conversion_source TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id)
);
"""

ORDER_ACTION_INDEXES_SQL = """\
CREATE INDEX IF NOT EXISTS order_action_strategy_ts_event_idx
  ON order_action (strategy_id, ts_event);

CREATE INDEX IF NOT EXISTS order_action_client_order_ts_event_idx
  ON order_action (client_order_id, ts_event);

CREATE INDEX IF NOT EXISTS order_action_quote_cycle_id_idx
  ON order_action (quote_cycle_id);

CREATE INDEX IF NOT EXISTS order_action_trader_strategy_action_state_ts_event_idx
  ON order_action (trader_id, strategy_id, action_type, action_state, ts_event);
"""

ORDER_ACTION_SCHEMA_SQL = ORDER_ACTION_TABLE_SQL + "\n" + ORDER_ACTION_INDEXES_SQL

ORDER_ACTION_MIGRATION_DEFAULTS = {
    "action_id": "NULL",
    "action_reason": "NULL",
    "run_id": "NULL",
    "quote_cycle_id": "NULL",
    "reason_code": "NULL",
    "level_index": "NULL",
    "target_px": "NULL",
    "cancel_px": "NULL",
    "match_tol": "NULL",
    "ts_market_data_event_ns": "NULL",
    "ts_market_data_recv_ns": "NULL",
    "ts_decision_ns": "NULL",
    "ts_submit_local_ns": "NULL",
    "ts_command_init_ns": "NULL",
    "ts_risk_recv_ns": "NULL",
    "ts_risk_forward_ns": "NULL",
    "ts_exec_recv_ns": "NULL",
    "ts_exec_forward_ns": "NULL",
    "ts_client_submit_ns": "NULL",
    "ts_adapter_submit_start_ns": "NULL",
    "ts_cancel_request_local_ns": "NULL",
    "decision_context_json": DECISION_CONTEXT_JSON_DEFAULT_SQL,
    "order_side": "NULL",
    "order_type": "NULL",
    "time_in_force": "NULL",
    "post_only": "NULL",
    "reduce_only": "NULL",
    "order_qty": "NULL",
    "order_px": "NULL",
    "rejection_reason": "NULL",
    "ts_ingest": "0",
    "reconciliation": "0",
    "payload_json": "'{}'",
    "order_qty_base": "NULL",
    "order_qty_venue": "NULL",
    "qty_conversion_status": "NULL",
    "qty_conversion_source": "NULL",
    "created_at": "(strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
}


INSERT_ORDER_ACTION_SQL = """\
INSERT INTO order_action (
  trader_id,
  event_id,
  strategy_id,
  instrument_id,
  client_order_id,
  account_id,
  venue_order_id,
  position_id,
  action_type,
  action_state,
  event_type,
  action_id,
  action_reason,
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
  ts_cancel_request_local_ns,
  decision_context_json,
  order_side,
  order_type,
  time_in_force,
  post_only,
  reduce_only,
  order_qty,
  order_px,
  rejection_reason,
  ts_event,
  ts_init,
  ts_ingest,
  reconciliation,
  payload_json,
  order_qty_base,
  order_qty_venue,
  qty_conversion_status,
  qty_conversion_source
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, event_id) DO NOTHING
"""
