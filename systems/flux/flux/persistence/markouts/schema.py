EXECUTION_MARKOUT_COLUMN_NAMES = (
    "trader_id",
    "event_id",
    "trade_id",
    "strategy_id",
    "instrument_id",
    "client_order_id",
    "order_side",
    "fill_px",
    "fill_qty",
    "benchmark_name",
    "horizon_s",
    "target_ts_ms",
    "benchmark_ts_ms",
    "benchmark_px",
    "markout_abs",
    "markout_bps",
    "resolution_status",
    "run_id",
    "quote_cycle_id",
    "reason_code",
    "level_index",
    "created_at",
)

EXECUTION_MARKOUT_SCHEMA_SQL = """\
CREATE TABLE IF NOT EXISTS execution_markout (
  trader_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  trade_id TEXT NOT NULL,
  strategy_id TEXT NOT NULL,
  instrument_id TEXT NOT NULL,
  client_order_id TEXT NOT NULL,
  order_side TEXT NOT NULL,
  fill_px TEXT NOT NULL,
  fill_qty TEXT NOT NULL,
  benchmark_name TEXT NOT NULL,
  horizon_s INTEGER NOT NULL,
  target_ts_ms INTEGER NOT NULL,
  benchmark_ts_ms INTEGER,
  benchmark_px TEXT,
  markout_abs TEXT,
  markout_bps TEXT,
  resolution_status TEXT NOT NULL,
  run_id TEXT,
  quote_cycle_id TEXT,
  reason_code TEXT,
  level_index INTEGER,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id, horizon_s)
);

CREATE INDEX IF NOT EXISTS execution_markout_strategy_target_ts_ms_idx
  ON execution_markout (strategy_id, target_ts_ms);

CREATE INDEX IF NOT EXISTS execution_markout_quote_cycle_id_idx
  ON execution_markout (quote_cycle_id);

CREATE INDEX IF NOT EXISTS execution_markout_resolution_status_idx
  ON execution_markout (resolution_status);
"""

INSERT_EXECUTION_MARKOUT_SQL = """\
INSERT INTO execution_markout (
  trader_id,
  event_id,
  trade_id,
  strategy_id,
  instrument_id,
  client_order_id,
  order_side,
  fill_px,
  fill_qty,
  benchmark_name,
  horizon_s,
  target_ts_ms,
  benchmark_ts_ms,
  benchmark_px,
  markout_abs,
  markout_bps,
  resolution_status,
  run_id,
  quote_cycle_id,
  reason_code,
  level_index
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, event_id, horizon_s) DO NOTHING
"""
