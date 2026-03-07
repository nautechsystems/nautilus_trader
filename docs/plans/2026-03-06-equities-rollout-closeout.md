# Equities trade[XYZ] Rollout Closeout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **Execution mode:** Keep `/equities`, `profile=equities`, `portfolio=equities`, and the current trade[XYZ]-on-Hyperliquid venue model stable. Do not add speculative mixed-version strategy support. Refactor only the shared seams already duplicated by TokenMM and Equities.

**Goal:** Finish the equities rollout by hardening the shared runtime/profile/deploy seams, keeping the outer equities surface stable, and leaving the codebase ready for more strategy sets without requiring another round of copy-paste infrastructure work.

**Architecture:** Treat `equities` as a first-class strategy set with a stable API/UI/deploy identity. Extract the duplicated TokenMM/Equities runner and deploy plumbing into shared utilities, make backend strategy metadata explicit enough for Fluxboard to stop guessing, and keep strategy-core behavior local to MakerV3 for now. Close the rollout with docs, deploy contracts, and a comprehensive verification/review pass.

**Tech Stack:** Python (Flux API/runners), Rust + PyO3 (Hyperliquid adapter already in scope), React/TypeScript (Fluxboard), TOML configs, Redis, systemd, pytest, Vitest, shell deploy scripts.

---

## Scope

**In scope**

1. Keep the current equities production surface stable:
   - `/equities`
   - `profile=equities`
   - `portfolio=equities`
   - `equities_strategy_ids`
   - `equities_required_strategy_ids`
   - Pulse group `equities`
2. Extract duplicated TokenMM/Equities runner bootstrap and profile/deploy plumbing where the abstractions are already obvious.
3. Make backend strategy metadata explicit enough that Fluxboard can key off metadata rather than parameter-shape guessing.
4. Keep trade[XYZ] as `HYPERLIQUID` plus `dex`/account-targeting, not a fake separate exchange family.
5. Finish docs, deploy contracts, end-to-end verification, and a final review for the equities port.

**Out of scope**

1. No `MakerV4` strategy implementation.
2. No mixed `MakerV3`/future-version coexistence in one equities surface.
3. No new venue family beyond the existing Hyperliquid + `dex` model.
4. No generic “maker base class” abstraction.
5. No unrelated repo-wide cleanup beyond seams directly duplicated by TokenMM and Equities.

## Design constraints

1. The equities rollout should look operationally like TokenMM, but should not remain a second copy of TokenMM internals.
2. Shared abstractions must come from already duplicated code, not from hypothetical future requirements.
3. Fluxboard should consume explicit backend metadata wherever possible.
4. Strategy-core logic remains local to `flux/strategies/makerv3/*` until a real strategy delta exists.
5. Existing trade[XYZ] adapter work is assumed complete enough to support rollout closeout; this plan is about finishing the product/ops surface around it.

## Acceptance criteria

1. TokenMM and Equities runner entrypoints rely on shared bootstrap helpers for config loading, Redis wiring, allowlist parsing, and startup locks.
2. TokenMM and Equities API entrypoints rely on shared profile/strategy-set descriptor data for base paths, aliases, allowlist fields, and Pulse identity.
3. Flux API and Fluxboard consume explicit strategy metadata for profile/family/param-set decisions instead of relying only on parameter signatures.
4. Equities docs and deploy scripts are production-ready and remain aligned with TokenMM conventions without duplicating large behavior blocks.
5. The equities rollout has a final verification record that covers adapter, backend, frontend, deploy contract, and namespace identity checks.
6. Any remaining failures are explicitly documented as unrelated residual issues, not silently ignored.

## Files already known to be in play

- `flux/api/app.py`
- `flux/api/socketio.py`
- `flux/api/payloads.py`
- `systems/flux/flux/runners/live/venues.py`
- `systems/flux/flux/runners/tokenmm/run_api.py`
- `systems/flux/flux/runners/tokenmm/run_node.py`
- `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- `systems/flux/flux/runners/tokenmm/redis_runtime.py`
- `systems/flux/flux/runners/equities/run_api.py`
- `systems/flux/flux/runners/equities/run_node.py`
- `systems/flux/flux/runners/equities/run_portfolio.py`
- `systems/flux/flux/runners/equities/redis_runtime.py`
- `flux/common/params.py`
- `flux/common/portfolio_inventory.py`
- `flux/strategies/__init__.py`
- `flux/strategies/makerv3/strategy.py`
- `flux/strategies/makerv3/runtime_params.py`
- `fluxboard/config/uiProfiles.ts`
- `fluxboard/config/paramsProfiles.ts`
- `fluxboard/api.ts`
- `fluxboard/types.ts`
- `deploy/tokenmm/README.md`
- `deploy/tokenmm/tokenmm.live.toml`
- `deploy/equities/README.md`
- `deploy/equities/equities.live.toml`
- `deploy/equities/strategies/README.md`
- `deploy/equities/systemd/common.env.example`
- `ops/scripts/deploy/install_tokenmm_systemd.sh`
- `ops/scripts/deploy/install_equities_systemd.sh`
- `ops/scripts/deploy/tokenmm_stack.sh`
- `ops/scripts/deploy/equities_stack.sh`
- `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py`
- `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- `tests/unit_tests/flux/api/test_app.py`
- `tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- `tests/unit_tests/flux/api/test_payloads.py`
- `tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py`

## Task 1: Introduce a shared strategy-set/profile descriptor

**Files:**
- Create: `systems/flux/flux/runners/shared/strategy_set.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `flux/api/app.py`
- Modify: `flux/api/socketio.py`
- Modify: `fluxboard/config/uiProfiles.ts`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- Test: `fluxboard/config/uiProfiles.test.ts`

**Step 1: Write the failing tests**

Add tests that prove TokenMM and Equities can be described from stable descriptor data instead of hard-coded per-file literals.

```python
def test_equities_run_api_uses_equities_descriptor_fields() -> None:
    config = {"api": {"equities_strategy_ids": ["aapl_tradexyz_makerv3"]}}
    summary = build_api_summary(config)
    assert "profile=equities" in summary
    assert "equities_strategy_ids=['aapl_tradexyz_makerv3']" in summary
```

```ts
it('resolves equities and tokenmm surfaces from stable profile definitions', () => {
  expect(getUiSurface('equities').profile).toBe('equities');
  expect(buildProfilePath('equities', '/signal')).toBe('/equities/signal');
});
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py

cd fluxboard && npm test -- --run config/uiProfiles.test.ts
```

Expected: FAIL because profile/base-path/allowlist identity is still duplicated across API entrypoints and consumers.

**Step 3: Write the minimal implementation**

Create a descriptor object with explicit fields:

```python
@dataclass(frozen=True, slots=True)
class StrategySetDescriptor:
    profile: str
    aliases: tuple[str, ...]
    base_path: str
    strategy_ids_field: str
    required_strategy_ids_field: str
    default_portfolio_id: str
    env_prefix: str
    pulse_group_key: str
    lock_dir_name: str
```

Use it from:
- TokenMM and Equities `run_api.py`
- Flux API profile resolution and allowlist fanout
- Fluxboard surface routing

Do not change the external `equities` or `tokenmm` behavior.

**Step 4: Run tests to verify they pass**

Run the same pytest and npm commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/shared/strategy_set.py \
  systems/flux/flux/runners/tokenmm/run_api.py \
  systems/flux/flux/runners/equities/run_api.py \
  flux/api/app.py \
  flux/api/socketio.py \
  fluxboard/config/uiProfiles.ts \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py \
  fluxboard/config/uiProfiles.test.ts
git commit -m "refactor: centralize strategy set profile descriptors"
```

## Task 2: Extract shared runner bootstrap helpers for TokenMM and Equities

**Files:**
- Create: `systems/flux/flux/runners/shared/bootstrap.py`
- Create: `systems/flux/flux/runners/shared/redis_env.py`
- Create: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- Modify: `systems/flux/flux/runners/equities/run_portfolio.py`
- Modify: `systems/flux/flux/runners/tokenmm/redis_runtime.py`
- Modify: `systems/flux/flux/runners/equities/redis_runtime.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Step 1: Write the failing tests**

Add tests that pin the shared behavior rather than the duplicated implementation.

```python
def test_equities_startup_lock_uses_descriptor_specific_lock_dir(tmp_path: Path) -> None:
    config = {"identity": {"strategy_id": "aapl_tradexyz_makerv3"}}
    with _strategy_startup_lock(config, lock_dir=tmp_path):
        assert (tmp_path / "aapl_tradexyz_makerv3.lock").exists()
```

```python
def test_equities_portfolio_allowlist_uses_shared_parser() -> None:
    api_cfg = {"equities_strategy_ids": ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"]}
    assert parse_strategy_ids(api_cfg, field_name="equities_strategy_ids") == [
        "aapl_tradexyz_makerv3",
        "msft_tradexyz_makerv3",
    ]
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
```

Expected: FAIL because those behaviors are still embedded separately in TokenMM and Equities entrypoints.

**Step 3: Write the minimal implementation**

Extract shared helpers for:
- config loading and shared TOML table merge
- Redis env override application by `env_prefix`
- Redis client/database config construction
- allowlist and required-allowlist parsing
- startup lock creation
- portfolio inventory aggregator loop setup

Keep the top-level entrypoints thin and product-named, but move the repeated internals into `runners/shared/*`.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/shared/bootstrap.py \
  systems/flux/flux/runners/shared/redis_env.py \
  systems/flux/flux/runners/shared/portfolio_runner.py \
  systems/flux/flux/runners/tokenmm/run_node.py \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/runners/tokenmm/run_portfolio.py \
  systems/flux/flux/runners/equities/run_portfolio.py \
  systems/flux/flux/runners/tokenmm/redis_runtime.py \
  systems/flux/flux/runners/equities/redis_runtime.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "refactor: share tokenmm and equities runner bootstrap"
```

## Task 3: Make backend strategy metadata explicit for Fluxboard

**Files:**
- Modify: `flux/api/payloads.py`
- Modify: `flux/api/app.py`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/config/paramsProfiles.ts`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `fluxboard/api.flux.test.ts`
- Test: `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Step 1: Write the failing tests**

Add tests that pin explicit metadata fields in backend payloads and frontend normalization.

```python
def test_strategy_metadata_payload_includes_param_set_and_strategy_version() -> None:
    meta = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="equities",
        base_asset="AAPL",
        quote_asset="USD",
        param_set="makerv3",
        strategy_version="v3",
    )
    assert meta.as_payload(strategy_id="aapl_tradexyz_makerv3")["param_set"] == "makerv3"
```

```ts
it('prefers explicit param_set over signature guessing', async () => {
  const rows = await api.getSignals();
  expect(rows.strategies[0].meta?.param_set).toBe('makerv3');
});
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py

cd fluxboard && npm test -- --run api.flux.test.ts __tests__/config/paramsProfiles.test.ts
```

Expected: FAIL because backend metadata does not yet carry enough explicit strategy information.

**Step 3: Write the minimal implementation**

Extend strategy metadata to carry:
- `param_set`
- `strategy_family`
- `strategy_version`

Update Fluxboard to use explicit metadata first and only fall back to signature-based inference for old payloads.

Do not redesign the Params UI; just reduce guesswork.

**Step 4: Run tests to verify they pass**

Run the same pytest and npm commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  flux/api/payloads.py \
  flux/api/app.py \
  fluxboard/types.ts \
  fluxboard/api.ts \
  fluxboard/config/paramsProfiles.ts \
  fluxboard/components/domain/signal/SignalTable.tsx \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  fluxboard/api.flux.test.ts \
  fluxboard/__tests__/config/paramsProfiles.test.ts
git commit -m "feat: expose explicit strategy metadata to fluxboard"
```

## Task 4: Add a minimal strategy registry at the runner boundary

**Files:**
- Create: `flux/strategies/registry.py`
- Modify: `flux/strategies/__init__.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Step 1: Write the failing tests**

Add tests that prove runners resolve strategy implementation through a small registry instead of importing `MakerV3Strategy` directly.

```python
def test_flux_strategy_registry_exposes_canonical_makerv3_binding() -> None:
    spec = get_strategy_spec("makerv3")
    assert spec.param_set == "makerv3"
    assert spec.strategy_cls.__name__ == "MakerV3Strategy"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
```

Expected: FAIL because there is no shared registry abstraction yet.

**Step 3: Write the minimal implementation**

Create a minimal registry:

```python
@dataclass(frozen=True, slots=True)
class FluxStrategySpec:
    name: str
    strategy_cls: type
    config_cls: type
    param_set: str
    strategy_family: str
    strategy_version: str
```

Register only the current `makerv3` strategy. Use it from TokenMM and Equities node runners. Do not attempt plugin loading or multiple versions.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  flux/strategies/registry.py \
  flux/strategies/__init__.py \
  systems/flux/flux/runners/tokenmm/run_node.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "refactor: add minimal flux strategy registry"
```

## Task 5: Unify TokenMM and Equities systemd/install stack generation

**Files:**
- Create: `ops/scripts/deploy/shared_strategy_stack.sh`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `ops/scripts/deploy/tokenmm_stack.sh`
- Modify: `ops/scripts/deploy/equities_stack.sh`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/tokenmm/strategies/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Add tests that prove both stacks are generated from the same conventions:

```python
def test_equities_and_tokenmm_installers_use_shared_strategy_stack_conventions() -> None:
    tokenmm = _read(_repo_root() / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    equities = _read(_repo_root() / "ops/scripts/deploy/install_equities_systemd.sh")
    assert "PULSE_ENABLED=1" in tokenmm
    assert "PULSE_ENABLED=1" in equities
```

Pin the desired differences:
- distinct Pulse group keys
- distinct strategy allowlist fields
- distinct stack env variables

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
```

Expected: FAIL because the scripts still duplicate behavior independently.

**Step 3: Write the minimal implementation**

Extract only the common mechanics:
- discover strategy config files
- build target unit `Wants=`
- emit per-service env files
- set Pulse fields
- print consistent operator instructions

Keep top-level TokenMM and Equities scripts as thin wrappers with their own names, env prefixes, and strategy set descriptors.

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/shared_strategy_stack.sh \
  ops/scripts/deploy/install_tokenmm_systemd.sh \
  ops/scripts/deploy/install_equities_systemd.sh \
  ops/scripts/deploy/tokenmm_stack.sh \
  ops/scripts/deploy/equities_stack.sh \
  deploy/tokenmm/README.md \
  deploy/equities/README.md \
  deploy/tokenmm/strategies/README.md \
  deploy/equities/strategies/README.md \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "refactor: share strategy stack deploy generation"
```

## Task 6: Finish the equities rollout docs and operational contracts

**Files:**
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `deploy/equities/systemd/common.env.example`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Create: `fluxboard/docs/equities_contract.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Add contract tests for the docs/examples you want operators to depend on.

```python
def test_equities_docs_reference_profile_and_portfolio_contracts() -> None:
    readme = _read(_repo_root() / "deploy/equities/README.md")
    assert "/equities" in readme
    assert "/api/v1/params?profile=equities" in readme
    assert "TRADE_XYZ_AGENT_PK" in readme
```

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py
```

Expected: FAIL if docs and contract examples drift from the actual rollout surface.

**Step 3: Write the minimal implementation**

Document:
- stable equities surface identity
- trade[XYZ] env/account fields
- one-strategy-per-stock config pattern
- local smoke vs production install workflow
- profile-scoped API contract examples
- explicit note that future strategy changes should preserve the outer equities surface

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  deploy/equities/systemd/common.env.example \
  deploy/equities/equities.live.toml \
  fluxboard/docs/equities_contract.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "docs: finalize equities rollout contract and ops docs"
```

## Task 7: Run end-to-end verification and write the final review record

**Files:**
- Modify: `docs/plans/2026-03-06-equities-rollout-closeout.md`
- Create: `docs/reviews/2026-03-06-equities-rollout-review.md`

**Step 1: Run the verification matrix**

Run:

```bash
uv run --group test pytest -q \
  tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py \
  tests/integration_tests/adapters/hyperliquid/test_factories.py \
  tests/integration_tests/adapters/hyperliquid/test_execution.py \
  tests/integration_tests/adapters/hyperliquid/test_providers.py \
  tests/integration_tests/adapters/hyperliquid/test_data.py \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py

cargo test -p nautilus-hyperliquid dex -- --nocapture

cd fluxboard && npm test -- --run \
  api.flux.test.ts \
  __tests__/api.test.ts \
  config/uiProfiles.test.ts \
  main.routes.test.tsx \
  sockets.test.ts \
  App.test.tsx \
  Nav.test.tsx
```

Also run the namespace identity check:

```bash
uv run python - <<'PY'
import flux.api.app as a1
import nautilus_trader.flux.api.app as a2
import flux.api.socketio as s1
import nautilus_trader.flux.api.socketio as s2
import flux.api.payloads as p1
import nautilus_trader.flux.api.payloads as p2
print("app", a1 is a2)
print("socketio", s1 is s2)
print("payloads", p1 is p2)
PY
```

**Step 2: Record failures before fixing anything**

Create a review record with findings-first output:

```md
1. Blocking findings
2. Non-blocking findings
3. Residual unrelated failures
4. Verification evidence
```

If unrelated failures remain, list them explicitly with file references and do not relabel them as equities regressions.

**Step 3: Fix only equities-rollout regressions if present**

If verification finds a rollout-specific failure, add a small targeted task before continuing. Do not expand scope into unrelated pre-existing payload/observability issues.

**Step 4: Re-run the affected verification slice**

Re-run only the failing slice plus the namespace identity check.

Expected: PASS for rollout-specific verification. Any remaining red tests must be documented as unrelated residuals.

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-06-equities-rollout-closeout.md \
  docs/reviews/2026-03-06-equities-rollout-review.md
git commit -m "docs: record equities rollout verification and review"
```

## Notes for the implementing engineer

1. Keep the outer equities identity stable. The goal is to stop infrastructure duplication, not to rename the product.
2. Do not build speculative support for future strategy versions. Add only enough indirection to keep runners and Fluxboard from baking in `MakerV3` forever.
3. Prefer explicit metadata over frontend heuristics.
4. Prefer thin wrappers over giant generic frameworks.
5. If a refactor requires touching both `flux/api/*` and `systems/flux/flux/api/*`, remember these files may be hardlinked in this worktree. Treat namespace aliasing as the real bug class, not file duplication.

## Execution Record

- Task 1 completed and verified on the targeted API/profile/UI test slices.
- Task 2 completed and verified on the targeted node/portfolio bootstrap test slices.
- Task 3 completed on the existing explicit metadata path and re-verified on the targeted backend/frontend metadata slices.
- Task 4 completed with a minimal `makerv3` strategy registry and verified on the targeted runner/export slice.
- Task 5 completed with shared strategy-stack install/render helpers and verified on the TokenMM/Equities stack contract slice.
- Task 6 completed with equities deploy/docs contract updates and verified on the equities stack contract slice.
- Task 7 verification matrix results:
  - `uv run --group test pytest ...` rollout matrix: `196 passed in 47.83s`
  - `cargo test -p nautilus-hyperliquid dex -- --nocapture`: passed
  - `cd fluxboard && npm test -- --run ...`: `107 passed`
  - Review-driven rerun slice: `100 passed`
  - Namespace identity: the exact closeout-plan command now returns `app True`, `socketio True`, and `payloads True`.
