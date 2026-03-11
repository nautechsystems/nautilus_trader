from __future__ import annotations

from collections.abc import Iterable
from typing import Any

from nautilus_trader.flux.persistence.balance_snapshots.schema import (
    FLUX_BALANCE_SNAPSHOT_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.balance_snapshots.schema import (
    FLUX_BALANCE_SNAPSHOT_ROW_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.portfolio_inventory_snapshots.schema import (
    PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.quote_cycles.schema import QUOTE_CYCLE_COLUMN_NAMES
from nautilus_trader.persistence.fills.schema import EXECUTION_FILL_COLUMN_NAMES
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_COLUMN_NAMES
from nautilus_trader.persistence.shipper.config import TelemetryPostgresConfig


TABLE_COLUMN_NAMES: dict[str, tuple[str, ...]] = {
    "flux_balance_snapshot": FLUX_BALANCE_SNAPSHOT_COLUMN_NAMES,
    "flux_balance_snapshot_row": FLUX_BALANCE_SNAPSHOT_ROW_COLUMN_NAMES,
    "execution_fill": EXECUTION_FILL_COLUMN_NAMES,
    "order_action": ORDER_ACTION_COLUMN_NAMES,
    "portfolio_inventory_snapshot": PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES,
    "quote_cycle": QUOTE_CYCLE_COLUMN_NAMES,
}

TABLE_PRIMARY_KEYS: dict[str, tuple[str, ...]] = {
    "flux_balance_snapshot": ("source_profile", "trader_id", "snapshot_id"),
    "flux_balance_snapshot_row": ("source_profile", "trader_id", "snapshot_id", "row_key"),
    "execution_fill": ("source_profile", "trader_id", "event_id"),
    "order_action": ("source_profile", "trader_id", "event_id"),
    "portfolio_inventory_snapshot": ("source_profile", "portfolio_id", "base_currency", "snapshot_id"),
    "quote_cycle": ("source_profile", "trader_id", "quote_cycle_id"),
}

TABLE_CREATE_SQL: dict[str, str] = {
    "flux_balance_snapshot": """\
CREATE TABLE IF NOT EXISTS {schema}.flux_balance_snapshot (
  trader_id TEXT NOT NULL,
  strategy_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  topic TEXT NOT NULL,
  snapshot_hash TEXT NOT NULL,
  ts_event_ns BIGINT,
  ts_ms BIGINT NOT NULL,
  ts_ingest_ns BIGINT NOT NULL,
  account_count INTEGER NOT NULL,
  position_count INTEGER NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  source_profile TEXT NOT NULL,
  source_host TEXT NOT NULL,
  shipped_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (source_profile, trader_id, snapshot_id)
);
CREATE INDEX IF NOT EXISTS flux_balance_snapshot_source_strategy_ts_ms_idx
  ON {schema}.flux_balance_snapshot (source_profile, strategy_id, ts_ms);
""",
    "flux_balance_snapshot_row": """\
CREATE TABLE IF NOT EXISTS {schema}.flux_balance_snapshot_row (
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
  ts_ms BIGINT NOT NULL,
  row_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  source_profile TEXT NOT NULL,
  source_host TEXT NOT NULL,
  shipped_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (source_profile, trader_id, snapshot_id, row_key)
);
CREATE INDEX IF NOT EXISTS flux_balance_snapshot_row_source_strategy_ts_ms_idx
  ON {schema}.flux_balance_snapshot_row (source_profile, strategy_id, ts_ms);
CREATE INDEX IF NOT EXISTS flux_balance_snapshot_row_source_strategy_instrument_ts_ms_idx
  ON {schema}.flux_balance_snapshot_row (source_profile, strategy_id, instrument_id, ts_ms);
CREATE INDEX IF NOT EXISTS flux_balance_snapshot_row_source_strategy_account_asset_ts_ms_idx
  ON {schema}.flux_balance_snapshot_row (source_profile, strategy_id, account_id, asset, ts_ms);
""",
    "execution_fill": """\
CREATE TABLE IF NOT EXISTS {schema}.execution_fill (
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
  ts_event BIGINT NOT NULL,
  ts_init BIGINT NOT NULL,
  reconciliation INTEGER NOT NULL DEFAULT 0,
  info_json TEXT NOT NULL DEFAULT '{{}}',
  run_id TEXT,
  quote_cycle_id TEXT,
  reason_code TEXT,
  level_index INTEGER,
  target_px TEXT,
  cancel_px TEXT,
  match_tol TEXT,
  ts_market_data_event_ns BIGINT,
  ts_market_data_recv_ns BIGINT,
  ts_decision_ns BIGINT,
  ts_submit_local_ns BIGINT,
  ts_command_init_ns BIGINT,
  ts_risk_recv_ns BIGINT,
  ts_risk_forward_ns BIGINT,
  ts_exec_recv_ns BIGINT,
  ts_exec_forward_ns BIGINT,
  ts_client_submit_ns BIGINT,
  ts_adapter_submit_start_ns BIGINT,
  ts_ingest_ns BIGINT NOT NULL DEFAULT 0,
  ts_submit_gateway_send_ns BIGINT,
  ts_cancel_gateway_send_ns BIGINT,
  ts_open_order_recv_ns BIGINT,
  ts_order_status_recv_ns BIGINT,
  ts_exec_details_recv_ns BIGINT,
  created_at TEXT NOT NULL,
  source_profile TEXT NOT NULL,
  source_host TEXT NOT NULL,
  shipped_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (source_profile, trader_id, event_id)
);
CREATE INDEX IF NOT EXISTS execution_fill_source_strategy_ts_event_idx
  ON {schema}.execution_fill (source_profile, strategy_id, ts_event);
CREATE INDEX IF NOT EXISTS execution_fill_source_instrument_ts_event_idx
  ON {schema}.execution_fill (source_profile, instrument_id, ts_event);
CREATE INDEX IF NOT EXISTS execution_fill_source_account_ts_event_idx
  ON {schema}.execution_fill (source_profile, account_id, ts_event);
CREATE INDEX IF NOT EXISTS execution_fill_source_quote_cycle_id_idx
  ON {schema}.execution_fill (source_profile, quote_cycle_id);
""",
    "order_action": """\
CREATE TABLE IF NOT EXISTS {schema}.order_action (
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
  ts_market_data_event_ns BIGINT,
  ts_market_data_recv_ns BIGINT,
  ts_decision_ns BIGINT,
  ts_submit_local_ns BIGINT,
  ts_command_init_ns BIGINT,
  ts_risk_recv_ns BIGINT,
  ts_risk_forward_ns BIGINT,
  ts_exec_recv_ns BIGINT,
  ts_exec_forward_ns BIGINT,
  ts_client_submit_ns BIGINT,
  ts_adapter_submit_start_ns BIGINT,
  ts_cancel_request_local_ns BIGINT,
  decision_context_json TEXT NOT NULL DEFAULT 'null',
  order_side TEXT,
  order_type TEXT,
  time_in_force TEXT,
  post_only INTEGER,
  reduce_only INTEGER,
  order_qty TEXT,
  order_px TEXT,
  rejection_reason TEXT,
  ts_event BIGINT NOT NULL,
  ts_init BIGINT NOT NULL,
  ts_ingest BIGINT NOT NULL,
  reconciliation INTEGER NOT NULL DEFAULT 0,
  payload_json TEXT NOT NULL DEFAULT '{{}}',
  created_at TEXT NOT NULL,
  source_profile TEXT NOT NULL,
  source_host TEXT NOT NULL,
  shipped_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (source_profile, trader_id, event_id)
);
CREATE INDEX IF NOT EXISTS order_action_source_strategy_ts_event_idx
  ON {schema}.order_action (source_profile, strategy_id, ts_event);
CREATE INDEX IF NOT EXISTS order_action_source_client_order_ts_event_idx
  ON {schema}.order_action (source_profile, client_order_id, ts_event);
CREATE INDEX IF NOT EXISTS order_action_source_quote_cycle_id_idx
  ON {schema}.order_action (source_profile, quote_cycle_id);
CREATE INDEX IF NOT EXISTS order_action_source_trader_strategy_state_ts_event_idx
  ON {schema}.order_action (source_profile, trader_id, strategy_id, action_type, action_state, ts_event);
""",
    "portfolio_inventory_snapshot": """\
CREATE TABLE IF NOT EXISTS {schema}.portfolio_inventory_snapshot (
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
  stale_after_ms BIGINT NOT NULL DEFAULT 0,
  ts_ms BIGINT NOT NULL,
  ts_ingest_ns BIGINT NOT NULL,
  created_at TEXT NOT NULL,
  source_profile TEXT NOT NULL,
  source_host TEXT NOT NULL,
  shipped_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (source_profile, portfolio_id, base_currency, snapshot_id)
);
CREATE INDEX IF NOT EXISTS portfolio_inventory_snapshot_source_portfolio_ts_ms_idx
  ON {schema}.portfolio_inventory_snapshot (source_profile, portfolio_id, base_currency, ts_ms);
""",
    "quote_cycle": """\
CREATE TABLE IF NOT EXISTS {schema}.quote_cycle (
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
  trigger_md_ts_event_ns BIGINT,
  trigger_md_ts_init_ns BIGINT,
  ts_cycle_start_ns BIGINT,
  ts_cycle_end_ns BIGINT,
  state_from TEXT,
  state_to TEXT,
  cancel_count INTEGER,
  place_count INTEGER,
  bid_levels INTEGER,
  ask_levels INTEGER,
  decision_context_json TEXT NOT NULL DEFAULT 'null',
  created_at TEXT NOT NULL,
  source_profile TEXT NOT NULL,
  source_host TEXT NOT NULL,
  shipped_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (source_profile, trader_id, quote_cycle_id)
);
CREATE INDEX IF NOT EXISTS quote_cycle_source_strategy_ts_cycle_start_ns_idx
  ON {schema}.quote_cycle (source_profile, strategy_id, ts_cycle_start_ns);
CREATE INDEX IF NOT EXISTS quote_cycle_source_run_seq_idx
  ON {schema}.quote_cycle (source_profile, run_id, quote_cycle_seq);
CREATE INDEX IF NOT EXISTS quote_cycle_source_reason_ts_cycle_start_ns_idx
  ON {schema}.quote_cycle (source_profile, reason_code, ts_cycle_start_ns);
""",
}


class TelemetryPostgresSink:
    def __init__(self, config: TelemetryPostgresConfig) -> None:
        self._config = config
        self._conn = None

    def close(self) -> None:
        if self._conn is not None:
            self._conn.close()
            self._conn = None

    def ensure_schema(self) -> None:
        from psycopg import sql

        conn = self._get_conn()
        with conn.cursor() as cursor:
            cursor.execute(
                sql.SQL("CREATE SCHEMA IF NOT EXISTS {}").format(
                    sql.Identifier(self._config.schema),
                ),
            )
            for ddl in TABLE_CREATE_SQL.values():
                cursor.execute(ddl.format(schema=self._config.schema))
        conn.commit()

    def validate_tables(self, table_names: Iterable[str]) -> None:
        conn = self._get_conn()
        with conn.cursor() as cursor:
            for table_name in table_names:
                cursor.execute("SELECT to_regclass(%s)", (f"{self._config.schema}.{table_name}",))
                result = cursor.fetchone()
                if result is None or result[0] is None:
                    raise RuntimeError(
                        f"Telemetry sink table `{self._config.schema}.{table_name}` is missing. "
                        "Run the bootstrap command first.",
                    )

    def insert_rows(self, table_name: str, rows: list[dict[str, Any]]) -> int:
        if not rows:
            return 0

        from psycopg import sql

        column_names = TABLE_COLUMN_NAMES[table_name]
        insert_columns = (*column_names, "source_profile", "source_host")
        conflict_columns = TABLE_PRIMARY_KEYS[table_name]
        query = sql.SQL(
            "INSERT INTO {}.{} ({}) VALUES ({}) ON CONFLICT ({}) DO NOTHING",
        ).format(
            sql.Identifier(self._config.schema),
            sql.Identifier(table_name),
            sql.SQL(", ").join(sql.Identifier(name) for name in insert_columns),
            sql.SQL(", ").join(sql.Placeholder() for _ in insert_columns),
            sql.SQL(", ").join(sql.Identifier(name) for name in conflict_columns),
        )
        params = [
            tuple(row.get(name) for name in insert_columns)
            for row in rows
        ]

        conn = self._get_conn()
        with conn.cursor() as cursor:
            cursor.executemany(query, params)
            inserted = cursor.rowcount if cursor.rowcount >= 0 else len(rows)
        conn.commit()
        return inserted

    def _get_conn(self):
        if self._conn is None or self._conn.closed:
            import psycopg

            self._conn = psycopg.connect(
                host=self._config.host,
                port=self._config.port,
                dbname=self._config.database,
                user=self._config.username,
                password=self._config.password,
                sslmode=self._config.sslmode,
                connect_timeout=int(self._config.connect_timeout_secs),
                application_name=self._config.application_name,
            )
        return self._conn
