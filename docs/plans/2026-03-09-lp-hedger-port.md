# LP Hedger Port Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Port the chainsaw LP hedger into this monorepo as a first-class `/lp` operator surface, preserving the chainsaw runtime/config key contracts while cleaning up the structural debt that would make future ports messy.

**Architecture:** Do not force LP hedgers into `flux.strategies/*`. They are operational services with their own config, Redis contracts, and control-plane endpoints, not Flux strategy families. Create a new repo-level system at `systems/lp/` for hedger runtime and API code, then let the existing public Flux host proxy `/lp` and `/api/v1/hedgers/*` to a hidden `lp-api` backend the same way it already proxies `/equities`. Split “UI/profile identity” from “strategy-set identity” so `/lp` can be a stable Fluxboard profile without pretending to be a TokenMM allowlist.

**Tech Stack:** Python 3.12+, Flask, Redis, ccxt Bybit client, existing Fluxboard React/TypeScript app, systemd/Pulse, TOML/INI configs, pytest, Vitest.

## Scope

**In scope**

1. Port the chainsaw LP hedger core, registry-backed API surface, config model, service runners, and deploy assets.
2. Add a dedicated Fluxboard profile at `/lp` and move the hedger UI there as its stable home.
3. Keep the chainsaw Redis/state/payload/config key contracts so the same operational setups still work.
4. Clean up known design bugs while porting: misleading target semantics, hard-coded job IDs, ETH/PLUME-only naming leakage, and inconsistent env handling.
5. Generalize public profile proxying so future non-strategy ports do not require another equities-style one-off.

**Out of scope**

1. No rewrite into Rust or Nautilus-native execution primitives in this port.
2. No attempt to collapse LP hedgers into MakerV3/MakerV4 runtime params.
3. No speculative generic “all services” framework beyond seams already duplicated by equities and LP.
4. No redesign of the Fluxboard Hedger page beyond what is needed for `/lp` and token0/token1 cleanup.

## Design Constraints

1. Preserve chainsaw contracts that operators depend on:
   - Same hedger IDs.
   - Same Pulse job IDs.
   - Same Redis key families: `<state_key>:state|snapshot|events|mode|geometry_overrides|threshold_overrides`.
   - Same JSON payload field names, including compatibility aliases.
   - Same Bybit/system-config section names and secret field names.
2. Checked-in configs in this repo must not carry live credentials; preserve key names, not secret values.
3. `/lp` must become the canonical user-facing route. The legacy default-surface `/hedger` route should be retired rather than duplicated.
4. The public host remains the Flux API process already serving Fluxboard and Pulse; LP runs as a hidden backend behind it.
5. Use token0/token1 naming as the internal model, and emit ETH/PLUME aliases only for backward compatibility.

## Acceptance Criteria

1. `systems/lp/` exists as a standalone repo-level system with importable runtime/API modules and tests.
2. Public Fluxboard exposes a stable `/lp` profile and no longer treats the hedger as part of the default surface.
3. Public Flux API proxies `/lp` UI paths and `/api/v1/hedgers/*` to a hidden LP backend via `LP_API_BACKEND_URL`.
4. The LP backend preserves chainsaw hedger discovery/status/control/config payloads and Redis keys.
5. The port fixes the known cleanup items:
   - `[target]` values are either respected or removed from the public contract; they are not silently ignored.
   - `job_id` is no longer hard-coded outside config/registry data.
   - `token0`/`token1` is the primary naming model.
   - Band2 no longer deletes `REDIS_URL`.
   - Unused or misleading config knobs are either implemented or removed with explicit compatibility handling.
6. Deploy assets exist under `deploy/lp/` with docs, env examples, install scripts, and contract tests.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | LP hedger port is complete. Final closeout verification is green: targeted Python suites (`67 passed`), targeted Fluxboard LP/profile tests (`55 passed`), and PTY smoke on alternate ports confirmed `/lp` plus `/api/v1/hedgers/*` through the shared public host. |
| Task 1: Generalize Public Surface Proxying And Create LP System Skeleton | completed | main | Quality review found LP-vs-equities precedence risk for `/api/v1/hedgers/*?profile=equities`; fixed by making explicit base paths win before profile-scoped API prefix matches. Verified `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py` `25 passed` and targeted `ruff` on touched files. |
| Task 2: Add Stable Fluxboard `/lp` Profile And Retire Default `/hedger` | completed | main | Added stable `lp` profile routing, moved default hedger ownership to `/lp`, and made `/lp` land on Hedger while keeping `/lp/hedger` active. Verified `pnpm --dir fluxboard exec vitest run config/uiProfiles.test.ts main.routes.test.tsx App.test.tsx Nav.test.tsx` (`36 passed`). |
| Task 3: Port Chainsaw Config And Registry Contracts Into `systems/lp` | completed | main | Added canonical token0/token1 config model with legacy ETH/PLUME aliases plus default hedger registry metadata and root `lp` entrypoint wiring. Verified `uv run --group test pytest -q tests/unit_tests/lp/test_config.py tests/unit_tests/lp/test_registry.py` (`5 passed`) and targeted `ruff` on new files. |
| Task 4: Port Hedger Core Runtime And Fix Contract-Debt Bugs | completed | main | Completed after spec + quality review: post-hedge Redis `:state` writes are best-effort, target semantics remain config-driven, `tests/unit_tests/lp/test_core.py` is green (`12 passed`), and targeted Ruff on LP Python files is clean. |
| Task 5: Port Registry-Backed LP API Endpoints | completed | main | Completed after spec + quality review: Hedger config editing/saving is generic across hedger IDs, ETH configs are editable, target/bybit payloads are preserved, and `pnpm --dir fluxboard exec vitest run Hedger.test.tsx` passes (`19 passed`). |
| Task 6: Port Service Runners And Credential/Env Resolution | completed | main | Completed after spec + quality review: hidden LP API bind defaults, chainsaw-compatible Bybit credential precedence, preserved `REDIS_URL`, and `lp.runners` package wiring all verify via runner tests (`6 passed`) and targeted Ruff clean. |
| Task 7: Add Deploy, Pulse, And Documentation Surfaces Under `deploy/lp` | completed | main | Completed with smoke-driven cleanup: `lp_stack.sh` now preserves explicit `LP_*` env overrides over `lp_stack.env`, deploy assets/docs remain under `deploy/lp/`, active checked-in configs stay Band1/Band2 only, and targeted contract tests are green. |
| Task 8: Verify End-To-End Contracts And Close Out Residual Risk | completed | main | Completed after fixing the remaining runtime gaps: hidden LP API injects real Redis plus Pulse-backed status/control wiring, `/lp` stays on the public Fluxboard host while `/api/v1/hedgers/*` proxies to `lp-api`, targeted Python suites passed (`67 passed`), targeted Fluxboard LP/profile suites passed (`55 passed`), and PTY smoke on alternate ports verified `/lp`, `/api/v1/hedgers/instances`, and `/api/v1/hedgers/eth_plume_lp` all return `200`. |

---

## Contract Decisions To Hold Constant

1. Hidden LP backend listens on `127.0.0.1:5025` by default and is referenced from the public host as `LP_API_BACKEND_URL=http://127.0.0.1:5025`.
2. Active checked-in instances for v1 should be the production-backed ETH/PLUME Band1 and Band2 configs; the extra hedgers should be ported as disabled or template-only until their pool geometry and credentials are validated.
3. The public LP surface is route-based, not profile-query-based:
   - UI base path: `/lp`
   - API path family: `/api/v1/hedgers/*`
   - No Socket.IO contract is required for LP in v1 because the current Hedger page is polling-based.
4. The public Fluxboard `/lp` home route should render the Hedger page directly, not the generic dashboard.

## Task 1: Generalize Public Surface Proxying And Create LP System Skeleton

**Files:**
- Create: `systems/lp/README.md`
- Create: `systems/lp/pyproject.toml`
- Create: `systems/lp/lp/__init__.py`
- Create: `systems/flux/flux/runners/shared/surface_proxy.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `systems/README.md`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`

**Step 1: Write the failing tests**

Add proxy tests proving the public Flux host can forward a non-strategy surface:

```python
def test_proxy_routes_lp_surface_paths_to_hidden_backend(client, monkeypatch):
    captured = {}

    def fake_proxy(url: str):
        captured["url"] = url
        return Response("ok", status=200)

    monkeypatch.setenv("LP_API_BACKEND_URL", "http://127.0.0.1:5025")
    app = _build_test_app()
    _attach_surface_router_proxy(app, proxy_request=fake_proxy)

    response = client.get("/lp")

    assert response.status_code == 200
    assert captured["url"] == "http://127.0.0.1:5025/lp"
```

```python
def test_proxy_routes_hedger_api_paths_to_hidden_lp_backend(client, monkeypatch):
    captured = {}
    monkeypatch.setenv("LP_API_BACKEND_URL", "http://127.0.0.1:5025")
    app = _build_test_app()
    _attach_surface_router_proxy(app, proxy_request=lambda url: captured.setdefault("url", url) or Response("ok"))

    client.get("/api/v1/hedgers/instances")

    assert captured["url"] == "http://127.0.0.1:5025/api/v1/hedgers/instances"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k "lp or hedger"
```

Expected: FAIL because the public proxy logic only knows about the equities backend.

**Step 3: Write the minimal implementation**

Introduce an explicit surface-proxy contract instead of another hard-coded branch:

```python
@dataclass(frozen=True, slots=True)
class SurfaceProxyDescriptor:
    surface: str
    base_paths: tuple[str, ...]
    backend_env_var: str
    api_prefixes: tuple[str, ...] = ()
```

Use it from `tokenmm.run_api` to proxy:

- `/equities` via `EQUITIES_API_BACKEND_URL`
- `/lp` plus `/api/v1/hedgers/*` via `LP_API_BACKEND_URL`

Create the `systems/lp/` skeleton now so the new surface has a canonical home before any runtime files are ported.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/lp/README.md \
  systems/lp/pyproject.toml \
  systems/lp/lp/__init__.py \
  systems/flux/flux/runners/shared/surface_proxy.py \
  systems/flux/flux/runners/tokenmm/run_api.py \
  systems/README.md \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py
git commit -m "refactor: generalize public surface proxying for lp"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 2: Add Stable Fluxboard `/lp` Profile And Retire Default `/hedger`

**Files:**
- Modify: `fluxboard/config/uiProfiles.ts`
- Modify: `fluxboard/main.tsx`
- Modify: `fluxboard/App.tsx`
- Modify: `fluxboard/Nav.tsx`
- Test: `fluxboard/config/uiProfiles.test.ts`
- Test: `fluxboard/main.routes.test.tsx`
- Test: `fluxboard/App.test.tsx`
- Test: `fluxboard/Nav.test.tsx`

**Step 1: Write the failing tests**

Add tests that pin the new profile and its dedicated landing route:

```ts
it('resolves /lp as the lp profile', () => {
  expect(resolvePathnameProfile('/lp')).toBe('lp');
  expect(buildProfilePath('lp', '/hedger')).toBe('/lp/hedger');
});
```

```tsx
it('routes /lp to the Hedger page instead of the generic dashboard', async () => {
  const routes = buildFluxboardTopLevelRoutes();
  expect(routes.some((route) => route.path === '/lp')).toBe(true);
});
```

```ts
it('does not expose hedger on the default surface anymore', () => {
  expect(getUiSurface('default').routePaths).not.toContain('/hedger');
  expect(getUiSurface('lp').routePaths).toContain('/hedger');
});
```

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm --dir fluxboard test -- --run config/uiProfiles.test.ts main.routes.test.tsx App.test.tsx Nav.test.tsx
```

Expected: FAIL because `lp` is not a known profile and the default surface still owns `/hedger`.

**Step 3: Write the minimal implementation**

Extend the profile contract:

```ts
export type PathProfile = 'default' | 'tokenmm' | 'equities' | 'lp';

const PROFILE_DEFINITIONS = {
  tokenmm: { profile: 'tokenmm', aliases: ['tokenmm', 'tokenm'], basePath: '/tokenmm' },
  equities: { profile: 'equities', aliases: ['equities'], basePath: '/equities' },
  lp: { profile: 'lp', aliases: ['lp'], basePath: '/lp' },
} as const;
```

Generalize the route builder so a profile can declare a non-dashboard home. The `lp` surface should use the Hedger page as both `/lp` and `/lp/hedger`.

**Step 4: Run tests to verify they pass**

Run the same pnpm command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/config/uiProfiles.ts \
  fluxboard/main.tsx \
  fluxboard/App.tsx \
  fluxboard/Nav.tsx \
  fluxboard/config/uiProfiles.test.ts \
  fluxboard/main.routes.test.tsx \
  fluxboard/App.test.tsx \
  fluxboard/Nav.test.tsx
git commit -m "feat: add dedicated lp fluxboard profile"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 3: Port Chainsaw Config And Registry Contracts Into `systems/lp`

**Files:**
- Create: `systems/lp/lp/config.py`
- Create: `systems/lp/lp/hedgers/__init__.py`
- Create: `systems/lp/lp/hedgers/models.py`
- Create: `systems/lp/lp/hedgers/registry.py`
- Test: `tests/unit_tests/lp/test_config.py`
- Test: `tests/unit_tests/lp/test_registry.py`

**Step 1: Write the failing tests**

Add tests that pin the chainsaw config surface and registry identity:

```python
def test_config_loader_accepts_chainsaw_keys_and_token_aliases(tmp_path: Path) -> None:
    cfg = write_ini(
        tmp_path,
        """
        [identity]
        id = eth_plume_lp
        label = ETH/PLUME LP Band1
        state_key = eth_plume_lp_hedger
        job_id = service-eth-plume-lp-hedger

        [lp_pool]
        token0_symbol = WETH
        token1_symbol = WPLUME
        initial_eth = 1.6085
        initial_plume = 169377
        price_lower = 85000
        price_upper = 111000

        [target]
        target_net_eth = 0.0
        target_net_plume = 0.0
        """
    )
    loaded = load_lp_hedger_config(cfg)
    assert loaded.hedger_id == "eth_plume_lp"
    assert loaded.state_key == "eth_plume_lp_hedger"
    assert loaded.job_id == "service-eth-plume-lp-hedger"
```

```python
def test_registry_preserves_chainsaw_env_var_names() -> None:
    meta = get_hedger_meta("eth_plume_lp")
    assert meta.config_env_var == "ETH_PLUME_LP_HEDGER_CONFIG"
    assert meta.mode_key == "eth_plume_lp_hedger:mode"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/lp/test_config.py tests/unit_tests/lp/test_registry.py
```

Expected: FAIL because `systems/lp` config and registry modules do not exist yet.

**Step 3: Write the minimal implementation**

Create a token0/token1-first config model with compatibility aliases:

```python
@dataclass(frozen=True, slots=True)
class LpHedgerConfig:
    hedger_id: str
    label: str
    job_id: str
    state_key: str
    token0_symbol: str
    token1_symbol: str
    initial_token0: Decimal
    initial_token1: Decimal
    target_net_token0: Decimal
    target_net_token1: Decimal
```

Rules:

1. Accept chainsaw legacy names (`initial_eth`, `target_net_plume`, `eth_symbol`, etc.).
2. Store canonical token0/token1 fields internally.
3. Preserve chainsaw env-var names and default config paths in registry metadata.
4. Read `job_id` from config/registry metadata instead of hard-coding it in API code.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/lp/lp/config.py \
  systems/lp/lp/hedgers/__init__.py \
  systems/lp/lp/hedgers/models.py \
  systems/lp/lp/hedgers/registry.py \
  tests/unit_tests/lp/test_config.py \
  tests/unit_tests/lp/test_registry.py
git commit -m "feat: port lp hedger config and registry contracts"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 4: Port Hedger Core Runtime And Fix Contract-Debt Bugs

**Files:**
- Create: `systems/lp/lp/execution/bybit.py`
- Create: `systems/lp/lp/market/rooster.py`
- Create: `systems/lp/lp/hedgers/core.py`
- Test: `tests/unit_tests/lp/test_core.py`

**Step 1: Write the failing tests**

Add focused runtime tests for parity and cleanup behavior:

```python
def test_snapshot_preserves_chainsaw_field_names_and_token_aliases() -> None:
    hedger = build_test_hedger()
    snapshot = hedger.build_snapshot()
    assert snapshot["lp_eth"] == snapshot["lp_token0"]
    assert snapshot["lp_plume"] == snapshot["lp_token1"]
    assert snapshot["net_eth"] == snapshot["net_token0"]
    assert snapshot["net_plume"] == snapshot["net_token1"]
```

```python
def test_target_values_are_respected_instead_of_overwritten() -> None:
    hedger = build_test_hedger(target_net_token0=Decimal("5"), target_net_token1=Decimal("10"))
    assert hedger.target_net_token0 == Decimal("5")
    assert hedger.target_net_token1 == Decimal("10")
```

```python
def test_max_slippage_bps_is_applied_or_rejected_explicitly() -> None:
    hedger = build_test_hedger(max_slippage_bps=Decimal("30"))
    order = hedger.build_market_order(...)
    assert order.max_slippage_bps == Decimal("30")
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/lp/test_core.py
```

Expected: FAIL because the runtime is not ported yet.

**Step 3: Write the minimal implementation**

Port the chainsaw runtime into `systems/lp/lp/hedgers/core.py`, but clean the semantics while preserving payload keys:

```python
snapshot["lp_token0"] = str(lp_token0)
snapshot["lp_token1"] = str(lp_token1)
snapshot["lp_eth"] = snapshot["lp_token0"]      # compatibility alias
snapshot["lp_plume"] = snapshot["lp_token1"]    # compatibility alias
```

Required cleanup in this task:

1. Honor configured targets instead of resetting them to initial LP amounts.
2. Make `max_slippage_bps` either part of order construction or remove it from the config contract with a migration note. Prefer implementing it.
3. Keep token0/token1 internals primary.
4. Keep Redis snapshot/event/state keys unchanged.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/lp/lp/execution/bybit.py \
  systems/lp/lp/market/rooster.py \
  systems/lp/lp/hedgers/core.py \
  tests/unit_tests/lp/test_core.py
git commit -m "feat: port lp hedger runtime core"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 5: Port Registry-Backed LP API Endpoints

**Files:**
- Create: `systems/lp/lp/api/__init__.py`
- Create: `systems/lp/lp/api/app.py`
- Test: `tests/unit_tests/lp/api/test_app.py`
- Test: `fluxboard/Hedger.test.tsx`

**Step 1: Write the failing tests**

Add backend tests for the public chainsaw-compatible API surface:

```python
def test_list_hedger_instances_returns_registry_metadata(client):
    response = client.get("/api/v1/hedgers/instances")
    data = response.get_json()["data"]
    assert data[0]["id"] == "eth_plume_lp"
```

```python
def test_status_endpoint_returns_chainsaw_payload_shape(client):
    response = client.get("/api/v1/hedgers/eth_plume_lp")
    payload = response.get_json()["data"]
    assert "snapshot" in payload
    assert "geometry_effective" in payload
    assert "threshold_effective" in payload
    assert "hedger_enabled" in payload
```

```tsx
it('loads the lp surface hedger page without default-profile assumptions', async () => {
  mockApi.listHedgerInstances.mockResolvedValue([{ id: 'eth_plume_lp', label: 'ETH/PLUME LP Band1' }]);
  mockApi.getHedgerStatusById.mockResolvedValue(buildStatus());
  render(<Hedger />);
  expect(await screen.findByText(/ETH\/PLUME LP/i)).toBeInTheDocument();
});
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/lp/api/test_app.py
pnpm --dir fluxboard test -- --run Hedger.test.tsx
```

Expected: FAIL because no LP backend app exists yet.

**Step 3: Write the minimal implementation**

Expose the chainsaw-compatible routes from the new LP backend:

- `GET /api/v1/hedgers/instances`
- `GET /api/v1/hedgers/<hedger_id>`
- `POST /api/v1/hedgers/<hedger_id>/job`
- `GET /api/v1/hedgers/<hedger_id>/config`
- `PATCH /api/v1/hedgers/<hedger_id>/config`
- `POST|DELETE /api/v1/hedgers/<hedger_id>/geometry-overrides`
- `POST|DELETE /api/v1/hedgers/<hedger_id>/threshold-overrides`
- `POST /api/v1/hedgers/<hedger_id>/enabled`
- `POST /api/v1/hedgers/<hedger_id>/events/clear`

Do not hard-code ETH/PLUME route variants outside thin backward-compat shims.

**Step 4: Run tests to verify they pass**

Run the same pytest and pnpm commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/lp/lp/api/__init__.py \
  systems/lp/lp/api/app.py \
  tests/unit_tests/lp/api/test_app.py \
  fluxboard/Hedger.test.tsx
git commit -m "feat: add lp hedger api surface"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 6: Port Service Runners And Credential/Env Resolution

**Files:**
- Create: `systems/lp/lp/runners/__init__.py`
- Create: `systems/lp/lp/runners/run_api.py`
- Create: `systems/lp/lp/runners/run_hedger.py`
- Test: `tests/unit_tests/lp/runners/test_run_api.py`
- Test: `tests/unit_tests/lp/runners/test_run_hedger.py`

**Step 1: Write the failing tests**

Add tests for credential precedence and env handling:

```python
def test_run_hedger_uses_same_credential_precedence_as_chainsaw(tmp_path: Path):
    creds = resolve_bybit_credentials(system_cfg=tmp_path / "config.ini", hedger_cfg=tmp_path / "hedger.ini")
    assert creds == ("from_hedger", "from_hedger_secret")
```

```python
def test_band2_runner_does_not_delete_redis_url(monkeypatch):
    monkeypatch.setenv("REDIS_URL", "redis://example")
    runner = build_runner_for_band2()
    runner.build_redis_client()
    assert os.environ["REDIS_URL"] == "redis://example"
```

```python
def test_lp_api_runner_binds_hidden_backend_default_port():
    args = parse_args(["--config", "deploy/lp/lp.live.toml"])
    assert resolve_bind_port(args, {}) == 5025
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/lp/runners/test_run_api.py tests/unit_tests/lp/runners/test_run_hedger.py
```

Expected: FAIL because the runners do not exist yet.

**Step 3: Write the minimal implementation**

Implement:

1. A generic `run_hedger.py --config <ini> --system-config <ini>` runner that can launch any registered hedger instance.
2. Chainsaw-compatible Bybit credential precedence:
   - hedger `[bybit] api_key/api_secret`
   - system `[bybit_hedger_band2]` when relevant
   - system `[bybit_hedger]`
   - system `[bybit]`
3. A hidden `run_api.py` runner for the LP backend on `127.0.0.1:5025`.
4. Compatibility env var support:
   - `ETH_PLUME_LP_HEDGER_CONFIG`
   - `ETH_PLUME_LP_HEDGER_BAND2_CONFIG`
   - `HYPE_USDT_LP_HEDGER_CONFIG`
   - `PLUME_WETH_LP_HEDGER_CONFIG`
   - `THIRD_LP_HEDGER_CONFIG`

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/lp/lp/runners/__init__.py \
  systems/lp/lp/runners/run_api.py \
  systems/lp/lp/runners/run_hedger.py \
  tests/unit_tests/lp/runners/test_run_api.py \
  tests/unit_tests/lp/runners/test_run_hedger.py
git commit -m "feat: add lp hedger runners and env handling"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 7: Add Deploy, Pulse, And Documentation Surfaces Under `deploy/lp`

**Files:**
- Create: `deploy/lp/README.md`
- Create: `deploy/lp/lp.live.toml`
- Create: `deploy/lp/lp_stack.env.example`
- Create: `deploy/lp/hedgers/eth_plume_lp_hedger.ini`
- Create: `deploy/lp/hedgers/eth_plume_lp_hedger_band2.ini`
- Create: `deploy/lp/hedgers/hype_usdt_lp_hedger.ini.disabled`
- Create: `deploy/lp/hedgers/plume_weth_lp_hedger.ini.disabled`
- Create: `deploy/lp/hedgers/third_lp_hedger.ini.disabled`
- Create: `deploy/lp/hedgers/lp_hedger.template.ini`
- Create: `deploy/lp/systemd/common.env.example`
- Create: `deploy/lp/systemd/flux-lp.target`
- Create: `deploy/lp/systemd/flux-pulse.sudoers`
- Create: `ops/scripts/deploy/install_lp_systemd.sh`
- Create: `ops/scripts/deploy/lp_stack.sh`
- Create: `fluxboard/docs/lp_contract.md`
- Modify: `deploy/tokenmm/systemd/common.env.example`
- Test: `tests/unit_tests/examples/lp/test_lp_stack_contract.py`

**Step 1: Write the failing tests**

Add deploy contract tests:

```python
def test_lp_common_env_mentions_hidden_backend_proxy():
    content = _read(_repo_root() / "deploy/lp/systemd/common.env.example")
    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in content
```

```python
def test_disabled_extra_hedgers_are_not_auto_enrolled():
    configs = strategy_stack_discover_configs(_repo_root() / "deploy/lp/hedgers")
    assert "hype_usdt_lp_hedger" not in configs
```

```python
def test_lp_contract_doc_mentions_same_redis_key_family():
    doc = _read(_repo_root() / "fluxboard/docs/lp_contract.md")
    assert ":snapshot" in doc
    assert ":geometry_overrides" in doc
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/lp/test_lp_stack_contract.py
```

Expected: FAIL because the deploy root and contract docs do not exist yet.

**Step 3: Write the minimal implementation**

Create a dedicated LP deploy root. Keep the same config keys, but sanitize checked-in secrets:

```ini
[bybit]
eth_symbol = ETHUSDT
eth_qty_step = 0.001
plume_symbol = PLUMEUSDT
plume_qty_step = 1
api_key =
api_secret =
```

Rules for this task:

1. Active checked-in configs are Band1 and Band2 only.
2. Extra chainsaw configs are ported as `.disabled` or template-only until validated.
3. Public host docs mention `LP_API_BACKEND_URL`.
4. Docs explicitly call out the same chainsaw key contracts and the cleanup changes made in this repo.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  deploy/lp/README.md \
  deploy/lp/lp.live.toml \
  deploy/lp/lp_stack.env.example \
  deploy/lp/hedgers \
  deploy/lp/systemd/common.env.example \
  deploy/lp/systemd/flux-lp.target \
  deploy/lp/systemd/flux-pulse.sudoers \
  ops/scripts/deploy/install_lp_systemd.sh \
  ops/scripts/deploy/lp_stack.sh \
  fluxboard/docs/lp_contract.md \
  deploy/tokenmm/systemd/common.env.example \
  tests/unit_tests/examples/lp/test_lp_stack_contract.py
git commit -m "feat: add lp deploy and contract surfaces"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 8: Verify End-To-End Contracts And Close Out Residual Risk

**Files:**
- Modify: `docs/plans/2026-03-09-lp-hedger-port.md`
- Create: `docs/reviews/2026-03-09-lp-hedger-port-review-summary.md`

**Step 1: Run the targeted automated suites**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/lp/test_config.py \
  tests/unit_tests/lp/test_registry.py \
  tests/unit_tests/lp/test_core.py \
  tests/unit_tests/lp/api/test_app.py \
  tests/unit_tests/lp/runners/test_run_api.py \
  tests/unit_tests/lp/runners/test_run_hedger.py \
  tests/unit_tests/examples/lp/test_lp_stack_contract.py
```

```bash
pnpm --dir fluxboard test -- --run \
  config/uiProfiles.test.ts \
  main.routes.test.tsx \
  App.test.tsx \
  Nav.test.tsx \
  Hedger.test.tsx
```

Expected: PASS.

**Step 2: Run the local smoke workflow**

Run:

```bash
cp deploy/lp/lp_stack.env.example deploy/lp/lp_stack.env
LP_MODE=paper \
LP_CONFIRM_LIVE=0 \
LP_ENABLE_EXECUTION=0 \
ops/scripts/deploy/lp_stack.sh start
curl -fsS http://127.0.0.1:5022/lp
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/instances
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/eth_plume_lp
ops/scripts/deploy/lp_stack.sh stop
```

Expected:

1. `/lp` serves the Fluxboard SPA.
2. `instances` returns the registered hedger list.
3. `eth_plume_lp` returns a chainsaw-compatible payload.

**Step 3: Request review in parallel**

Use fresh subagents in parallel:

1. Spec reviewer on `systems/lp` contract parity vs chainsaw.
2. Quality reviewer on public proxying and frontend route cleanup.
3. Optional security review on checked-in configs and secret handling.

Record findings in:

```markdown
# LP Hedger Port Review Summary

- Spec findings:
- Quality findings:
- Security findings:
- Residual risks:
```

**Step 4: Update docs and residual risks**

Document any remaining gaps explicitly:

1. No Socket.IO contract for LP v1.
2. Hidden backend public-port assumptions.
3. Any chainsaw behaviors intentionally dropped.

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-09-lp-hedger-port.md \
  docs/reviews/2026-03-09-lp-hedger-port-review-summary.md
git commit -m "docs: finalize lp hedger port verification record"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Parallelization Notes

1. Task 1 must land first because it creates the public proxy seam and system home.
2. After Task 1, Task 2 and Task 3 can proceed in parallel:
   - one subagent on Fluxboard `/lp`
   - one subagent on `systems/lp` config/registry
3. Task 4 and Task 5 can then split:
   - one subagent on runtime core
   - one subagent on Flask API and payload contract tests
4. Task 6 and Task 7 can overlap once the API/config contract is stable.
5. Task 8 must be done centrally after integration.

## Open Questions Resolved By This Plan

1. **Where should the port live?**
   Answer: `systems/lp/` plus `deploy/lp/`, not `flux.strategies/*`.

2. **How do we keep “same keys as chainsaw”?**
   Answer: preserve hedger IDs, job IDs, Redis key families, env var names, config key names, and payload field names; clean up behavior behind those contracts rather than renaming them.

3. **How should `/lp` fit the monorepo?**
   Answer: as a first-class Fluxboard profile and public proxy surface, with its own hidden backend and no default-surface duplication.
