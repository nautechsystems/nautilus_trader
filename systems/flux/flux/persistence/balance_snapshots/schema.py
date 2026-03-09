from __future__ import annotations


FLUX_BALANCE_SNAPSHOT_COLUMN_NAMES = (
    "trader_id",
    "strategy_id",
    "snapshot_id",
    "topic",
    "snapshot_hash",
    "ts_event_ns",
    "ts_ms",
    "ts_ingest_ns",
    "account_count",
    "position_count",
    "payload_json",
    "created_at",
)

FLUX_BALANCE_SNAPSHOT_ROW_COLUMN_NAMES = (
    "trader_id",
    "strategy_id",
    "snapshot_id",
    "row_key",
    "kind",
    "exchange",
    "account_id",
    "account",
    "asset",
    "instrument_id",
    "side",
    "signed_qty",
    "quantity",
    "free",
    "locked",
    "total",
    "avg_px_open",
    "avg_px_close",
    "realized_pnl",
    "ts_ms",
    "row_json",
    "created_at",
)

FLUX_BALANCE_SNAPSHOT_SCHEMA_SQL = """\
CREATE TABLE IF NOT EXISTS flux_balance_snapshot (
  trader_id TEXT NOT NULL,
  strategy_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  topic TEXT NOT NULL,
  snapshot_hash TEXT NOT NULL,
  ts_event_ns INTEGER,
  ts_ms INTEGER NOT NULL,
  ts_ingest_ns INTEGER NOT NULL,
  account_count INTEGER NOT NULL,
  position_count INTEGER NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, snapshot_id)
);

CREATE INDEX IF NOT EXISTS flux_balance_snapshot_strategy_ts_ms_idx
  ON flux_balance_snapshot (strategy_id, ts_ms);

CREATE TABLE IF NOT EXISTS flux_balance_snapshot_row (
  trader_id TEXT NOT NULL,
  strategy_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  row_key TEXT NOT NULL,
  kind TEXT NOT NULL,
  exchange TEXT,
  account_id TEXT,
  account TEXT,
  asset TEXT,
  instrument_id TEXT,
  side TEXT,
  signed_qty TEXT,
  quantity TEXT,
  free TEXT,
  locked TEXT,
  total TEXT,
  avg_px_open TEXT,
  avg_px_close TEXT,
  realized_pnl TEXT,
  ts_ms INTEGER NOT NULL,
  row_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, snapshot_id, row_key)
);

CREATE INDEX IF NOT EXISTS flux_balance_snapshot_row_strategy_ts_ms_idx
  ON flux_balance_snapshot_row (strategy_id, ts_ms);

CREATE INDEX IF NOT EXISTS flux_balance_snapshot_row_strategy_instrument_ts_ms_idx
  ON flux_balance_snapshot_row (strategy_id, instrument_id, ts_ms);

CREATE INDEX IF NOT EXISTS flux_balance_snapshot_row_strategy_account_asset_ts_ms_idx
  ON flux_balance_snapshot_row (strategy_id, account_id, asset, ts_ms);
"""

INSERT_FLUX_BALANCE_SNAPSHOT_SQL = """\
INSERT INTO flux_balance_snapshot (
  trader_id,
  strategy_id,
  snapshot_id,
  topic,
  snapshot_hash,
  ts_event_ns,
  ts_ms,
  ts_ingest_ns,
  account_count,
  position_count,
  payload_json,
  created_at
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, snapshot_id) DO NOTHING
"""

INSERT_FLUX_BALANCE_SNAPSHOT_ROW_SQL = """\
INSERT INTO flux_balance_snapshot_row (
  trader_id,
  strategy_id,
  snapshot_id,
  row_key,
  kind,
  exchange,
  account_id,
  account,
  asset,
  instrument_id,
  side,
  signed_qty,
  quantity,
  free,
  locked,
  total,
  avg_px_open,
  avg_px_close,
  realized_pnl,
  ts_ms,
  row_json,
  created_at
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, snapshot_id, row_key) DO NOTHING
"""
