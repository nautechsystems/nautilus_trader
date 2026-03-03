#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from __future__ import annotations

import importlib.util
import json
import os
from dataclasses import dataclass
import time
from pathlib import Path
from typing import Any

import redis
from flask import Flask
from flask import jsonify
from flask import request

DEFAULT_STRATEGY_ID = "bybit_binance_plumeusdt_makerv3"
DEFAULT_REDIS_HOST = "127.0.0.1"
DEFAULT_REDIS_PORT = 6379
DEFAULT_REDIS_DB = 0


PARAMS_DEFAULTS: dict[str, Any] = {
    "bot_on": True,
    "max_age_ms": 2_000,
    "bid_edge1": 0.0005,
    "ask_edge1": 0.0005,
    "distance1": 0.0002,
    "n_orders1": 2,
    "bid_edge2": 0.0012,
    "ask_edge2": 0.0012,
    "distance2": 0.0004,
    "n_orders2": 2,
    "bid_edge3": 0.0024,
    "ask_edge3": 0.0024,
    "distance3": 0.0008,
    "n_orders3": 2,
}

PARAMS_SCHEMA: dict[str, dict[str, Any]] = {
    "bot_on": {"type": "boolean", "description": "Enable quote publishing and quote management."},
    "max_age_ms": {"type": "integer", "description": "Replace managed orders older than this threshold."},
    "bid_edge1": {"type": "number", "description": "Band 1 bid edge offset."},
    "ask_edge1": {"type": "number", "description": "Band 1 ask edge offset."},
    "distance1": {"type": "number", "description": "Band 1 order spacing increment."},
    "n_orders1": {"type": "integer", "description": "Band 1 order depth per side."},
    "bid_edge2": {"type": "number", "description": "Band 2 bid edge offset."},
    "ask_edge2": {"type": "number", "description": "Band 2 ask edge offset."},
    "distance2": {"type": "number", "description": "Band 2 order spacing increment."},
    "n_orders2": {"type": "integer", "description": "Band 2 order depth per side."},
    "bid_edge3": {"type": "number", "description": "Band 3 bid edge offset."},
    "ask_edge3": {"type": "number", "description": "Band 3 ask edge offset."},
    "distance3": {"type": "number", "description": "Band 3 order spacing increment."},
    "n_orders3": {"type": "integer", "description": "Band 3 order depth per side."},
}


@dataclass(frozen=True)
class ContractMapping:
    exchange: str
    symbol: str


def _decode(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return "" if value is None else str(value)


def _load_json(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, (dict, list, int, float, bool)):
        return value
    text = _decode(value).strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def _coerce_ts_ms(value: Any) -> int | None:
    if value is None:
        return None
    try:
        raw = float(value)
    except (TypeError, ValueError):
        return None

    if raw <= 0:
        return None
    if raw < 1_000_000_000_000:
        return int(raw * 1_000)
    if raw >= 1_000_000_000_000_000_000:
        return int(raw / 1_000_000)
    if raw >= 1_000_000_000_000_000:
        return int(raw / 1000)
    return int(raw)


def _safe_float(value: Any) -> float | None:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _safe_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    if isinstance(value, tuple):
        return list(value)
    return [value]


def _load_contracts_module() -> Any | None:
    path = Path(__file__).resolve().parent / "contracts.py"
    if not path.exists():
        return None
    spec = importlib.util.spec_from_file_location("nautilus_poc_contracts", path)
    if spec is None or spec.loader is None:
        return None
    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)
        return module
    except Exception:
        return None


def _load_contract_mappings() -> tuple[ContractMapping, ...]:
    contracts_module = _load_contracts_module()
    if contracts_module is not None:
        entries = getattr(contracts_module, "INSTRUMENT_CONTRACTS", None)
        if entries:
            out: list[ContractMapping] = []
            for entry in entries:
                try:
                    out.append(ContractMapping(exchange=str(entry.chainsaw_exchange), symbol=str(entry.chainsaw_symbol)))
                except Exception:
                    continue
            if out:
                return tuple(out)

    return (
        ContractMapping(exchange="bybit_linear", symbol="PLUME/USDT"),
        ContractMapping(exchange="binance_spot", symbol="PLUME/USDT"),
    )


def _split_symbol(symbol: str) -> tuple[str, str]:
    if "/" in symbol:
        base, quote = symbol.split("/", maxsplit=1)
        if base and quote:
            return base.upper(), quote.upper()
    normalized = _decode(symbol).replace("-", "").upper()
    for quote in ("USDT", "USDC", "USD", "BTC", "ETH"):
        if normalized.endswith(quote) and len(normalized) > len(quote):
            return normalized[: -len(quote)], quote
    return "", ""


def _market_key(exchange: str, symbol: str) -> str:
    base, quote = _split_symbol(symbol)
    if not base or not quote:
        return ""
    return f"last:{exchange}:{base}_{quote}"


def _strategy_key(strategy_id: str) -> str:
    return f"maker_arb:{strategy_id}:state"


def _events_key(strategy_id: str) -> str:
    return f"maker_arb:{strategy_id}:events"


def _read_rows_from_list(redis_client: redis.Redis, key: str, limit: int | None = None) -> list[dict[str, Any]]:
    if limit is None or limit <= 0:
        limit = 200
    rows_raw = redis_client.lrange(key, 0, limit - 1)
    out: list[dict[str, Any]] = []
    for item in rows_raw:
        parsed = _load_json(item)
        if isinstance(parsed, dict):
            out.append(dict(parsed))
            continue
        if isinstance(parsed, list):
            for sub in parsed:
                if isinstance(sub, dict):
                    out.append(dict(sub))
            continue
        out.append({"value": parsed})
    return out


def _strategy_id_from_row(row: Any, fallback: str) -> str:
    if isinstance(row, dict):
        value = _decode(row.get("strategy_id")).strip()
        if value:
            return value
    return fallback


def _build_legs(redis_client: redis.Redis) -> dict[str, Any]:
    now_ms = _now_ms()
    out: dict[str, Any] = {}
    for contract in _load_contract_mappings():
        key = _market_key(contract.exchange, contract.symbol)
        row = _load_json(redis_client.get(key)) if key else {}
        if not isinstance(row, dict):
            row = {}
        bid = _safe_float(row.get("bid"))
        ask = _safe_float(row.get("ask"))
        ts_ms = _coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        age_ms = (now_ms - ts_ms) if ts_ms else None
        out[contract.exchange] = {
            "exchange": contract.exchange,
            "symbol": contract.symbol,
            "bid": bid,
            "ask": ask,
            "mid": (bid + ask) / 2 if bid is not None and ask is not None else None,
            "ts_ms": ts_ms,
            "age_ms": age_ms,
            "state": row.get("state") or "",
        }
    return out


def _build_signals_payload(redis_client: redis.Redis, strategy_id: str) -> dict[str, Any]:
    state = _load_json(redis_client.get(_strategy_key(strategy_id)))
    if not isinstance(state, dict):
        state = {}

    fvs = _as_list(_load_json(redis_client.get("fvs.snapshot")))
    fv_row: dict[str, Any] = {}
    for row in fvs:
        if isinstance(row, dict) and _strategy_id_from_row(row, strategy_id) == strategy_id:
            fv_row = row
            break
    if not fv_row and fvs:
        if isinstance(fvs[0], dict):
            fv_row = fvs[0]

    bot_on = bool(state.get("bot_on", False))
    managed = _safe_int(state.get("managed_orders")) or 0
    ts_ms = _coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))

    return {
        "id": strategy_id,
        "meta": {
            "class": "maker_v3",
            "strategy_groups": "tokenmm",
            "base_asset": "PLUME",
            "quote_asset": "USDT",
        },
        "tradeable": bot_on,
        "blocked": not bot_on,
        "near_tradeable": False,
        "managed_orders": managed,
        "maker_v3": {
            "quote_snapshot": {
                "mode": "ON" if bot_on else "OFF",
                "reason": str(state.get("state", "")),
                "ts_ms": ts_ms,
            },
        },
        "state": state,
        "legs": _build_legs(redis_client),
        "fv_row": fv_row,
    }


def _build_balances_payload(redis_client: redis.Redis, strategy_id: str) -> list[dict[str, Any]]:
    raw = _load_json(redis_client.get("balances.snapshot"))
    rows = _as_list(raw)
    out: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        row = dict(row)
        sid = _strategy_id_from_row(row, strategy_id)
        row["strategy_id"] = sid
        accounts = row.get("accounts")
        if isinstance(accounts, list) and accounts:
            for index, account in enumerate(accounts):
                if not isinstance(account, dict):
                    continue
                out.append({**account, "strategy_id": sid, "row_id": f"{strategy_id}:acc:{index}"})
            continue
        out.append(row)

    return [row for row in out if _strategy_id_from_row(row, strategy_id) == strategy_id]


def _build_trades_payload(redis_client: redis.Redis, strategy_id: str, limit: int, since_ms: int | None) -> list[dict[str, Any]]:
    rows = _read_rows_from_list(redis_client, "trades.blotter", limit=limit)
    filtered: list[dict[str, Any]] = []
    for row in rows:
        if _strategy_id_from_row(row, strategy_id) != strategy_id:
            continue
        row_ts = _coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        if row_ts is not None:
            row["ts_ms"] = row_ts
        if since_ms is not None and row_ts is not None and row_ts <= since_ms:
            continue
        filtered.append(row)

    filtered.sort(key=lambda item: _coerce_ts_ms(item.get("ts_ms")) or 0, reverse=True)
    return filtered


def _build_alerts_payload(redis_client: redis.Redis, strategy_id: str, limit: int) -> list[dict[str, Any]]:
    rows = _read_rows_from_list(redis_client, "alerts.blotter", limit=limit)
    filtered = [row for row in rows if _strategy_id_from_row(row, strategy_id) == strategy_id]
    filtered.sort(key=lambda item: _coerce_ts_ms(item.get("ts_ms") or item.get("ts") or item.get("timestamp")) or 0, reverse=True)
    return filtered


def _now_ms() -> int:
    return int(time.time() * 1000)


def _is_redis_healthy(redis_client: redis.Redis) -> bool:
    try:
        return bool(redis_client.ping())
    except redis.RedisError:
        return False


BASE_HTML_TEMPLATE = """<!doctype html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\" />
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
    <title>Nautilus TokenMM</title>
    <style>
      :root { color-scheme: dark; }
      body { margin: 0; font-family: Arial, sans-serif; background: #081025; color: #d8e2ff; }
      .wrap { max-width: 1200px; margin: 0 auto; padding: 18px; }
      nav a { display: inline-block; margin-right: 10px; color: #7fd1ff; text-decoration: none; border: 1px solid #233456; border-radius: 8px; padding: 6px 10px; }
      nav a.active { background: #12213c; }
      .card { background: #101c35; border: 1px solid #223357; border-radius: 10px; padding: 12px; margin-top: 12px; }
      table { width: 100%; border-collapse: collapse; font-size: 13px; }
      th, td { border-bottom: 1px solid #223357; text-align: left; padding: 8px 6px; }
      th { color: #93c5fd; }
      .mono { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; }
      .status { margin-top: 8px; font-size: 12px; color: #9caecf; }
      .ok { color: #86efac; }
      .warn { color: #facc15; }
      .err { color: #f87171; }
      pre { max-height: 280px; overflow: auto; white-space: pre-wrap; }
    </style>
  </head>
  <body>
    <div class=\"wrap\">
      <h1>Nautilus TokenMM</h1>
      <nav id=\"menu\"></nav>
      <div id=\"status\" class=\"status mono\">loading</div>
      <div class=\"card\" id=\"content\"></div>
    </div>
    <script>
      const ROUTE = window.location.pathname;
      const STRATEGY = new URLSearchParams(window.location.search).get('strategy') || '__DEFAULT_STRATEGY__';
      const ENDPOINTS = {
        '/tokenmm': '/api/v1/signals',
        '/tokenmm/signal': '/api/v1/signals',
        '/tokenmm/params': '/api/v1/params',
        '/tokenmm/balances': '/api/v1/balances',
        '/tokenmm/trades': '/api/v1/trades',
        '/tokenmm/alerts': '/api/v1/alerts',
      };

      const pages = [
        ['/tokenmm/signal', 'Signal'],
        ['/tokenmm/params', 'Params'],
        ['/tokenmm/balances', 'Balances'],
        ['/tokenmm/trades', 'Trades'],
        ['/tokenmm/alerts', 'Alerts'],
      ];

      const fmt = (value) => (value === null || value === undefined ? '-' : String(value));
      const fmtTs = (value) => {
        const v = Number(value);
        if (!Number.isFinite(v) || v <= 0) {
          return '-';
        }
        return new Date(v).toISOString();
      };

      const renderMenu = () => {
        const links = pages.map(([href, label]) => {
          const active = ROUTE.startsWith(href) ? 'active' : '';
          return `<a href=\"${href}?strategy=${STRATEGY}\" class=\"${active}\">${label}</a>`;
        }).join('');
        document.getElementById('menu').innerHTML = links;
      };

      const renderSignal = async () => {
        const data = await (await fetch('/api/v1/signals')).json();
        const strategy = (data?.data?.strategies || [])[0] || {};
        const legs = Object.entries(strategy.legs || {}).map(([name, row]) => `<tr><td>${name}</td><td>${row.exchange}</td><td>${fmt(row.bid)}</td><td>${fmt(row.ask)}</td><td>${fmt(row.mid)}</td><td>${fmt(row.age_ms)} ms</td></tr>`).join('');
        const fv = strategy.fv_row || {};
        document.getElementById('content').innerHTML = `
          <div class=\"mono\">strategy=${strategy.id || '-'} blocked=${strategy.blocked}</div>
          <h3>Market BBO</h3>
          <table><thead><tr><th>Leg</th><th>Exchange</th><th>Bid</th><th>Ask</th><th>Mid</th><th>Age</th></tr></thead><tbody>${legs || '<tr><td colspan=\"6\">No data</td></tr>'}</tbody></table>
          <h3>Last FV</h3>
          <pre class=\"mono\">${JSON.stringify(fv, null, 2)}</pre>
          <h3>State</h3>
          <pre class=\"mono\">${JSON.stringify(strategy.state || {}, null, 2)}</pre>
        `;
      };

      const renderParams = async () => {
        const data = await (await fetch('/api/v1/params')).json();
        const row = (data?.data?.strategies || [])[0] || {};
        const params = row.params || {};
        const schema = row.schema || {};
        const body = Object.entries(params).map(([name, value]) => {
          const meta = schema[name] || {};
          return `<tr><td>${name}</td><td>${fmt(value)}</td><td>${meta.type || '-'}</td><td>${meta.description || '-'}</td></tr>`;
        }).join('');
        document.getElementById('content').innerHTML = `<table><thead><tr><th>Param</th><th>Value</th><th>Type</th><th>Description</th></tr></thead><tbody>${body}</tbody></table>`;
      };

      const renderBalances = async () => {
        const data = await (await fetch('/api/v1/balances')).json();
        const rows = data?.data?.rows || [];
        const body = rows.map((row) => `<tr><td>${fmt(row.strategy_id)}</td><td>${fmt(row.exchange || row.venue || '')}</td><td>${fmt(row.base || row.coin || '')}</td><td>${fmt(row.free)}</td><td>${fmt(row.locked)}</td><td>${fmt(row.total)}</td></tr>`).join('');
        document.getElementById('content').innerHTML = `<table><thead><tr><th>strategy_id</th><th>exchange</th><th>asset</th><th>free</th><th>locked</th><th>total</th></tr></thead><tbody>${body || '<tr><td colspan=\"6\">No balances</td></tr>'}</tbody></table>`;
      };

      const renderTrades = async () => {
        const data = await (await fetch('/api/v1/trades')).json();
        const rows = data?.data?.rows || [];
        const body = rows.map((row) => `<tr><td>${fmtTs(row.ts_ms)}</td><td>${fmt(row.side || row.order_side)}</td><td>${fmt(row.qty)}</td><td>${fmt(row.price)}</td><td>${fmt(row.symbol || row.coin)}</td><td>${fmt(row.venue || row.exchange)}</td></tr>`).join('');
        document.getElementById('content').innerHTML = `<table><thead><tr><th>time</th><th>side</th><th>qty</th><th>price</th><th>symbol</th><th>venue</th></tr></thead><tbody>${body || '<tr><td colspan=\"6\">No trades</td></tr>'}</tbody></table>`;
      };

      const renderAlerts = async () => {
        const data = await (await fetch('/api/v1/alerts')).json();
        const rows = data?.data?.rows || [];
        const body = rows.map((row) => `<tr><td>${fmtTs(row.ts_ms)}</td><td>${fmt(row.level)}</td><td>${fmt(row.message || row.event || row.text)}</td><td>${fmt(row.reason)}</td></tr>`).join('');
        document.getElementById('content').innerHTML = `<table><thead><tr><th>time</th><th>level</th><th>message</th><th>reason</th></tr></thead><tbody>${body || '<tr><td colspan=\"4\">No alerts</td></tr>'}</tbody></table>`;
      };

      const renderMap = {
        '/tokenmm/signal': renderSignal,
        '/tokenmm/params': renderParams,
        '/tokenmm/balances': renderBalances,
        '/tokenmm/trades': renderTrades,
        '/tokenmm/alerts': renderAlerts,
      };

      const render = async () => {
        const target = ROUTE === '/tokenmm' ? '/tokenmm/signal' : ROUTE;
        const fn = renderMap[target] || renderSignal;
        try {
          const health = await (await fetch('/api/v1/healthz')).json();
          const ok = health?.ok ? 'ok' : 'error';
          document.getElementById('status').innerHTML = `${ok} redis=${health?.data?.redis_available ? 'up' : 'down'} strategy=${STRATEGY}`;
          await fn();
        } catch (err) {
          document.getElementById('status').innerHTML = `<span class=\"err\">dashboard error</span> ${err}`;
        }
      };

      renderMenu();
      render();
      setInterval(render, 2000);
    </script>
  </body>
</html>
""".replace("__DEFAULT_STRATEGY__", DEFAULT_STRATEGY_ID)


def build_app() -> Flask:
    app = Flask(__name__)

    redis_host = os.getenv("POC_REDIS_HOST", DEFAULT_REDIS_HOST)
    redis_port = int(os.getenv("POC_REDIS_PORT", str(DEFAULT_REDIS_PORT)))
    redis_db = int(os.getenv("POC_REDIS_DB", str(DEFAULT_REDIS_DB)))
    redis_username = os.getenv("POC_REDIS_USERNAME") or None
    redis_password = os.getenv("POC_REDIS_PASSWORD") or None

    redis_client = redis.Redis(
        host=redis_host,
        port=redis_port,
        db=redis_db,
        username=redis_username,
        password=redis_password,
        decode_responses=False,
    )

    @app.get("/")
    @app.get("/tokenmm")
    @app.get("/tokenmm/signal")
    @app.get("/tokenmm/params")
    @app.get("/tokenmm/balances")
    @app.get("/tokenmm/trades")
    @app.get("/tokenmm/alerts")
    def tokenmm() -> str:
        return BASE_HTML_TEMPLATE

    @app.get("/api/v1/healthz")
    def health() -> Any:
        redis_ok = _is_redis_healthy(redis_client)
        fvs = _as_list(_load_json(redis_client.get("fvs.snapshot")))
        return jsonify(
            {
                "ok": redis_ok,
                "data": {
                    "redis_available": redis_ok,
                    "has_fvs": bool(fvs),
                    "time_ms": _now_ms(),
                },
                "error": None if redis_ok else "redis_unavailable",
            },
        )

    @app.get("/api/v1/readyz")
    def ready() -> Any:
        redis_ok = _is_redis_healthy(redis_client)
        fvs = _as_list(_load_json(redis_client.get("fvs.snapshot")))
        ready = bool(redis_ok and fvs)
        return jsonify(
            {
                "ok": ready,
                "data": {
                    "redis_available": redis_ok,
                    "has_fvs": bool(fvs),
                },
                "error": None if ready else "service_not_ready",
            },
        ), (200 if ready else 503)

    @app.get("/api/v1/signals")
    def api_signals() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        data = {
            "server_ts_ms": _now_ms(),
            "strategies": [_build_signals_payload(redis_client, strategy_id=sid)],
        }
        return jsonify({"ok": True, "data": data, "error": None}), 200

    @app.get("/api/v1/param-schema")
    def api_param_schema() -> Any:
        return jsonify({"ok": True, "data": {"schema": PARAMS_SCHEMA}, "error": None}), 200

    @app.get("/api/v1/params")
    def api_params() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        payload = {
            "strategies": [
                {
                    "strategy_id": sid,
                    "params": PARAMS_DEFAULTS,
                    "schema": PARAMS_SCHEMA,
                },
            ],
        }
        return jsonify({"ok": True, "data": payload, "error": None}), 200

    @app.get("/api/v1/strategies")
    def api_strategies() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        return jsonify(
            {
                "ok": True,
                "data": {
                    "strategies": [_build_signals_payload(redis_client, strategy_id=sid)],
                    "count": 1,
                },
                "error": None,
            },
        ), 200

    @app.get("/api/v1/strategies/<string:strategy_id>/parameters")
    def api_strategy_parameters(strategy_id: str) -> Any:
        sid = strategy_id or DEFAULT_STRATEGY_ID
        return jsonify(
            {
                "ok": True,
                "data": {
                    "strategy_id": sid,
                    "params": PARAMS_DEFAULTS,
                    "schema": PARAMS_SCHEMA,
                },
                "error": None,
            },
        ), 200

    @app.get("/api/v1/balances")
    def api_balances() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        rows = _build_balances_payload(redis_client, sid)
        return jsonify(
            {
                "ok": True,
                "data": {
                    "rows": rows[: max(1, min(200, limit))],
                    "count": len(rows),
                    "server_ts_ms": _now_ms(),
                },
                "error": None,
            },
        ), 200

    @app.get("/api/v1/trades")
    def api_trades() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        rows = _build_trades_payload(redis_client, sid, limit=max(1, min(200, limit)), since_ms=None)
        return jsonify({"ok": True, "data": {"rows": rows, "count": len(rows), "server_ts_ms": _now_ms()}, "error": None}), 200

    @app.get("/api/v1/trades/delta")
    def api_trades_delta() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        since_ms = _coerce_ts_ms(request.args.get("after"))
        rows = _build_trades_payload(redis_client, sid, limit=max(1, min(200, limit)), since_ms=since_ms)
        return jsonify({"ok": True, "data": {"rows": rows, "count": len(rows), "server_ts_ms": _now_ms(), "after": since_ms}, "error": None}), 200

    @app.get("/api/v1/alerts")
    def api_alerts() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        limit = _safe_int(request.args.get("limit")) or 50
        rows = _build_alerts_payload(redis_client, sid, limit=max(1, min(200, limit)))
        return jsonify({"ok": True, "data": {"rows": rows, "count": len(rows), "server_ts_ms": _now_ms()}, "error": None}), 200

    return app


def main() -> None:
    app = build_app()
    host = os.getenv("HOST", "0.0.0.0")
    port = int(os.getenv("PORT", "5022"))
    app.run(host=host, port=port, debug=False, use_reloader=False)


if __name__ == "__main__":
    main()
