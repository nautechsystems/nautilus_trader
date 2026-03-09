# LP Hedger Production Rollout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Take the merged LP hedger port, including final Chainsaw GUI parity for the LP Hedger surface, from repo-ready to production-ready on the shared Flux host, with explicit rollout, validation, and rollback contracts for `/lp`, `/api/v1/hedgers/*`, and the Band1/Band2 systemd/Pulse service set.

**Architecture:** Assume `docs/plans/2026-03-09-lp-hedger-port.md` has landed first. Keep the public TokenMM host on `:5022`, keep the hidden `lp-api` backend on loopback `:5025`, and treat productionization as hardening the remaining seams around that topology: final GUI parity/porting of the Chainsaw `/hedger` operator surface onto monorepo `/lp`, shared-host static asset serving, preflight validation, systemd/Pulse enrollment, runbook quality, and cutover evidence. Do not change hedger IDs, job IDs, Redis key families, or the `/lp` route contract during this rollout follow-up.

**Tech Stack:** Python 3.12+, Flask, existing Flux API/Fluxboard shared host, Vite build output, systemd, Pulse, Redis, TOML/INI configs, shell deploy scripts, pytest, Vitest.

**Dependency:** This plan assumes the LP port work is merged first, including `systems/lp/`, `deploy/lp/`, shared-host `/lp` routing, and the current targeted test coverage.

## Scope

**In scope**

1. Audit the Chainsaw Fluxboard LP Hedger GUI and port any remaining operator-visible parity gaps into the monorepo `/lp` profile.
2. Harden the shared public host so `/lp` is production-safe and does not depend on TokenMM-specific static asset paths.
3. Add production rollout docs, preflight checks, and rollout health-check automation for the LP stack.
4. Tighten systemd/Pulse/common-env contracts so Band1 and Band2 can be enrolled safely on the existing shared host.
5. Record a concrete canary rollout procedure, go/no-go gates, and rollback path.

**Out of scope**

1. No new LP hedger instances beyond Band1 and Band2.
2. No net-new LP workflows beyond matching the Chainsaw operator surface and hardening it for prod rollout.
3. No removal of Chainsaw rollback material during this PR.
4. No changes to chainsaw-compatible hedger IDs, job IDs, Redis key names, or payload field names.

## Design Constraints

1. `/lp` remains the canonical public UI route and `/api/v1/hedgers/*` remains the only LP API path family.
2. The shared public host on `:5022` continues to serve Fluxboard and Pulse; `lp-api` stays hidden on loopback.
3. The rollout must remain reversible without changing user-facing routes.
4. Band1 and Band2 are the only checked-in active production instances for this rollout.
5. Repo docs and examples may name required secret keys, but may not contain live secret values.

## Acceptance Criteria

1. The shared `/lp` profile is explicitly validated against the Chainsaw Hedger GUI contract: instance selector, operator state pills, restart, enable/disable, recent-hedges clear, geometry overrides, threshold overrides, config editor, and Band1/Band2 selection all remain available on the monorepo surface.
2. Production `/lp` no longer relies on `/tokenmm/assets/...` as an implicit static-asset side effect; the shared host serves Fluxboard assets from an explicit shared asset prefix or equivalent neutral contract.
3. The repo contains a production rollout runbook covering GUI parity notes, preflight, install, restart order, smoke validation, Pulse checks, and rollback.
4. The repo contains an LP preflight/audit tool that validates the shared-host env/config assumptions before operators start `flux-lp.target`.
5. `install_lp_systemd.sh` and the checked-in LP env examples are explicit enough to enroll Band1/Band2 safely on the shared host without colliding with the public TokenMM service.
6. The production rollout includes a scripted health-check path that validates `/lp`, the LP hedger API, and the Pulse service set after cutover.
7. The rollout review captures final evidence and any residual risks explicitly.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | not_started | unassigned | Plan created |
| Task 1: Freeze The Production LP Rollout Contract | not_started | unassigned | Plan created |
| Task 2: Audit And Port Remaining Chainsaw LP GUI Parity To `/lp` | not_started | unassigned | Plan created |
| Task 3: Decouple `/lp` From TokenMM-Specific Static Asset Paths | not_started | unassigned | Plan created |
| Task 4: Add LP Preflight Audit Tooling For Shared-Host Production | not_started | unassigned | Plan created |
| Task 5: Harden LP Systemd And Shared-Host Env Contracts | not_started | unassigned | Plan created |
| Task 6: Add Rollout Health Checks, Canary Procedure, And Rollback Evidence | not_started | unassigned | Plan created |

---

## Files Already Known To Be In Play

- `deploy/lp/README.md`
- `deploy/lp/lp.live.toml`
- `deploy/lp/systemd/common.env.example`
- `deploy/lp/systemd/flux-lp.target`
- `deploy/lp/systemd/flux-pulse.sudoers`
- `deploy/lp/hedgers/eth_plume_lp_hedger.ini`
- `deploy/lp/hedgers/eth_plume_lp_hedger_band2.ini`
- `deploy/tokenmm/systemd/common.env.example`
- `fluxboard/Hedger.tsx`
- `fluxboard/Hedger.test.tsx`
- `fluxboard/api.ts`
- `fluxboard/main.tsx`
- `fluxboard/main.routes.test.tsx`
- `fluxboard/config/uiProfiles.ts`
- `fluxboard/config/uiProfiles.test.ts`
- `fluxboard/vite.config.ts`
- `fluxboard/index.html`
- `fluxboard/docs/lp_contract.md`
- `systems/flux/flux/runners/tokenmm/run_api.py`
- `ops/scripts/deploy/install_lp_systemd.sh`
- `ops/scripts/deploy/lp_stack.sh`
- `ops/scripts/deploy/check_flux_host_baseline.sh`
- `ops/scripts/deploy/install_flux_host_baseline.sh`
- `tests/unit_tests/examples/lp/test_lp_stack_contract.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`

## Task 1: Freeze The Production LP Rollout Contract

**Files:**

- Create: `docs/runbooks/lp-hedger-production-rollout.md`
- Create: `tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py`
- Modify: `deploy/lp/README.md`
- Modify: `fluxboard/docs/lp_contract.md`

**Step 1: Write the failing contract tests**

Add docs/contract tests that pin the intended production topology and operator path:

```python
def test_lp_prod_runbook_documents_shared_host_topology() -> None:
    text = read("docs/runbooks/lp-hedger-production-rollout.md")
    assert "/lp" in text
    assert "/api/v1/hedgers/*" in text
    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in text
    assert "flux-lp.target" in text
    assert "service-eth-plume-lp-hedger" in text
    assert "service-eth-plume-lp-hedger-band2" in text
    assert "rollback" in text.lower()
```

```python
def test_lp_prod_docs_keep_band1_band2_as_only_active_instances() -> None:
    text = read("deploy/lp/README.md")
    assert "Band1 and Band2" in text
    assert ".ini.disabled" in text
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py
```

Expected: FAIL because the production runbook and rollout-specific contract doc do not exist yet.

**Step 3: Write the minimal implementation**

Document:

- shared-host topology (`:5022` public host, `:5025` hidden `lp-api`)
- `/lp` as the production route for the Chainsaw LP Hedger operator surface
- required operator-managed files:
  - `/etc/flux/common.env`
  - `/etc/flux/lp-system.ini`
  - `/etc/flux/lp-api.env`
  - `/etc/flux/service-eth-plume-lp-hedger.env`
  - `/etc/flux/service-eth-plume-lp-hedger-band2.env`
- preflight, install, restart order, smoke validation, and rollback
- the fact that Band1/Band2 are the only active checked-in production instances

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  docs/runbooks/lp-hedger-production-rollout.md \
  tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py \
  deploy/lp/README.md \
  fluxboard/docs/lp_contract.md
git commit -m "docs: freeze lp hedger production rollout contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 2: Audit And Port Remaining Chainsaw LP GUI Parity To `/lp`

**Files:**

- Modify: `fluxboard/Hedger.tsx`
- Modify: `fluxboard/Hedger.test.tsx`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/main.tsx`
- Modify: `fluxboard/main.routes.test.tsx`
- Modify: `fluxboard/config/uiProfiles.ts`
- Modify: `fluxboard/config/uiProfiles.test.ts`
- Modify: `fluxboard/docs/lp_contract.md`
- Modify: `fluxboard/types.ts`

**Step 1: Write the failing tests**

Add targeted Fluxboard parity tests that pin the Chainsaw operator surface now expected at `/lp`:

```tsx
it('treats /lp as the LP Hedger home surface', () => {
  const routes = buildFluxboardChildRoutes(getUiSurface('lp'), {
    includeScannersHarness: false,
    fallbackPath: '/lp',
  });

  expect(routes[0]?.element).toBeTruthy();
  expect(getUiSurface('lp').homeRoutePath).toBe('/hedger');
});
```

```tsx
it('keeps Chainsaw hedger operator controls available on the monorepo LP surface', async () => {
  render(<Hedger />);
  expect(await screen.findByRole('button', { name: /Edit Config/i })).toBeEnabled();
  expect(screen.getByRole('button', { name: /Restart/i })).toBeEnabled();
  expect(screen.getByRole('button', { name: /Disable Hedger|Enable Hedger/i })).toBeEnabled();
  expect(screen.getByRole('button', { name: /Clear/i })).toBeEnabled();
});
```

Expand the parity suite to cover the known Chainsaw-sensitive paths:

- multi-instance selector including `eth_plume_lp` and `eth_plume_lp_band2`
- config edit flow for both ETH/PLUME and non-ETH LP hedgers
- geometry override and threshold override editors
- recent-hedges clearing
- generic by-ID API usage instead of hard-coded ETH/PLUME-only UI paths where the monorepo has already generalized behavior

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm --dir fluxboard test:run -- Hedger.test.tsx main.routes.test.tsx config/uiProfiles.test.ts
```

Expected: FAIL until any remaining GUI parity gaps between Chainsaw `/hedger` and monorepo `/lp` are closed or their intended deltas are documented explicitly.

**Step 3: Write the minimal implementation**

Use Chainsaw as the UI reference and make `/lp` the explicit home for that surface:

- keep the `lp` Fluxboard profile pointed at the Hedger screen
- preserve the operator controls Chainsaw expects:
  - instance selector
  - state pills and restart / enable-disable controls
  - config editor
  - geometry overrides
  - threshold overrides
  - recent-hedges clear
- document any intentional monorepo improvements over Chainsaw in `fluxboard/docs/lp_contract.md` instead of leaving them as accidental drift

Prefer porting missing behavior into the shared Hedger surface rather than forking a separate LP-only app.

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm --dir fluxboard test:run -- Hedger.test.tsx main.routes.test.tsx config/uiProfiles.test.ts
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/Hedger.tsx \
  fluxboard/Hedger.test.tsx \
  fluxboard/api.ts \
  fluxboard/main.tsx \
  fluxboard/main.routes.test.tsx \
  fluxboard/config/uiProfiles.ts \
  fluxboard/config/uiProfiles.test.ts \
  fluxboard/docs/lp_contract.md \
  fluxboard/types.ts
git commit -m "feat: align lp fluxboard gui with chainsaw hedger"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 3: Decouple `/lp` From TokenMM-Specific Static Asset Paths

**Files:**

- Modify: `fluxboard/vite.config.ts`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Modify: `fluxboard/docs/lp_contract.md`

**Step 1: Write the failing tests**

Add a contract test that proves the shared public host can serve Fluxboard assets for both `/tokenmm` and `/lp` without `/lp` depending on `/tokenmm/assets/...`:

```python
def test_attach_fluxboard_routes_serve_neutral_shared_asset_prefix(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    (dist_dir / "index.html").write_text(
        '<script type="module" src="/static/fluxboard/assets/app.js"></script>',
        encoding="utf-8",
    )
    (dist_dir / "assets").mkdir()
    (dist_dir / "assets" / "app.js").write_text("console.log('shared')", encoding="utf-8")

    app = Flask(__name__)
    _attach_fluxboard_tokenmm_routes(app, dist_dir=dist_dir)
    client = app.test_client()

    assert client.get("/lp").status_code == 200
    assert client.get("/tokenmm").status_code == 200
    assert client.get("/static/fluxboard/assets/app.js").status_code == 200
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k "shared_asset_prefix or lp"
```

Expected: FAIL because the current build/runtime contract still anchors production assets to the TokenMM base path.

**Step 3: Write the minimal implementation**

Move the built Fluxboard asset contract to a neutral shared prefix, for example `/static/fluxboard/`, by:

- changing the production Vite base path from `/tokenmm/` to the neutral shared prefix
- teaching the shared public host to serve that neutral asset prefix
- keeping `/tokenmm`, `/tokenm`, and `/lp` as SPA route entries, but not as asset path owners

Do not change the user-facing route contract for `/tokenmm` or `/lp`.

**Step 4: Run tests to verify they pass**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k "shared_asset_prefix or lp"
pnpm --dir fluxboard build
rg -n "/static/fluxboard/assets/" fluxboard/dist/index.html
```

Expected: PASS, and the built Fluxboard HTML references the neutral shared asset prefix.

**Step 5: Commit**

```bash
git add \
  fluxboard/vite.config.ts \
  systems/flux/flux/runners/tokenmm/run_api.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  fluxboard/docs/lp_contract.md
git commit -m "fix: decouple lp production assets from tokenmm base path"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 4: Add LP Preflight Audit Tooling For Shared-Host Production

**Files:**

- Create: `ops/scripts/lp_hedger_preflight.py`
- Modify: `deploy/lp/README.md`
- Modify: `docs/runbooks/lp-hedger-production-rollout.md`
- Modify: `tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py`

**Step 1: Write the failing tests**

Add tests for a host-preflight tool that validates the production assumptions before enabling the LP stack:

```python
def test_lp_preflight_requires_lp_backend_url_and_system_ini_sections(tmp_path: Path) -> None:
    common_env = tmp_path / "common.env"
    common_env.write_text("LP_API_BACKEND_URL=http://127.0.0.1:5025\n", encoding="utf-8")
    system_ini = tmp_path / "lp-system.ini"
    system_ini.write_text("[redis]\nurl=redis://example\n[plume]\nrpc_url=http://rpc\n", encoding="utf-8")

    result = run_preflight(common_env=common_env, system_ini=system_ini)

    assert result.ok is False
    assert "bybit_hedger" in result.errors
```

```python
def test_lp_preflight_accepts_band1_band2_config_contract(tmp_path: Path) -> None:
    ...
    assert result.ok is True
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py -k preflight
```

Expected: FAIL because the LP preflight tool does not exist yet.

**Step 3: Write the minimal implementation**

Create a small operator tool that can validate:

- `LP_API_BACKEND_URL` is present and loopback-targeted
- `/etc/flux/lp-system.ini` contains:
  - `[redis]`
  - `[plume]`
  - `[bybit]`
  - `[bybit_hedger]`
  - `[bybit_hedger_band2]`
- the Band1/Band2 INI paths exist and are readable
- hidden/public ports are not obviously misconfigured for the shared host

Prefer machine-readable output (`--json`) plus a human-readable summary.

**Step 4: Run tests to verify they pass**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py -k preflight
python3 ops/scripts/lp_hedger_preflight.py --help
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  ops/scripts/lp_hedger_preflight.py \
  deploy/lp/README.md \
  docs/runbooks/lp-hedger-production-rollout.md \
  tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py
git commit -m "feat: add lp hedger production preflight audit"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 5: Harden LP Systemd And Shared-Host Env Contracts

**Files:**

- Modify: `ops/scripts/deploy/install_lp_systemd.sh`
- Modify: `deploy/lp/systemd/common.env.example`
- Modify: `deploy/lp/README.md`
- Modify: `tests/unit_tests/examples/lp/test_lp_stack_contract.py`
- Modify: `tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py`

**Step 1: Write the failing tests**

Add contract tests that pin the production install behavior:

```python
def test_lp_systemd_contract_documents_shared_host_env_requirements() -> None:
    text = read("deploy/lp/systemd/common.env.example")
    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in text
    assert "LP_SYSTEM_CONFIG=/etc/flux/lp-system.ini" in text
```

```python
def test_install_lp_systemd_references_band1_band2_only_and_shared_host_restart_order() -> None:
    script = read("ops/scripts/deploy/install_lp_systemd.sh")
    assert "lp-api.env" in script
    assert "service-eth-plume-lp-hedger.env" in script
    assert "service-eth-plume-lp-hedger-band2.env" in script
    assert "service-hedger3" not in script
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/lp/test_lp_stack_contract.py \
  tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py
```

Expected: FAIL on any missing shared-host env or installer guarantees that are still implicit.

**Step 3: Write the minimal implementation**

Make the installer/docs explicit about:

- the shared public host dependency on `LP_API_BACKEND_URL`
- the operator-managed `LP_SYSTEM_CONFIG`
- Band1/Band2-only enrollment
- restart ordering when the shared public host must pick up new common env
- coexistence with the already-running public TokenMM service on `:5022`

If needed, make the installer emit a clear warning instead of silently assuming the public host has already been restarted.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/install_lp_systemd.sh \
  deploy/lp/systemd/common.env.example \
  deploy/lp/README.md \
  tests/unit_tests/examples/lp/test_lp_stack_contract.py \
  tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py
git commit -m "docs: harden lp production systemd and env contracts"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Task 6: Add Rollout Health Checks, Canary Procedure, And Rollback Evidence

**Files:**

- Create: `ops/scripts/deploy/check_lp_rollout.sh`
- Create: `docs/reviews/YYYY-MM-DD-lp-hedger-prod-rollout.md`
- Modify: `docs/runbooks/lp-hedger-production-rollout.md`
- Modify: `deploy/lp/README.md`
- Modify: `tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py`

**Step 1: Write the failing tests**

Add rollout contract tests that pin the post-cutover evidence path:

```python
def test_lp_rollout_check_script_covers_ui_api_and_pulse() -> None:
    script_lines = read("ops/scripts/deploy/check_lp_rollout.sh").splitlines()
    assert any("curl" in line and "/lp" in line for line in script_lines)
    assert any("curl" in line and "/api/v1/hedgers/instances" in line for line in script_lines)
    assert any("curl" in line and "/api/v1/hedgers/eth_plume_lp" in line for line in script_lines)
    assert any("curl" in line and "/api/pulse/jobs" in line for line in script_lines)
```

```python
def test_lp_prod_runbook_documents_go_no_go_and_rollback() -> None:
    text = read("docs/runbooks/lp-hedger-production-rollout.md")
    assert "go/no-go" in text.lower()
    assert "rollback" in text.lower()
    assert "chainsaw" in text.lower()
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py -k "rollout or rollback"
```

Expected: FAIL because the rollout-check script and final evidence template do not exist yet.

**Step 3: Write the minimal implementation**

Add:

- a small rollout-check script that curls:
  - `/lp`
  - `/api/v1/hedgers/instances`
  - `/api/v1/hedgers/eth_plume_lp`
  - `/api/pulse/jobs`
- a runbook section covering:
  - preflight
  - install/restart order
  - canary validation
  - rollback trigger conditions
  - chainsaw fallback/standby path
- a review template that captures:
  - exact restart times
  - smoke results
  - Pulse screenshots/data
  - residual risks after cutover

**Step 4: Run tests to verify they pass**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py -k "rollout or rollback"
bash ops/scripts/deploy/check_lp_rollout.sh --help
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/check_lp_rollout.sh \
  docs/reviews/YYYY-MM-DD-lp-hedger-prod-rollout.md \
  docs/runbooks/lp-hedger-production-rollout.md \
  deploy/lp/README.md \
  tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py
git commit -m "ops: add lp rollout health checks and rollback evidence"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
