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

ORDER_ACTION_SCHEMA_SQL = """\
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
  ts_decision_ns INTEGER,
  signal_snapshot_json TEXT NOT NULL DEFAULT 'null',

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
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id)
);

CREATE INDEX IF NOT EXISTS order_action_strategy_ts_event_idx
  ON order_action (strategy_id, ts_event);

CREATE INDEX IF NOT EXISTS order_action_client_order_ts_event_idx
  ON order_action (client_order_id, ts_event);
"""


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
  ts_decision_ns,
  signal_snapshot_json,
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
  payload_json
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, event_id) DO NOTHING
"""
