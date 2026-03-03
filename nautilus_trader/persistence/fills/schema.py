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

EXECUTION_FILL_SCHEMA_SQL = """\
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
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id)
);

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
"""


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
  info_json
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, event_id) DO NOTHING
"""

