# Fluxboard TokenMM Minimal Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **For executing agent:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Bring the TokenMM view of Fluxboard into this repo, with production-grade REST + Socket.IO real-time parity for the TokenMM surface (Dashboard, Signals, Params, Balances, Trades, Alerts), explicitly excluding order-view.

**Architecture:**

1. Vendored Vite/React SPA under `fluxboard/`.
2. FluxAPI backend is the production `flux` API app (`nautilus_trader/flux/api/*`) with a small TokenMM compatibility layer.
3. Socket.IO server runs in the same Python service and emits polling-transport events to support real-time tokenmm updates (Signals/Trades/Alerts) with REST as the authoritative fallback.

**Tech Stack:** React 18 + TypeScript + Vite + React Router + Zustand + Tailwind + Socket.IO (JS), Python + Flask (existing FluxAPI) + Redis.

---

## Scope / Non-goals

**In scope**

1. TokenMM pages: Dashboard, Signals, Params, Balances, Trades blotter, Alerts.
2. Socket.IO real-time updates: `signal_delta`, `trade_update`, `market_update`.
3. REST endpoints required by the tokenmm UI (minimal surface, stable envelopes).
4. Production-like serving of `/tokenmm/*` (SPA deep links resolve) and `/socket.io`.

**Out of scope**

1. Order viewer: `/tokenmm/order-view` route, nav entry, API endpoints, and socket streams.
2. Any non-tokenmm profiles, scanners, PnL/tearsheet UI, FX/hedger pages.
3. Strategy/engine refactors (handled elsewhere).

---

## Execution dependencies (explicit)

1. This work runs in its own worktree/branch to avoid collisions with the ongoing strategy refactor work.
2. Backend contract decision is locked:
   - `legs` is a map keyed by stable `contract_id`.
3. FluxAPI bugfix dependency:
   - API market leg lookups must be keyed by `contract_id` (not `exchange`) to avoid symbol collisions.
4. Real-time behavior:
   - Socket.IO is required and must work in polling mode by default.

---

## Non-negotiable acceptance criteria

1. `/tokenmm` and nested routes render (deep links work) and exclude `/tokenmm/order-view` from router + nav.
2. TokenMM UI loads fully using REST only (socket optional at startup) with stable envelope shapes.
3. Socket.IO connects at `/socket.io` (polling transport) and real-time updates work for:
   - Trades append/update (`trade_update`)
   - Signal patch updates (`signal_delta`)
   - Alerts refresh signal (`market_update` includes alerts or triggers polling)
4. Alerts are implemented end-to-end:
   - `GET /api/v1/alerts` and `DELETE /api/v1/alerts`
   - alerts visible in TokenMM nav/surface
5. `pnpm build` for the SPA succeeds.
6. Targeted python unit tests for the TokenMM API compatibility layer exist and pass.

---

## Contract decisions (future-proofing)

### 1) `contract_id` and `legs` shape

- Standardize on `contract_id` as the unique identity for a leg in both REST and Socket payloads.
- `legs` is a map keyed by `contract_id`.
- Each leg row MUST also include `contract_id` (redundant but explicit).
- Provide an optional `legs_order: string[]` if the UI requires deterministic ordering independent of map ordering.

**Recommended `contract_id` format**

- `"{exchange}:{symbol}"` (or `"{exchange}:{instrument_id}"` if symbol ambiguity exists).
- The same format must be used everywhere: REST payloads, Socket deltas, and Redis key-building logic.

### 2) Envelope contract

All HTTP responses must use the FluxAPI envelope shape (already used in `nautilus_trader/flux/api/*`):

- `ok: bool`
- `api_version: string`
- `request_id: string`
- `timestamp_ms: int`
- `data: object | null`
- `error: {code: string, message: string, details?: object} | null`

### 3) REST is authoritative

- Socket.IO must never be required for initial render.
- Socket events may be lossy; clients must be able to recover via polling.

---

## Required HTTP endpoints (TokenMM)

Implement these in the production FluxAPI app (not the deprecated `examples/live/poc/*` shim):

- `GET /api/v1/signals?profile=tokenmm`
  - Must include `strategies` and `server_ts_ms` at minimum.
  - Strategy legs must use `legs[contract_id]`.
- `GET /api/v1/params?profile=tokenmm`
- `PATCH /api/v1/params` (bulk updates)
- `GET /api/v1/param-schema?profile=tokenmm`
- `GET /api/v1/balances`
- `GET /api/v1/trades`
- `GET /api/v1/trades/delta`
- `GET /api/v1/alerts`
- `DELETE /api/v1/alerts`

Optional compatibility endpoints (only if Fluxboard needs them in TokenMM):

- `GET /api/v1/strategies/<strategy_id>/parameters`
- `PATCH /api/v1/strategies/<strategy_id>/parameters`
- `GET /api/v1/strategies/<strategy_id>/config-files`

---

## Required Socket.IO events (TokenMM)

### Connection and room model

1. Client connects to `/socket.io` using polling transport.
2. Client provides `profile` in connection query.
3. Client emits `set_profile` with `{profile: "tokenmm"}`.
4. Server joins `profile:<normalized_profile>` room.

**Profile normalization**

- Normalize `tokenm` and `tokenmm` to `tokenmm`.

### Event list

1. `market_update`
   - Purpose: lightweight heartbeat, and trigger UI refresh (alerts/strategies) when changed.
   - Minimal payload: `{server_ts_ms, server_time, alerts?, strategies?}`
2. `signal_delta`
   - Purpose: patch update one strategy (or one leg) without full reload.
   - Must support patch semantics:
     - missing field => no change
     - explicit `null` => delete
     - leg updates addressed by `contract_id` key
3. `trade_update`
   - Purpose: append/upsert/delete trades in the blotter.
   - Must include: `{op, row_id, version, seq, trade}`.

---

## Plan structure

Track A: Frontend vendoring + route surface
Track B: Backend REST compatibility (FluxAPI)
Track C: Socket.IO realtime parity
Track D: Serving + runbook + validation

---

## Task list (execution order)

### Task 1: Contract freeze (TokenMM)

**Files**

- Create: `docs/fluxboard/tokenmm_contract.md`
- Create: `docs/fluxboard/tokenmm_socket_contract.md`

**Steps**

1. Document the TokenMM required routes and explicitly exclude order-view.
2. Document each HTTP endpoint with request/response examples.
3. Document each Socket.IO event with payload examples and reconnect semantics.
4. Include the `contract_id`/`legs` decision and required fields.

**Acceptance**

- [ ] Docs are complete enough to implement without reading upstream code.
- [ ] `legs` map is specified as keyed by `contract_id`.

### Task 2: Vendor Fluxboard UI (TokenMM)

**Files**

- Create: `fluxboard/`
- Modify: `.gitignore`

**Steps**

1. Copy the Fluxboard source tree into `fluxboard/` (source path provided at execution time).
2. Ensure ignores exist for `fluxboard/node_modules/`, `fluxboard/dist/`, caches.
3. Ensure package manager and scripts are documented and runnable.

**Acceptance**

- [ ] `pnpm install` succeeds.
- [ ] `pnpm build` succeeds before behavior changes.

### Task 3: Prune TokenMM surface (include Alerts, exclude Order View)

**Files (expected, adjust after vendoring)**

- Modify: `fluxboard/config/uiProfiles.ts`
- Modify: `fluxboard/main.tsx`
- Modify: `fluxboard/Nav.tsx`
- Modify: route/nav tests under `fluxboard/`

**Steps**

1. Ensure TokenMM nav/route surface includes Alerts.
2. Remove order-view route from TokenMM nav and router.
3. Preserve `/tokenm` alias redirect to `/tokenmm`.
4. Add/adjust tests to assert Alerts exists and order-view does not.

**Acceptance**

- [ ] No reachable `/tokenmm/order-view` route.
- [ ] Alerts page is reachable and in navigation.

### Task 4: Update Fluxboard client model for `contract_id` keyed legs

**Files (expected, adjust after vendoring)**

- Modify: Signal store/selectors and any code assuming `legs` keyed by exchange.

**Steps**

1. Update UI data model assumptions:
   - `legs` is keyed by `contract_id`.
2. Ensure stable ordering:
   - use `legs_order` if present, otherwise sort by `contract_id`.
3. Update any derived views that relied on `exchange` as the map key.

**Acceptance**

- [ ] Signals page renders correctly with two contracts on the same exchange.

### Task 5: Implement TokenMM REST compatibility in production FluxAPI

**Files**

- Modify: `nautilus_trader/flux/api/app.py`
- Modify: `nautilus_trader/flux/api/payloads.py`
- Modify: `docs/flux/api.md` (contract updates)
- Create: `tests/unit_tests/flux/api/test_tokenmm_compat.py`

**Steps**

1. Fix market rows lookup/keying bug:
   - key per leg by `contract_id`, not just by `exchange`.
2. Implement required TokenMM endpoints listed above (only minimal fields needed by UI).
3. Ensure empty datasets return stable envelope shapes.
4. Sanitize error responses (do not leak raw exception strings to clients).
5. Add targeted unit tests:
   - multiple contracts per exchange
   - alerts empty/non-empty
   - params schema + params load/update
   - trades pagination/delta shape

**Acceptance**

- [ ] `pytest -q tests/unit_tests/flux/api/test_tokenmm_compat.py` passes.

### Task 6: Add Socket.IO server to FluxAPI

**Files**

- Create: `nautilus_trader/flux/api/socketio.py` (or similar)
- Modify: `nautilus_trader/flux/api/app.py` (wire Socket.IO into app factory)
- Create: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- Modify: `docs/flux/api.md` (socket contract)

**Steps**

1. Implement Socket.IO server at `/socket.io` with polling transport by default.
2. Implement `set_profile` room join/leave and normalization.
3. Implement an emitter loop which publishes:
   - `market_update` (1–2 Hz) with alerts/strategies when changed
   - `signal_delta` patches
   - `trade_update` deltas
4. Ensure REST fallback is authoritative and clients can reconnect safely.
5. Add tests for:
   - room behavior
   - minimum payload shapes
   - patch semantics (null delete, missing no-change)

**Acceptance**

- [ ] Fluxboard receives real-time updates without page reload.

### Task 7: Serving and runbook

**Files**

- Modify: `fluxboard/vite.config.ts`
- Modify/Create: `fluxboard/.env.example`
- Modify: `examples/live/makerv3_single_leg/run_api.py`
- Modify: `examples/live/makerv3_single_leg/README.md`
- Create: `docs/fluxboard/tokenmm_runbook.md`

**Steps**

1. Option A (dev): Vite dev server proxies to FluxAPI for `/api/*` and `/socket.io`.
2. Option B (prod-like): FluxAPI serves `fluxboard/dist` at `/tokenmm/*` with SPA fallback.
3. Document required env vars, ports, and security notes (localhost default).

**Acceptance**

- [ ] Deep links under `/tokenmm/*` return SPA HTML in prod-like mode.
- [ ] Socket.IO works in both Option A and Option B.

### Task 8: Final validation

**Files**

- Modify: `docs/plans/2026-03-04-fluxboard-tokenmm-minimal-migration.md`

**Steps**

1. Add final smoke checklist (manual + scripted).
2. Record known limitations and future migration notes.

**Acceptance**

- [x] Plan contains a clear go/no-go checklist and handoff notes.

---

## Status tracker (execution owner fills this)

| Task | Owner | Status | Started | Completed | Evidence |
| --- | --- | --- | --- | --- | --- |
| 1 | Codex | Done | 2026-03-04 03:36 UTC | 2026-03-04 03:47 UTC | `docs/fluxboard/tokenmm_contract.md`, `docs/fluxboard/tokenmm_socket_contract.md`, subagent spec ✅ + quality ✅ |
| 2 | Codex | Done | 2026-03-04 03:49 UTC | 2026-03-04 04:08 UTC | Vendored `fluxboard/` from chainsaw, `.gitignore` updates, `pnpm install --frozen-lockfile` + `pnpm build` pass, spec ✅ + quality ✅ |
| 3 | Codex | Done | 2026-03-04 04:09 UTC | 2026-03-04 04:21 UTC | TokenMM nav/route pruned (order-view removed, alerts retained), `/tokenm` alias preserved, route/nav tests updated, spec ✅ + quality ✅ |
| 4 | Codex | Done | 2026-03-04 04:22 UTC | 2026-03-04 04:49 UTC | Contract_id leg-map adapter (`legs_order` + sorted fallback), generic leg delta merge/delete semantics, same-exchange coverage tests, spec ✅ + quality ✅ |
| 5 | Codex | Done | 2026-03-04 04:50 UTC | 2026-03-04 05:23 UTC | FluxAPI TokenMM compat endpoints + tests (`test_tokenmm_compat.py`), contract_id legs + bulk params + trades/delta + alerts DELETE, sanitized errors, spec ✅ + quality ✅ |
| 6 | Codex | Done | 2026-03-04 05:24 UTC | 2026-03-04 06:04 UTC | Added Socket.IO server wiring + bounded emitter cursors + stable signal patch diffing + unsupported-profile rejection, tests in `test_socketio_tokenmm.py`, `pytest` pass, spec ✅ + quality ✅ |
| 7 | Codex | Done | 2026-03-04 06:05 UTC | 2026-03-04 06:30 UTC | Completed serving + runbook validation gates: Option A proxy and Option B prod-like deep-link checks, Socket.IO polling handshake checks, and review gates (spec ✅ + quality ✅). |
| 8 | Codex | Done | 2026-03-04 06:31 UTC | 2026-03-04 06:37 UTC | Added final go/no-go/handoff gate, then resolved quality findings (reversible `PATCH /api/v1/params` smoke flow, assertive Task 7 serving assertions, unambiguous decision field, tracker/log consistency). |

**Status values:** `Not started`, `In progress`, `Blocked`, `Done`, `Needs follow-up`

**Progress log format:**

- `- [YYYY-MM-DD hh:mm UTC] Task {n}: {change} / {evidence} / {next}`
- [2026-03-04 03:36 UTC] Task 1: Started contract freeze docs with fresh implementer subagent / spawned implementer, spec reviewer, code-quality reviewer / iterate until both reviewers approve.
- [2026-03-04 03:47 UTC] Task 1: Created TokenMM HTTP + Socket contract docs and resolved reviewer findings (profile scoping, seq semantics, trade cursor versioning) / reviewer verdicts: spec ✅, quality ✅ / proceed to Task 2 vendoring.
- [2026-03-04 03:49 UTC] Task 2: Started vendoring fluxboard with fresh implementer subagent / rsync from `/home/ubuntu/chainsaw/fluxboard`, baseline `pnpm install` + `pnpm build` / run spec + quality gates.
- [2026-03-04 04:08 UTC] Task 2: Vendored `fluxboard/`, added ignore rules, fixed README/runability issues and deploy.sh tracking / evidence: install/build pass and reviewer verdicts spec ✅ quality ✅ / proceed to Task 3 route/nav pruning.
- [2026-03-04 04:09 UTC] Task 3: Started TokenMM surface pruning with fresh implementer subagent / test-first updates in `uiProfiles/main/Nav` tests and route config changes / run spec + quality gates.
- [2026-03-04 04:21 UTC] Task 3: Removed order-view from TokenMM route/nav surface, preserved alerts and `/tokenm` alias redirect, and added top-level route/nav active-state regression tests / reviewer verdicts spec ✅ quality ✅ / proceed to Task 4 contract_id leg model updates.
- [2026-03-04 04:22 UTC] Task 4: Started contract_id keyed leg-model migration with fresh implementer subagent / updated types, signal leg helpers, store merge semantics, signal delta patching, and same-exchange rendering tests / run spec + quality gates.
- [2026-03-04 04:49 UTC] Task 4: Finalized `legs` map handling keyed by contract_id with stable ordering (`legs_order` then sorted key fallback), fixed explicit-null delete semantics, and added default-path regression tests / reviewer verdicts spec ✅ quality ✅ / proceed to Task 5 FluxAPI REST compatibility.
- [2026-03-04 04:50 UTC] Task 5: Started FluxAPI TokenMM REST compatibility with fresh implementer subagent / implemented endpoint shape fixes, contract_id leg keying, error sanitization, and new `test_tokenmm_compat.py` / run spec + quality gates.
- [2026-03-04 05:23 UTC] Task 5: Completed REST compatibility loop including delta gap safety boundary fix and expanded edge-case tests (bulk strategy_id validation, pagination totals, reset_required behavior) / reviewer verdicts spec ✅ quality ✅ / proceed to Task 6 Socket.IO server.
- [2026-03-04 05:24 UTC] Task 6: Started Socket.IO server implementation with fresh implementer subagent / added `/socket.io` server, set_profile handlers, emitter loop, and socket tests / run spec + quality gates.
- [2026-03-04 06:04 UTC] Task 6: Completed Socket.IO quality fix loop (stable delta suppression, bounded reads, profile-aware strategy mapping, unsupported-profile rejection, clear-room ack) and docs sync / evidence: `PYTHONPATH=/tmp PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py --confcutdir=tests/unit_tests/flux/api` (12 passed), `... test_app.py ...` (7 passed), reviewer verdicts spec ✅ quality ✅ / proceed to Task 7 serving + runbook.
- [2026-03-04 06:05 UTC] Task 7: Started serving/runbook validation batch / verified Vite proxy + FluxAPI static serve wiring and prepared runbook command gates / run spec + quality review gates.
- [2026-03-04 06:30 UTC] Task 7: Completed serving/runbook gates / evidence: Option A (`/tokenmm` + `/socket.io` polling) and Option B (`/tokenmm`, `/tokenmm/alerts`, `/tokenmm/order-view`, `/socket.io`) verification checks, reviewer verdicts spec ✅ quality ✅ / proceed to Task 8 final validation gate.
- [2026-03-04 06:31 UTC] Task 8: Added final validation gate with scripted/manual smoke checklist + handoff criteria, and documented current migration limitations + next-step notes / evidence: this plan section update / next: run checklist in target environment before merge/deploy decision.
- [2026-03-04 06:37 UTC] Task 8: Resolved quality-gate findings / added reversible `PATCH /api/v1/params` smoke flow with GET assertions, converted Task 7 serving checks to assertive status/body + Engine.IO-open checks, replaced dual decision checkboxes with single decision field + required NO-GO metadata / next: execute the finalized checklist in target runtime and record GO/NO-GO.
- [2026-03-04 07:12 UTC] Post-plan review remediation: fixed alerts DELETE response compatibility (`success` + metadata), aligned TokenMM contract/runbook wording, updated frontend profile query propagation + clear-alerts client parsing, socket same-origin default, and explicit `/tokenmm/dashboard` route coverage / evidence: `pytest -q tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_app.py --confcutdir=tests/unit_tests/flux/api` (30 passed), `pnpm --dir fluxboard exec vitest run __tests__/api.test.ts Alerts.test.tsx sockets.test.ts main.routes.test.tsx config/uiProfiles.test.ts` (48 passed) / next: sync with mono-pr strategy branch and rerun compatibility checks.
- [2026-03-04 07:17 UTC] Mono-PR sync: rebased `poc/fluxboard-tokenmm-migration` onto local `poc/makerv3-singleleg-mono-pr`, resolved conflicts in `docs/flux/api.md`, `examples/live/makerv3_single_leg/{README.md,run_api.py}`, `nautilus_trader/flux/api/{app.py,payloads.py}`, and `tests/unit_tests/flux/api/test_app.py` while preserving strategy-branch host-default expectations and TokenMM contract changes / evidence: successful rebase and preserved `_resolve_bind_host` helper compatibility / next: rerun strategy-side targeted tests.
- [2026-03-04 07:20 UTC] Rebased compatibility verification: strategy/API guard tests passed after conflict resolution / evidence: `pytest -q tests/unit_tests/examples/strategies/test_makerv3_single_leg_run_api.py tests/unit_tests/examples/strategies/test_makerv3_single_leg_run_bridge.py --confcutdir=tests/unit_tests/examples/strategies` (9 passed), `pytest -q tests/unit_tests/scripts/test_check_flux_leakage.py --confcutdir=tests/unit_tests/scripts` (4 passed) / note: `tests/unit_tests/analysis/test_tearsheet.py` requires compiled Nautilus modules unavailable in this `/tmp` PYTHONPATH test harness.

---

## Task 8 final validation gate (go/no-go + handoff)

### Scripted smoke checklist (must all pass)

Run from repo root. Assume FluxAPI is reachable at `127.0.0.1:5022`.

```bash
API_BASE="${API_BASE:-http://127.0.0.1:5022}"
PROFILE="${PROFILE:-tokenmm}"
SMOKE_STRATEGY="${SMOKE_STRATEGY:-strategy_01}"  # Use a disposable strategy for DELETE checks.

# 1) FluxAPI health/readiness
curl -fsS "$API_BASE/api/v1/healthz" \
  | python -c 'import json,sys; b=json.load(sys.stdin); assert b["ok"] and b["data"]["redis_available"] and "required_keys" in b["data"]'
curl -fsS "$API_BASE/api/v1/readyz" \
  | python -c 'import json,sys; b=json.load(sys.stdin); assert b["ok"] and b["data"]["schema_ready"] is True'

# 2) Key TokenMM REST endpoints
for path in \
  "/api/v1/signals?profile=${PROFILE}" \
  "/api/v1/param-schema?profile=${PROFILE}" \
  "/api/v1/params?profile=${PROFILE}" \
  "/api/v1/balances?profile=${PROFILE}&limit=5" \
  "/api/v1/trades?profile=${PROFILE}&limit=5" \
  "/api/v1/trades/delta?profile=${PROFILE}&since_seq=0&limit=5" \
  "/api/v1/alerts?profile=${PROFILE}&limit=5"
do
  curl -fsS "${API_BASE}${path}" \
    | python -c 'import json,sys; b=json.load(sys.stdin); assert b["ok"] is True and "data" in b'
done
curl -fsS -X DELETE "${API_BASE}/api/v1/alerts?profile=${PROFILE}&strategy=${SMOKE_STRATEGY}" \
  | python -c 'import json,sys; b=json.load(sys.stdin); assert b["ok"] and "deleted" in b["data"] and "remaining" in b["data"]'

# 2b) PATCH /api/v1/params reversible smoke flow (legacy PATCH + bulk PATCH restore)
PARAMS_BEFORE="$(curl -fsS "${API_BASE}/api/v1/params?profile=${PROFILE}&strategy=${SMOKE_STRATEGY}")"
export PARAMS_BEFORE
PARAM_KEY="$(python - <<'PY'
import json
import os

body = json.loads(os.environ["PARAMS_BEFORE"])
assert body.get("ok") is True and body.get("data"), "params GET failed"
params = body["data"][0].get("params", {})
for key, value in params.items():
    if isinstance(value, (bool, int, float, str)):
        print(key)
        break
else:
    raise SystemExit("No reversible scalar param found for smoke PATCH.")
PY
)"
export PARAM_KEY
ORIG_JSON_VALUE="$(python - <<'PY'
import json
import os

body = json.loads(os.environ["PARAMS_BEFORE"])
print(json.dumps(body["data"][0]["params"][os.environ["PARAM_KEY"]]))
PY
)"
export ORIG_JSON_VALUE
NEW_JSON_VALUE="$(python - <<'PY'
import json
import os

body = json.loads(os.environ["PARAMS_BEFORE"])
value = body["data"][0]["params"][os.environ["PARAM_KEY"]]
if isinstance(value, bool):
    updated = not value
elif isinstance(value, int):
    updated = value + 1
elif isinstance(value, float):
    updated = round(value + 0.01, 8)
elif isinstance(value, str):
    updated = value + "_smoke"
else:
    raise SystemExit("Unsupported smoke value type.")
print(json.dumps(updated))
PY
)"
export NEW_JSON_VALUE

python - <<'PY' > /tmp/tokenmm_patch_legacy.json
import json
import os

payload = {
    "params": {os.environ["PARAM_KEY"]: json.loads(os.environ["NEW_JSON_VALUE"])},
}
print(json.dumps(payload))
PY
curl -fsS -X PATCH "${API_BASE}/api/v1/params?profile=${PROFILE}&strategy=${SMOKE_STRATEGY}" \
  -H 'Content-Type: application/json' \
  --data-binary @/tmp/tokenmm_patch_legacy.json \
  | python -c 'import json,sys; b=json.load(sys.stdin); assert b["ok"] and b["data"]["failed"] == [] and b["data"]["errors"] == []'
curl -fsS "${API_BASE}/api/v1/params?profile=${PROFILE}&strategy=${SMOKE_STRATEGY}" \
  | python -c 'import json,os,sys; b=json.load(sys.stdin); assert b["ok"]; observed=b["data"][0]["params"][os.environ["PARAM_KEY"]]; expected=json.loads(os.environ["NEW_JSON_VALUE"]); assert observed == expected'

python - <<'PY' > /tmp/tokenmm_patch_bulk_restore.json
import json
import os

payload = {
    "updates": [
        {
            "strategy_id": os.environ["SMOKE_STRATEGY"],
            "params": {os.environ["PARAM_KEY"]: json.loads(os.environ["ORIG_JSON_VALUE"])},
        },
    ],
}
print(json.dumps(payload))
PY
curl -fsS -X PATCH "${API_BASE}/api/v1/params?profile=${PROFILE}" \
  -H 'Content-Type: application/json' \
  --data-binary @/tmp/tokenmm_patch_bulk_restore.json \
  | python -c 'import json,sys; b=json.load(sys.stdin); assert b["ok"] and b["data"]["failed"] == [] and b["data"]["errors"] == []'
curl -fsS "${API_BASE}/api/v1/params?profile=${PROFILE}&strategy=${SMOKE_STRATEGY}" \
  | python -c 'import json,os,sys; b=json.load(sys.stdin); assert b["ok"]; observed=b["data"][0]["params"][os.environ["PARAM_KEY"]]; expected=json.loads(os.environ["ORIG_JSON_VALUE"]); assert observed == expected'

# 3) Socket.IO polling handshake (Task 6 critical path)
curl -fsS "${API_BASE}/socket.io/?EIO=4&transport=polling&t=$(date +%s%N)" \
  | python -c 'import json,sys; raw=sys.stdin.read(); assert raw.startswith("0{"); p=json.loads(raw[1:]); assert p.get("sid") and "pingInterval" in p and "pingTimeout" in p'

# 4) Frontend build (Task 7 prerequisite)
pnpm --dir fluxboard build

# 5) Task 6 targeted checks
PYTHONPATH=/tmp PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 \
  pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py --confcutdir=tests/unit_tests/flux/api
PYTHONPATH=/tmp PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 \
  pytest -q tests/unit_tests/flux/api/test_app.py --confcutdir=tests/unit_tests/flux/api -k 'healthz or readyz'
```

Task 7 targeted serving checks (run while API process is up in each mode):

```bash
assert_http_200_html() {
  local url="$1"
  local body_file
  body_file="$(mktemp)"
  local code
  code="$(curl -sS -o "$body_file" -w '%{http_code}' "$url")"
  test "$code" = "200"
  grep -Eqi '<!doctype html|<html' "$body_file"
  grep -q '<div id="root"' "$body_file"
  rm -f "$body_file"
}

assert_engineio_open() {
  local url="$1"
  curl -fsS "$url" \
    | python -c 'import json,sys; raw=sys.stdin.read(); assert raw.startswith("0{"); p=json.loads(raw[1:]); assert p.get("sid") and "pingInterval" in p and "pingTimeout" in p'
}

# Option A (dev proxy): API on :5022 + `pnpm --dir fluxboard dev` on :5173
assert_http_200_html "http://127.0.0.1:5173/tokenmm"
assert_engineio_open "http://127.0.0.1:5173/socket.io/?EIO=4&transport=polling&t=$(date +%s%N)"

# Option B (prod-like): `run_api.py --serve-fluxboard --host 127.0.0.1 --port 5022`
assert_http_200_html "http://127.0.0.1:5022/tokenmm"
assert_http_200_html "http://127.0.0.1:5022/tokenmm/alerts"
assert_http_200_html "http://127.0.0.1:5022/tokenmm/order-view"
assert_engineio_open "http://127.0.0.1:5022/socket.io/?EIO=4&transport=polling&t=$(date +%s%N)"
```

### Manual smoke checklist (operator/UAT)

- [ ] `/tokenmm` renders without console/runtime errors and data appears from REST on cold load.
- [ ] `/tokenmm/alerts` renders and alert actions reflect API state updates.
- [ ] TokenMM nav does not expose `order-view`; direct `/tokenmm/order-view` does not show order-view UI.
- [ ] Browser network panel confirms Socket.IO long-polling requests on `/socket.io/?EIO=4&transport=polling`.
- [ ] Live update behavior verified: at least one `trade_update`, `signal_delta`, and `market_update` event is observed and reflected in UI, with REST fallback still usable after disconnect/reconnect.
- [ ] Deep-link reload checks pass for `/tokenmm`, `/tokenmm/alerts`, and at least one strategy-specific page.

### Go/no-go decision checklist

- [ ] All scripted checks above exit `0`.
- [ ] Manual smoke checklist is complete with no Sev-1/Sev-2 defects.
- [ ] Known limitations below are accepted for this migration cut.
- [ ] Handoff bundle is attached (command logs, screenshots, environment details).
- Decision (single-select): `GO | NO-GO`.
- If decision is `NO-GO`, required metadata: `blocker_ids=<...>; owner=<...>; follow_up_date=YYYY-MM-DD`.

### Handoff notes (required)

1. Attach exact command output logs for the scripted checklist.
2. Record runtime context: git commit SHA, Redis/API/frontend ports, serving mode (Option A or B), and timestamp.
3. If `NO-GO`, include first failing command, response envelope (`code`, `message`, `request_id`), and a rollback/containment note.
4. Include quick links for operators:
   - `docs/fluxboard/tokenmm_runbook.md`
   - `docs/fluxboard/tokenmm_contract.md`
   - `docs/fluxboard/tokenmm_socket_contract.md`

### Known limitations and next-step migration notes

Current known limitations/gaps:

1. Socket transport is intentionally polling-only (`allow_upgrades=False`), which is safer for parity but less efficient than websocket upgrades at high event volume.
2. `/tokenmm/order-view` deep links return SPA HTML in prod-like mode due `/tokenmm/*` fallback; route/nav removal is enforced in frontend, not at HTTP routing layer.
3. REST `profile=tokenmm` is currently compatibility-oriented; effective data scope remains strategy-driven (default strategy unless explicit `strategy` query param is supplied).
4. Validation is command-driven today; no single CI job yet enforces this full end-to-end smoke gate automatically.
5. Security posture remains localhost-first for this migration slice (no auth/TLS hardening in scope here).

Recommended next migration steps:

1. Add a single `scripts/fluxboard/tokenmm_smoke.sh` (or CI job) that runs the scripted checklist and publishes artifacts.
2. Add explicit REST profile allowlisting/mapping tests for multi-strategy TokenMM deployments.
3. Add optional websocket-upgrade mode behind config with perf/compat benchmarks before enabling by default.
4. Add browser E2E checks that assert order-view exclusion behavior and reconnect recovery flows.
5. Define pre-production auth/network controls before exposing FluxAPI beyond localhost.

---

## Open questions (resolve before Task 5)

1. Exact `contract_id` string format: `exchange:symbol` vs `exchange:instrument_id`.
2. Whether FluxAPI should require strategy scoping for tokenmm views (profile->strategy allowlist) or return everything and let UI filter.
3. Whether Socket.IO emitter should read from Redis Streams directly or rely on API polling + diffing.
