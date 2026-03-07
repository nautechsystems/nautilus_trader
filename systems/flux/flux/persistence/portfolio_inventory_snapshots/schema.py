from __future__ import annotations


PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES = (
    "portfolio_id",
    "base_currency",
    "snapshot_id",
    "snapshot_hash",
    "global_qty",
    "degraded",
    "missing_required_json",
    "components_json",
    "ts_ms",
    "ts_ingest_ns",
    "created_at",
)

PORTFOLIO_INVENTORY_SNAPSHOT_SCHEMA_SQL = """\
CREATE TABLE IF NOT EXISTS portfolio_inventory_snapshot (
  portfolio_id TEXT NOT NULL,
  base_currency TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  snapshot_hash TEXT NOT NULL,
  global_qty TEXT,
  degraded INTEGER NOT NULL DEFAULT 0,
  missing_required_json TEXT NOT NULL DEFAULT '[]',
  components_json TEXT NOT NULL DEFAULT '[]',
  ts_ms INTEGER NOT NULL,
  ts_ingest_ns INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (portfolio_id, base_currency, snapshot_id)
);

CREATE INDEX IF NOT EXISTS portfolio_inventory_snapshot_portfolio_ts_ms_idx
  ON portfolio_inventory_snapshot (portfolio_id, base_currency, ts_ms);
"""

INSERT_PORTFOLIO_INVENTORY_SNAPSHOT_SQL = """\
INSERT INTO portfolio_inventory_snapshot (
  portfolio_id,
  base_currency,
  snapshot_id,
  snapshot_hash,
  global_qty,
  degraded,
  missing_required_json,
  components_json,
  ts_ms,
  ts_ingest_ns,
  created_at
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(portfolio_id, base_currency, snapshot_id) DO NOTHING
"""
