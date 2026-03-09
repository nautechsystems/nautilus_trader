from __future__ import annotations


PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES = (
    "portfolio_id",
    "base_currency",
    "snapshot_id",
    "snapshot_hash",
    "global_qty_base",
    "global_qty",
    "aggregation_mode",
    "global_qty_base_complete",
    "global_qty_complete",
    "degraded",
    "missing_required_json",
    "stale_required_json",
    "null_qty_required_json",
    "components_json",
    "usable_component_count",
    "expected_component_count",
    "stale_after_ms",
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
  global_qty_base TEXT,
  global_qty TEXT,
  aggregation_mode TEXT NOT NULL DEFAULT 'strict',
  global_qty_base_complete INTEGER NOT NULL DEFAULT 0,
  global_qty_complete INTEGER NOT NULL DEFAULT 0,
  degraded INTEGER NOT NULL DEFAULT 0,
  missing_required_json TEXT NOT NULL DEFAULT '[]',
  stale_required_json TEXT NOT NULL DEFAULT '[]',
  null_qty_required_json TEXT NOT NULL DEFAULT '[]',
  components_json TEXT NOT NULL DEFAULT '[]',
  usable_component_count INTEGER NOT NULL DEFAULT 0,
  expected_component_count INTEGER NOT NULL DEFAULT 0,
  stale_after_ms INTEGER NOT NULL DEFAULT 0,
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
  global_qty_base,
  global_qty,
  aggregation_mode,
  global_qty_base_complete,
  global_qty_complete,
  degraded,
  missing_required_json,
  stale_required_json,
  null_qty_required_json,
  components_json,
  usable_component_count,
  expected_component_count,
  stale_after_ms,
  ts_ms,
  ts_ingest_ns,
  created_at
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(portfolio_id, base_currency, snapshot_id) DO NOTHING
"""
