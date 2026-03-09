from nautilus_trader.persistence._action_intent import DECISION_CONTEXT_JSON_DEFAULT_LITERAL


DECISION_CONTEXT_JSON_DEFAULT_SQL = f"'{DECISION_CONTEXT_JSON_DEFAULT_LITERAL}'"

QUOTE_CYCLE_COLUMN_NAMES = (
    "trader_id",
    "strategy_id",
    "instrument_id",
    "run_id",
    "quote_cycle_id",
    "quote_cycle_seq",
    "quote_cycle_event",
    "reason_code",
    "trigger_source",
    "trigger_instrument_id",
    "trigger_md_ts_event_ns",
    "trigger_md_ts_init_ns",
    "ts_cycle_start_ns",
    "ts_cycle_end_ns",
    "state_from",
    "state_to",
    "cancel_count",
    "place_count",
    "bid_levels",
    "ask_levels",
    "decision_context_json",
    "created_at",
)

QUOTE_CYCLE_SCHEMA_SQL = f"""\
CREATE TABLE IF NOT EXISTS quote_cycle (
  trader_id TEXT NOT NULL,
  strategy_id TEXT NOT NULL,
  instrument_id TEXT NOT NULL,
  run_id TEXT NOT NULL,
  quote_cycle_id TEXT NOT NULL,
  quote_cycle_seq INTEGER NOT NULL,
  quote_cycle_event TEXT NOT NULL,
  reason_code TEXT NOT NULL,
  trigger_source TEXT,
  trigger_instrument_id TEXT,
  trigger_md_ts_event_ns INTEGER,
  trigger_md_ts_init_ns INTEGER,
  ts_cycle_start_ns INTEGER,
  ts_cycle_end_ns INTEGER,
  state_from TEXT,
  state_to TEXT,
  cancel_count INTEGER,
  place_count INTEGER,
  bid_levels INTEGER,
  ask_levels INTEGER,
  decision_context_json TEXT NOT NULL DEFAULT {DECISION_CONTEXT_JSON_DEFAULT_SQL},
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, quote_cycle_id)
);

CREATE INDEX IF NOT EXISTS quote_cycle_strategy_ts_cycle_start_ns_idx
  ON quote_cycle (strategy_id, ts_cycle_start_ns);

CREATE INDEX IF NOT EXISTS quote_cycle_run_seq_idx
  ON quote_cycle (run_id, quote_cycle_seq);

CREATE INDEX IF NOT EXISTS quote_cycle_reason_ts_cycle_start_ns_idx
  ON quote_cycle (reason_code, ts_cycle_start_ns);
"""


INSERT_QUOTE_CYCLE_SQL = """\
INSERT INTO quote_cycle (
  trader_id,
  strategy_id,
  instrument_id,
  run_id,
  quote_cycle_id,
  quote_cycle_seq,
  quote_cycle_event,
  reason_code,
  trigger_source,
  trigger_instrument_id,
  trigger_md_ts_event_ns,
  trigger_md_ts_init_ns,
  ts_cycle_start_ns,
  ts_cycle_end_ns,
  state_from,
  state_to,
  cancel_count,
  place_count,
  bid_levels,
  ask_levels,
  decision_context_json
) VALUES (
  ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
)
ON CONFLICT(trader_id, quote_cycle_id) DO NOTHING
"""
