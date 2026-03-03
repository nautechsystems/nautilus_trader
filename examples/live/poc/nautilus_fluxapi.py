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


def _safe_bool(value: Any) -> bool | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        if value in (0, 0.0):
            return False
        if value in (1, 1.0):
            return True
    text = _decode(value).strip().lower()
    if not text:
        return None
    if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
        return True
    if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
        return False
    return None


def _as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    if isinstance(value, tuple):
        return list(value)
    return [value]


def _param_key(strategy_id: str, name: str) -> str:
    return f"strategy.{strategy_id}.{name}"


def _coerce_param_value(name: str, value: Any, schema_type: str) -> Any:
    if schema_type == "boolean":
        result = _safe_bool(value)
        if result is None:
            raise ValueError(f"Invalid boolean value for param '{name}': {value!r}")
        return result
    if schema_type == "integer":
        try:
            return int(value)
        except (TypeError, ValueError):
            raise ValueError(f"Invalid integer value for param '{name}': {value!r}")
    if schema_type == "number":
        try:
            return float(value)
        except (TypeError, ValueError):
            raise ValueError(f"Invalid number value for param '{name}': {value!r}")
    return _decode(value)


def _param_value_to_redis(value: Any) -> str:
    if isinstance(value, bool):
        return "1" if value else "0"
    return str(value)


def _load_param_value(
    redis_client: redis.Redis,
    strategy_id: str,
    name: str,
    schema_type: str,
) -> Any | None:
    raw = redis_client.get(_param_key(strategy_id, name))
    if raw is None:
        return None
    loaded = _load_json(raw)
    value = _decode(raw) if loaded is None else loaded
    return _coerce_param_value(name, value, schema_type)


def _load_params(redis_client: redis.Redis, strategy_id: str) -> dict[str, Any]:
    params: dict[str, Any] = {}
    for name, schema in PARAMS_SCHEMA.items():
        schema_type = str(schema.get("type", "number"))
        value = _load_param_value(redis_client, strategy_id, name, schema_type)
        if value is None:
            value = PARAMS_DEFAULTS.get(name)
        params[name] = value
    return params


def _read_params_request_payload() -> dict[str, Any]:
    payload = request.get_json(silent=True)
    if not isinstance(payload, dict):
        return {}
    if isinstance(payload.get("params"), dict):
        return payload["params"]
    return {key: value for key, value in payload.items() if key != "source"}


def _build_params_payload(redis_client: redis.Redis, strategy_id: str) -> dict[str, Any]:
    return {
        "strategy_id": strategy_id,
        "params": _load_params(redis_client, strategy_id),
        "schema": PARAMS_SCHEMA,
    }


def _apply_params_update(redis_client: redis.Redis, strategy_id: str, updates: dict[str, Any]) -> dict[str, Any]:
    if not updates:
        params = _load_params(redis_client, strategy_id)
        return {"updated": [], "params": params}

    updated: list[str] = []
    for key, value in updates.items():
        schema = PARAMS_SCHEMA.get(key)
        if schema is None:
            raise ValueError(f"Unknown parameter '{key}' for strategy '{strategy_id}'")
        schema_type = str(schema.get("type", "number"))
        coerced = _coerce_param_value(key, value, schema_type)
        redis_client.set(_param_key(strategy_id, key), _param_value_to_redis(coerced))
        updated.append(key)

    return {"updated": sorted(updated), "params": _load_params(redis_client, strategy_id)}


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
      .toolbar { margin-top: 8px; }
      .toolbar button { padding: 6px 10px; border: 1px solid #294066; border-radius: 8px; background: #1d2f50; color: #d8e2ff; cursor: pointer; }
      .toolbar button:disabled { opacity: 0.5; cursor: not-allowed; }
      .sub { color: #9aaecf; margin-top: 8px; }
      .section { margin-top: 12px; }
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
      const STRATEGY_QUERY = `strategy=${encodeURIComponent(STRATEGY)}`;

      const pages = [
        ['/tokenmm', 'Home'],
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
      const escapeHtml = (value) => String(value ?? '').replace(/[&<>"']/g, (char) => {
        const map = {
          '&': '&amp;',
          '<': '&lt;',
          '>': '&gt;',
          '\"': '&quot;',
          "'": '&#39;',
        };
        return map[char];
      });
      const apiGet = async (path) => {
        const response = await fetch(`${path}?${STRATEGY_QUERY}`);
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`);
        }
        return response.json();
      };

      const renderMenu = () => {
        const links = pages.map(([href, label]) => {
          const active = href === '/tokenmm' ? (ROUTE === '/tokenmm' || ROUTE === '/' ? 'active' : '') : ROUTE === href ? 'active' : '';
          return `<a href=\"${href}?strategy=${STRATEGY}\" class=\"${active}\">${label}</a>`;
        }).join('');
        document.getElementById('menu').innerHTML = links;
      };

      const renderSignalPanel = (strategy, fvRow, legs) => {
        const rows = Object.entries(legs || {}).map(
          ([name, row]) =>
            `<tr><td>${escapeHtml(name)}</td><td>${escapeHtml(row.exchange)}</td><td>${escapeHtml(row.bid)}</td><td>${escapeHtml(row.ask)}</td><td>${escapeHtml(row.mid)}</td><td>${escapeHtml(row.age_ms)} ms</td></tr>`,
        ).join('');
        return `
          <div class=\"section card\">
            <h3>Signal</h3>
            <div class=\"mono\">strategy=${escapeHtml(strategy.id || '-')} blocked=${escapeHtml(strategy.blocked)} managed_orders=${escapeHtml(strategy.managed_orders || 0)} bot_state=${escapeHtml(strategy.tradeable ? 'on' : 'off')}</div>
            <h4>Market BBO</h4>
            <table><thead><tr><th>Leg</th><th>Exchange</th><th>Bid</th><th>Ask</th><th>Mid</th><th>Age</th></tr></thead><tbody>${rows || '<tr><td colspan=\"6\">No data</td></tr>'}</tbody></table>
            <h4>Last FV</h4>
            <pre class=\"mono\">${escapeHtml(JSON.stringify(fvRow || {}, null, 2))}</pre>
            <h4>State</h4>
            <pre class=\"mono\">${escapeHtml(JSON.stringify(strategy.state || {}, null, 2))}</pre>
          </div>
        `;
      };

      const renderTable = (headerHtml, bodyHtml) => {
        const columns = (headerHtml.match(/<th>/g) || []).length || 6;
        return `<table><thead><tr>${headerHtml}</tr></thead><tbody>${bodyHtml || `<tr><td colspan=\"${columns}\">No rows</td></tr>`}</tbody></table>`;
      };

      const renderParamsSection = ({params, schema, prefix, title}) => {
        const rows = Object.entries(params || {}).map(([name, value]) => {
          const meta = schema[name] || {};
          const type = meta.type || 'number';
          if (type === 'boolean') {
            return `<tr><td>${escapeHtml(name)}</td><td><label><input type=\"checkbox\" name=\"${escapeHtml(name)}\" data-param=\"${escapeHtml(name)}\" data-type=\"boolean\" ${value ? 'checked' : ''} /></label></td><td>${escapeHtml(meta.type || '')}</td><td>${escapeHtml(meta.description || '-')}</td></tr>`;
          }
          const step = type === 'integer' ? '1' : 'any';
          return `<tr><td>${escapeHtml(name)}</td><td><input type=\"number\" name=\"${escapeHtml(name)}\" data-param=\"${escapeHtml(name)}\" data-type=\"${escapeHtml(type)}\" value=\"${escapeHtml(value)}\" step=\"${escapeHtml(step)}\" /></td><td>${escapeHtml(meta.type || '')}</td><td>${escapeHtml(meta.description || '-')}</td></tr>`;
        }).join('');
        return `
          <div class=\"section card\">
            <h3>${escapeHtml(title || 'Params')}</h3>
            <form id=\"${prefix}-params-form\">
              <table><thead><tr><th>Param</th><th>Value</th><th>Type</th><th>Description</th></tr></thead><tbody>${rows || '<tr><td colspan=\"4\">No params</td></tr>'}</tbody></table>
              <div class=\"toolbar\"><button type=\"submit\">Save params</button></div>
            </form>
            <div id=\"${prefix}-params-status\" class=\"status mono\"></div>
          </div>
        `;
      };

      const postParams = async (event, options) => {
        event.preventDefault();
        const form = event.currentTarget;
        const status = document.getElementById(`${options.prefix}-params-status`);
        const updates = {};
        const errors = [];
        const fields = form.querySelectorAll('input[data-param]');
        for (const field of fields) {
          const name = field.getAttribute('data-param');
          const type = field.getAttribute('data-type') || 'number';
          if (!name) {
            continue;
          }
          if (type === 'boolean') {
            updates[name] = !!field.checked;
            continue;
          }
          const raw = field.value;
          if (raw === '') {
            errors.push(`${name}: required`);
            continue;
          }
          const parsed = type === 'integer' ? Number.parseInt(raw, 10) : Number.parseFloat(raw);
          if (!Number.isFinite(parsed)) {
            errors.push(`${name}: invalid number`);
            continue;
          }
          updates[name] = parsed;
        }
        if (errors.length) {
          status.innerHTML = `<span class=\"err\">invalid params: ${escapeHtml(errors.join(', '))}</span>`;
          return;
        }
        const btn = form.querySelector('button[type=\"submit\"]');
        if (btn) {
          btn.disabled = true;
        }
        status.innerHTML = '<span class=\"warn\">saving...</span>';
        try {
          const response = await fetch(`/api/v1/params?${STRATEGY_QUERY}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ params: updates, source: 'ui' }),
          });
          const payload = await response.json();
          if (!response.ok || payload?.ok === false) {
            status.innerHTML = `<span class=\"err\">save failed: ${escapeHtml(payload?.error || `HTTP ${response.status}`)}</span>`;
          } else {
            status.innerHTML = '<span class=\"ok\">saved</span>';
            if (typeof options.refresh === 'function') {
              await options.refresh();
            }
          }
        } catch (error) {
          status.innerHTML = `<span class=\"err\">save failed: ${escapeHtml(String(error))}</span>`;
        } finally {
          if (btn) {
            btn.disabled = false;
          }
        }
      };

      const renderSignal = async () => {
        const data = await apiGet('/api/v1/signals');
        const strategy = (data?.data?.strategies || [])[0] || {};
        document.getElementById('content').innerHTML = renderSignalPanel(strategy, strategy.fv_row || {}, strategy.legs || {});
      };

      const renderParams = async () => {
        const data = await apiGet('/api/v1/params');
        const row = (data?.data?.strategies || [])[0] || {};
        document.getElementById('content').innerHTML = renderParamsSection({
          params: row.params || {},
          schema: row.schema || {},
          prefix: 'params',
          title: 'Parameters',
        });
        document
          .getElementById('params-params-form')
          .addEventListener('submit', (event) => {
            postParams(event, {
              prefix: 'params',
              refresh: async () => {
                await renderParams();
              },
            });
          });
      };

      const renderHome = async () => {
        const [signalData, paramsData, balancesData, tradesData, alertsData] = await Promise.all([
          apiGet('/api/v1/signals'),
          apiGet('/api/v1/params'),
          apiGet('/api/v1/balances'),
          apiGet('/api/v1/trades'),
          apiGet('/api/v1/alerts'),
        ]);

        const strategy = (signalData?.data?.strategies || [])[0] || {};
        const row = (paramsData?.data?.strategies || [])[0] || {};
        const balances = balancesData?.data?.rows || [];
        const trades = tradesData?.data?.rows || [];
        const alerts = alertsData?.data?.rows || [];

        const balanceRows = balances.map((entry) => `<tr><td>${escapeHtml(entry.strategy_id)}</td><td>${escapeHtml(entry.exchange || entry.venue || '')}</td><td>${escapeHtml(entry.base || entry.coin || '')}</td><td>${escapeHtml(entry.free)}</td><td>${escapeHtml(entry.locked)}</td><td>${escapeHtml(entry.total)}</td></tr>`).join('');
        const tradeRows = trades.map((entry) => `<tr><td>${fmtTs(entry.ts_ms)}</td><td>${escapeHtml(entry.side || entry.order_side)}</td><td>${escapeHtml(entry.qty)}</td><td>${escapeHtml(entry.price)}</td><td>${escapeHtml(entry.symbol || entry.coin)}</td><td>${escapeHtml(entry.venue || entry.exchange)}</td></tr>`).join('');
        const alertRows = alerts.map((entry) => `<tr><td>${fmtTs(entry.ts_ms)}</td><td>${escapeHtml(entry.level)}</td><td>${escapeHtml(entry.message || entry.event || entry.text)}</td><td>${escapeHtml(entry.reason)}</td></tr>`).join('');

        document.getElementById('content').innerHTML = `
          ${renderSignalPanel(strategy, strategy.fv_row || {}, strategy.legs || {})}
          ${renderParamsSection({
            params: row.params || {},
            schema: row.schema || {},
            prefix: 'home',
            title: 'Params',
          })}
          <div class=\"section card\"><h3>Balances</h3>${renderTable('<th>strategy_id</th><th>exchange</th><th>asset</th><th>free</th><th>locked</th><th>total</th>', balanceRows)}</div>
          <div class=\"section card\"><h3>Trades</h3>${renderTable('<th>time</th><th>side</th><th>qty</th><th>price</th><th>symbol</th><th>venue</th>', tradeRows)}</div>
          <div class=\"section card\"><h3>Alerts</h3>${renderTable('<th>time</th><th>level</th><th>message</th><th>reason</th>', alertRows)}</div>
        `;
        document
          .getElementById('home-params-form')
          .addEventListener('submit', (event) => {
            postParams(event, {
              prefix: 'home',
              refresh: async () => {
                await renderHome();
              },
            });
          });
      };

      const renderBalances = async () => {
        const data = await apiGet('/api/v1/balances');
        const rows = data?.data?.rows || [];
        const body = rows.map((row) => `<tr><td>${fmt(row.strategy_id)}</td><td>${fmt(row.exchange || row.venue || '')}</td><td>${fmt(row.base || row.coin || '')}</td><td>${fmt(row.free)}</td><td>${fmt(row.locked)}</td><td>${fmt(row.total)}</td></tr>`).join('');
        document.getElementById('content').innerHTML = `<div class=\"section card\"><h3>Balances</h3>${renderTable('<th>strategy_id</th><th>exchange</th><th>asset</th><th>free</th><th>locked</th><th>total</th>', body)}</div>`;
      };

      const renderTrades = async () => {
        const data = await apiGet('/api/v1/trades');
        const rows = data?.data?.rows || [];
        const body = rows.map((row) => `<tr><td>${fmtTs(row.ts_ms)}</td><td>${fmt(row.side || row.order_side)}</td><td>${fmt(row.qty)}</td><td>${fmt(row.price)}</td><td>${fmt(row.symbol || row.coin)}</td><td>${fmt(row.venue || row.exchange)}</td></tr>`).join('');
        document.getElementById('content').innerHTML = `<div class=\"section card\"><h3>Trades</h3>${renderTable('<th>time</th><th>side</th><th>qty</th><th>price</th><th>symbol</th><th>venue</th>', body)}</div>`;
      };

      const renderAlerts = async () => {
        const data = await apiGet('/api/v1/alerts');
        const rows = data?.data?.rows || [];
        const body = rows.map((row) => `<tr><td>${fmtTs(row.ts_ms)}</td><td>${fmt(row.level)}</td><td>${fmt(row.message || row.event || row.text)}</td><td>${fmt(row.reason)}</td></tr>`).join('');
        document.getElementById('content').innerHTML = `<div class=\"section card\"><h3>Alerts</h3>${renderTable('<th>time</th><th>level</th><th>message</th><th>reason</th>', body)}</div>`;
      };

      const renderMap = {
        '/tokenmm/signal': renderSignal,
        '/tokenmm': renderHome,
        '/': renderHome,
        '/tokenmm/params': renderParams,
        '/tokenmm/balances': renderBalances,
        '/tokenmm/trades': renderTrades,
        '/tokenmm/alerts': renderAlerts,
      };

      const render = async () => {
        const target = (ROUTE === '/tokenmm' || ROUTE === '/') ? '/tokenmm' : ROUTE;
        const fn = renderMap[target] || renderSignal;
        try {
        const health = await (await fetch('/api/v1/healthz')).json();
          const ok = health?.ok ? 'ok' : 'error';
          document.getElementById('status').innerHTML = `${ok} redis=${health?.data?.redis_available ? 'up' : 'down'} strategy=${escapeHtml(STRATEGY)}`;
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
        payload = {"strategies": [_build_params_payload(redis_client, sid)]}
        return jsonify({"ok": True, "data": payload, "error": None}), 200

    @app.post("/api/v1/params")
    def api_params_update() -> Any:
        sid = request.args.get("strategy") or DEFAULT_STRATEGY_ID
        updates = _read_params_request_payload()
        if not updates:
            return jsonify({"ok": False, "data": None, "error": "missing_payload"}), 400
        try:
            result = _apply_params_update(redis_client, sid, updates)
        except ValueError as exc:
            return jsonify({"ok": False, "data": None, "error": str(exc)}), 400
        payload = {
            "strategy_id": sid,
            "updated": result["updated"],
            "params": result["params"],
            "schema": PARAMS_SCHEMA,
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
        payload = {
            "strategy_id": sid,
            "params": _load_params(redis_client, sid),
            "schema": PARAMS_SCHEMA,
        }
        return jsonify({"ok": True, "data": payload, "error": None}), 200

    @app.post("/api/v1/strategies/<string:strategy_id>/parameters")
    @app.patch("/api/v1/strategies/<string:strategy_id>/parameters")
    def api_strategy_parameters_update(strategy_id: str) -> Any:
        sid = strategy_id or DEFAULT_STRATEGY_ID
        updates = _read_params_request_payload()
        if not updates:
            return jsonify({"ok": False, "data": None, "error": "missing_payload"}), 400
        try:
            result = _apply_params_update(redis_client, sid, updates)
        except ValueError as exc:
            return jsonify({"ok": False, "data": None, "error": str(exc)}), 400
        return jsonify(
            {
                "ok": True,
                "data": {
                    "strategy_id": sid,
                    "updated": result["updated"],
                    "params": result["params"],
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
