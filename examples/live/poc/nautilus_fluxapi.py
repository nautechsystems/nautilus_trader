#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import time
from datetime import datetime, timezone
from typing import Any

import redis
from flask import Flask
from flask import jsonify
from flask import request


DEFAULT_STRATEGY_ID = "bybit_binance_plumeusdt_makerv3"
DEFAULT_BYBIT_LAST_KEY = "last:bybit_linear:PLUME_USDT"
DEFAULT_BINANCE_LAST_KEY = "last:binance_spot:PLUME_USDT"

HTML_PAGE = """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Nautilus POC TokenMM</title>
  <style>
    :root { color-scheme: dark; }
    body { font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Arial; margin: 20px; background: #0b1220; color: #dbe5ff; }
    h1 { margin: 0 0 8px 0; }
    .muted { color: #94a3b8; margin-bottom: 14px; }
    .card { border: 1px solid #223250; border-radius: 10px; padding: 12px; margin: 12px 0; background: #101a2d; }
    table { width: 100%; border-collapse: collapse; font-size: 14px; }
    th, td { border-bottom: 1px solid #223250; text-align: left; padding: 8px 6px; }
    th { color: #93c5fd; }
    .mono { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
    .badge { padding: 2px 8px; border-radius: 999px; font-size: 12px; border: 1px solid #334155; }
    .ok { color: #86efac; }
    .warn { color: #fcd34d; }
  </style>
</head>
<body>
  <h1>Nautilus POC TokenMM</h1>
  <div class="muted">No Chainsaw runtime. Data source: Redis stream + keys produced by Nautilus node and bridge.</div>

  <div class="card">
    <div><b>Strategy</b> <span id="sid" class="mono"></span> <span id="mode" class="badge"></span></div>
    <div id="ts" class="muted"></div>
    <table>
      <thead>
        <tr><th>Leg</th><th>Exchange</th><th>Bid</th><th>Ask</th><th>Mid</th><th>Age ms</th></tr>
      </thead>
      <tbody id="legs"></tbody>
    </table>
  </div>

  <div class="card">
    <b>FV Snapshot</b>
    <pre id="fv" class="mono"></pre>
  </div>

  <div class="card">
    <b>Balances Snapshot</b>
    <pre id="balances" class="mono"></pre>
  </div>

  <script>
    async function tick() {
      const res = await fetch('/api/v1/signals?profile=tokenmm');
      const payload = await res.json();
      const strat = payload?.data?.strategies?.[0];
      if (!strat) return;
      document.getElementById('sid').textContent = strat.id || 'unknown';
      const mode = strat?.maker_v3?.quote_snapshot?.mode || 'UNKNOWN';
      const modeEl = document.getElementById('mode');
      modeEl.textContent = mode;
      modeEl.className = 'badge ' + (mode === 'OFF' ? 'warn' : 'ok');
      document.getElementById('ts').textContent = 'server_ts_ms=' + (payload?.data?.server_ts_ms ?? 'n/a');

      const legs = strat.legs || {};
      const rows = [];
      for (const name of ['A', 'B']) {
        const leg = legs[name] || {};
        rows.push(`<tr>
          <td class="mono">${name}</td>
          <td class="mono">${leg.exchange || '-'}</td>
          <td class="mono">${leg.decision_bid ?? '-'}</td>
          <td class="mono">${leg.decision_ask ?? '-'}</td>
          <td class="mono">${leg.mid ?? '-'}</td>
          <td class="mono">${leg.md_age_ms ?? '-'}</td>
        </tr>`);
      }
      document.getElementById('legs').innerHTML = rows.join('');

      document.getElementById('fv').textContent = JSON.stringify(strat.fv_row || {}, null, 2);
      document.getElementById('balances').textContent = JSON.stringify(payload?.data?.balances || [], null, 2);
    }

    tick();
    setInterval(tick, 2000);
  </script>
</body>
</html>
"""


def _now_ms() -> int:
    return int(time.time() * 1000)


def _decode_text(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return "" if value is None else str(value)


def _json_load(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, (dict, list, int, float, bool)):
        return value
    text = _decode_text(value).strip()
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
        num = float(value)
    except (TypeError, ValueError):
        return None
    if num <= 0:
        return None
    if num < 1_000_000_000_000:
        return int(num * 1000)
    if num >= 1_000_000_000_000_000_000:
        return int(num / 1_000_000)
    if num >= 1_000_000_000_000_000:
        return int(num / 1000)
    return int(num)


def _safe_float(value: Any) -> float | None:
    try:
        if value is None:
            return None
        return float(value)
    except (TypeError, ValueError):
        return None


def _mid(bid: Any, ask: Any) -> float | None:
    bid_f = _safe_float(bid)
    ask_f = _safe_float(ask)
    if bid_f is None or ask_f is None:
        return None
    return (bid_f + ask_f) / 2.0


def _build_leg(row: dict[str, Any] | None, now_ms: int) -> dict[str, Any]:
    row = row or {}
    bid = _safe_float(row.get("bid") or row.get("best_bid"))
    ask = _safe_float(row.get("ask") or row.get("best_ask"))
    ts_ms = _coerce_ts_ms(row.get("ts_ms") or row.get("timestamp") or row.get("ts_event"))
    age_ms = max(0, now_ms - ts_ms) if ts_ms is not None else None
    return {
        "exchange": _decode_text(row.get("chainsaw_exchange") or row.get("exchange") or ""),
        "symbol": _decode_text(row.get("symbol") or ""),
        "decision_bid": bid,
        "decision_ask": ask,
        "mid": _mid(bid, ask),
        "md_ts_ms": ts_ms,
        "md_age_ms": age_ms,
    }


def _first_fv_row(fv_payload: Any, strategy_id: str) -> dict[str, Any]:
    if not isinstance(fv_payload, list):
        return {}
    for row in fv_payload:
        if isinstance(row, dict) and _decode_text(row.get("strategy_id")) == strategy_id:
            return row
    for row in fv_payload:
        if isinstance(row, dict):
            return row
    return {}


def _build_payload(
    client: redis.Redis,
    *,
    strategy_id: str,
    bybit_last_key: str,
    binance_last_key: str,
) -> dict[str, Any]:
    now_ms = _now_ms()
    state = _json_load(client.get(f"maker_arb:{strategy_id}:state")) or {}
    bybit_row = _json_load(client.get(bybit_last_key)) or {}
    binance_row = _json_load(client.get(binance_last_key)) or {}
    fv_payload = _json_load(client.get("fvs.snapshot")) or []
    balances = _json_load(client.get("balances.snapshot")) or []

    bot_on = bool(state.get("bot_on", False))
    mode = "ON" if bot_on else "OFF"

    strategy = {
        "id": strategy_id,
        "meta": {
            "class": "maker_v3",
            "strategy_groups": "tokenmm",
            "base_asset": "PLUME",
            "quote_asset": "USDT",
        },
        "blocked": not bot_on,
        "tradeable": bool(bot_on),
        "near_tradeable": False,
        "maker_v3": {
            "quote_snapshot": {
                "mode": mode,
                "reason": _decode_text(state.get("state") or ("bot_on" if bot_on else "bot_off")),
                "ts_ms": _coerce_ts_ms(state.get("ts_event")),
            },
        },
        "legs": {
            "A": _build_leg(bybit_row, now_ms),
            "B": _build_leg(binance_row, now_ms),
        },
        "fv_row": _first_fv_row(fv_payload, strategy_id),
    }

    server_time = datetime.fromtimestamp(now_ms / 1000, tz=timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
    return {
        "strategies": [strategy],
        "balances": balances if isinstance(balances, list) else [],
        "server_time": server_time,
        "server_ts_ms": now_ms,
    }


def build_app() -> Flask:
    app = Flask(__name__)

    redis_host = os.getenv("POC_REDIS_HOST", "127.0.0.1")
    redis_port = int(os.getenv("POC_REDIS_PORT", "6380"))
    redis_db = int(os.getenv("POC_REDIS_DB", "0"))
    redis_username = os.getenv("POC_REDIS_USERNAME") or None
    redis_password = os.getenv("POC_REDIS_PASSWORD") or None
    strategy_id = os.getenv("POC_STRATEGY_ID", DEFAULT_STRATEGY_ID)
    bybit_last_key = os.getenv("POC_BYBIT_LAST_KEY", DEFAULT_BYBIT_LAST_KEY)
    binance_last_key = os.getenv("POC_BINANCE_LAST_KEY", DEFAULT_BINANCE_LAST_KEY)

    client = redis.Redis(
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
    @app.get("/tokenmm/order-view")
    def tokenmm_index() -> str:
        return HTML_PAGE

    @app.get("/api/v1/healthz")
    def healthz() -> Any:
        return jsonify({"ok": True}), 200

    @app.get("/api/v1/readyz")
    def readyz() -> Any:
        try:
            pong = bool(client.ping())
        except redis.RedisError:
            pong = False
        payload = _build_payload(
            client,
            strategy_id=strategy_id,
            bybit_last_key=bybit_last_key,
            binance_last_key=binance_last_key,
        )
        has_fvs = bool(payload["strategies"][0].get("fv_row"))
        ready = bool(pong and has_fvs)
        data = {
            "ready": ready,
            "redis": pong,
            "has_fvs": has_fvs,
            "redis_target": {
                "host": redis_host,
                "port": redis_port,
                "db": redis_db,
            },
        }
        return jsonify({"ok": ready, "data": data, "error": None if ready else "not_ready"}), (200 if ready else 503)

    @app.get("/api/v1/signals")
    def signals() -> Any:
        _ = request.args.get("profile")
        payload = _build_payload(
            client,
            strategy_id=strategy_id,
            bybit_last_key=bybit_last_key,
            binance_last_key=binance_last_key,
        )
        return jsonify({"ok": True, "data": payload, "error": None}), 200

    return app


def main() -> None:
    app = build_app()
    port = int(os.getenv("PORT", "5022"))
    host = os.getenv("HOST", "0.0.0.0")
    app.run(host=host, port=port, debug=False, use_reloader=False)


if __name__ == "__main__":
    main()
