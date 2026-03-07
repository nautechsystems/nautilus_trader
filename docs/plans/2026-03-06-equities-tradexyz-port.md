# Equities trade[XYZ] Port Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a dedicated `equities` MakerV3 deployment and Fluxboard surface using trade[XYZ] on Hyperliquid as the first supported execution venue, with one strategy per stock and backend filtering on `profile=equities` and `portfolio=equities`.

**Architecture:** Build a dedicated `equities` stack that mirrors the proven TokenMM production shape instead of overloading `tokenmm`. Extend the Hyperliquid adapter and venue resolution path to explicitly support builder/HIP-3 DEX selection plus agent-wallet style credentials required for trade[XYZ]. Keep the UI contract narrow: reuse the existing `/equities` route and TokenMM-style panels, but drive filtering from Flux API and Socket.IO profile and portfolio scoping.

**Tech Stack:** Python (Flux runners and API), Rust + PyO3 (Hyperliquid adapter), React/TypeScript (Fluxboard), TOML configs, Redis, systemd, pytest, Vitest.

---

## Scope

In scope:
- Dedicated `equities` deployment surface and runner namespace.
- trade[XYZ] as the first live equities venue.
- One strategy config per stock.
- Fluxboard `/equities` view using the same panels as TokenMM.
- Backend filtering on `profile=equities` and `portfolio=equities`.
- Hyperliquid adapter changes needed for trade[XYZ] DEX selection and agent-wallet style auth.

Out of scope:
- IBKR execution.
- Multi-venue equities routing in v1.
- Generic TokenMM refactors unrelated to reuse.
- Broad Fluxboard redesign.
- Repricing strategy logic beyond what is required to make MakerV3 run against the new venue and symbols.

## Assumptions

- `trade[XYZ]` is implemented as a Hyperliquid HIP-3 / builder-deployed DEX path, not as a separate standalone exchange adapter.
- `private_key` remains the signing key, but the execution path needs an explicit account target when the signer is an API/agent wallet.
- The clean v1 boundary is a dedicated `equities` stack rather than making `tokenmm` multi-portfolio.
- Fluxboard already has enough route scaffolding for `/equities`; most remaining work is backend filtering and data contract enforcement.

## Open questions to resolve before implementation starts

- Confirm the canonical venue naming for config and metadata: `tradexyz`, `trade_xyz`, or `hyperliquid` + `dex="xyz"`.
- Confirm whether the account model should be represented as `account_address` or whether PR `#3668` already established a preferred field name in the adapter surface.
- Confirm the initial stock allowlist for v1 so strategy templates, contract catalogs, and deployment scripts can be seeded deterministically.

## Recommended approach

Use `hyperliquid` as the venue family and add an explicit `dex` selector plus explicit execution account targeting. That keeps the adapter aligned with the underlying transport and avoids creating a fake top-level adapter for trade[XYZ]. Create a dedicated `equities` runner and deploy surface, mirroring TokenMM, so profile, portfolio, and service naming stay isolated from `tokenmm`.

Rejected alternatives:
- Overload `tokenmm` with another profile and portfolio: lower short-term file count, higher operator and regression risk.
- Add a brand new `tradexyz` adapter namespace: hides the real Hyperliquid dependency and duplicates adapter work.
- Ship UI-only first: does not satisfy the venue requirement and risks locking the wrong backend contract.

### Task 1: Lock the Hyperliquid trade[XYZ] adapter contract in tests

**Files:**
- Modify: `tests/integration_tests/adapters/hyperliquid/test_factories.py`
- Modify: `tests/integration_tests/adapters/hyperliquid/test_execution.py`
- Create: `tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py`
- Modify: `tests/unit_tests/examples/test_live_venue_registry.py`

**Step 1: Write the failing tests**

Add tests that express the new public contract:

```python
def test_exec_config_accepts_trade_xyz_dex_and_account_address():
    config = HyperliquidExecClientConfig(
        private_key="0xabc",
        account_address="0xdef",
        dex="xyz",
    )
    assert config.account_address == "0xdef"
    assert config.dex == "xyz"
```

```python
def test_factory_passes_account_address_and_dex_to_http_client(monkeypatch):
    ...
```

```python
def test_resolve_strategy_venues_maps_trade_xyz_env_fields():
    ...
```

**Step 2: Run tests to verify they fail**

Run: `pytest tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py tests/integration_tests/adapters/hyperliquid/test_factories.py tests/integration_tests/adapters/hyperliquid/test_execution.py tests/unit_tests/examples/test_live_venue_registry.py -q`
Expected: FAIL because `account_address` and `dex` are not implemented yet.

**Step 3: Write the minimal implementation**

Plan the implementation around these public fields:

```python
class HyperliquidExecClientConfig(...):
    private_key: str | None = None
    account_address: str | None = None
    vault_address: str | None = None
    dex: str | None = None
```

Keep `private_key` as the signer key. Use `account_address` for the actual account being queried, subscribed, and traded when signer and account differ.

**Step 4: Run tests to verify they pass**

Run the same pytest command.
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py tests/integration_tests/adapters/hyperliquid/test_factories.py tests/integration_tests/adapters/hyperliquid/test_execution.py tests/unit_tests/examples/test_live_venue_registry.py nautilus_trader/adapters/hyperliquid/config.py nautilus_trader/adapters/hyperliquid/factories.py crates/adapters/hyperliquid/src/python/config.rs

git commit -m "feat: add trade xyz hyperliquid config contract"
```

### Task 2: Wire adapter config through Rust, PyO3, and factory surfaces

**Files:**
- Modify: `nautilus_trader/adapters/hyperliquid/config.py`
- Modify: `nautilus_trader/adapters/hyperliquid/factories.py`
- Modify: `crates/adapters/hyperliquid/src/config.rs`
- Modify: `crates/adapters/hyperliquid/src/python/config.rs`
- Modify: `crates/adapters/hyperliquid/src/python/http.rs`
- Modify: `crates/adapters/hyperliquid/src/http/client.rs`
- Modify: `crates/adapters/hyperliquid/src/execution/mod.rs`

**Step 1: Write the failing tests**

Add targeted assertions that the PyO3 HTTP client and execution client use `account_address` when present and preserve existing `vault_address` behavior.

```python
def test_execution_client_prefers_account_address_for_user_queries():
    ...
```

**Step 2: Run tests to verify they fail**

Run: `pytest tests/integration_tests/adapters/hyperliquid/test_execution.py -q`
Expected: FAIL because the execution path still derives the account only from signer or vault.

**Step 3: Write the minimal implementation**

Implement this precedence:

```text
user/account target = account_address if set else vault_address if set else signer_eoa
```

Do not break vault trading. `vault_address` remains the vault field used for signed actions; `account_address` is the logical account scope for info requests and subscriptions.

**Step 4: Run tests to verify they pass**

Run: `pytest tests/integration_tests/adapters/hyperliquid/test_execution.py tests/integration_tests/adapters/hyperliquid/test_factories.py -q`
Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/hyperliquid/config.py nautilus_trader/adapters/hyperliquid/factories.py crates/adapters/hyperliquid/src/config.rs crates/adapters/hyperliquid/src/python/config.rs crates/adapters/hyperliquid/src/python/http.rs crates/adapters/hyperliquid/src/http/client.rs crates/adapters/hyperliquid/src/execution/mod.rs tests/integration_tests/adapters/hyperliquid/test_execution.py tests/integration_tests/adapters/hyperliquid/test_factories.py

git commit -m "feat: support explicit hyperliquid account targeting"
```

### Task 3: Add trade[XYZ] DEX-scoped instrument discovery and subscriptions

**Files:**
- Modify: `nautilus_trader/adapters/hyperliquid/providers.py`
- Modify: `nautilus_trader/adapters/hyperliquid/data.py`
- Modify: `nautilus_trader/adapters/hyperliquid/execution.py`
- Modify: `crates/adapters/hyperliquid/src/http/client.rs`
- Modify: `crates/adapters/hyperliquid/src/http/models.rs`
- Modify: `crates/adapters/hyperliquid/src/http/parse.rs`
- Modify: `crates/adapters/hyperliquid/src/websocket/client.rs`
- Modify: `tests/integration_tests/adapters/hyperliquid/conftest.py`
- Modify: `tests/integration_tests/adapters/hyperliquid/test_providers.py`
- Modify: `tests/integration_tests/adapters/hyperliquid/test_data.py`

**Step 1: Write the failing tests**

Add provider and data-client tests that cover:
- requesting metadata for a builder DEX (`dex="xyz"`),
- parsing builder-perp asset identifiers,
- subscribing with DEX-scoped websocket payloads,
- loading only the requested trade[XYZ] equities symbols.

```python
def test_provider_loads_builder_perps_for_trade_xyz_dex():
    ...
```

```python
def test_data_client_subscribes_with_dex_scoped_all_mids():
    ...
```

**Step 2: Run tests to verify they fail**

Run: `pytest tests/integration_tests/adapters/hyperliquid/test_providers.py tests/integration_tests/adapters/hyperliquid/test_data.py -q`
Expected: FAIL because the provider currently assumes the default Hyperliquid metadata path.

**Step 3: Write the minimal implementation**

Implement provider and websocket behavior that:
- switches metadata calls to the DEX-aware path when `dex` is configured,
- maps builder-perp asset IDs using existing `builder_perp` primitives,
- keeps default Hyperliquid spot/perp behavior unchanged when `dex` is unset.

**Step 4: Run tests to verify they pass**

Run the same pytest command.
Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/hyperliquid/providers.py nautilus_trader/adapters/hyperliquid/data.py nautilus_trader/adapters/hyperliquid/execution.py crates/adapters/hyperliquid/src/http/client.rs crates/adapters/hyperliquid/src/http/models.rs crates/adapters/hyperliquid/src/http/parse.rs crates/adapters/hyperliquid/src/websocket/client.rs tests/integration_tests/adapters/hyperliquid/conftest.py tests/integration_tests/adapters/hyperliquid/test_providers.py tests/integration_tests/adapters/hyperliquid/test_data.py

git commit -m "feat: add trade xyz dex market discovery"
```

### Task 4: Teach Flux venue resolution about trade[XYZ] credentials and DEX config

**Files:**
- Modify: `systems/flux/flux/runners/live/venues.py`
- Modify: `tests/unit_tests/examples/test_live_venue_registry.py`
- Modify: `examples/live/hyperliquid/hyperliquid_exec_tester.py`
- Create: `examples/live/hyperliquid/trade_xyz_exec_tester.py`

**Step 1: Write the failing tests**

Add venue registry tests for a config like:

```python
config = {
    "venues": {
        "execution_venue": "HYPERLIQUID",
        "reference_venue": "HYPERLIQUID",
    },
    "node": {
        "venues": {
            "HYPERLIQUID": {
                "instrument_id": "AAPL-USD-PERP.HYPERLIQUID",
                "execution": True,
                "dex": "xyz",
                "private_key_env": "TRADE_XYZ_AGENT_PK",
                "account_address_env": "TRADE_XYZ_ACCOUNT",
            },
        },
    },
}
```

**Step 2: Run tests to verify they fail**

Run: `pytest tests/unit_tests/examples/test_live_venue_registry.py -q`
Expected: FAIL because the venue registry does not yet pass through `dex` and `account_address`.

**Step 3: Write the minimal implementation**

Allow the Hyperliquid venue spec to resolve:
- `private_key` / `private_key_env`
- `account_address` / `account_address_env`
- `vault_address` / `vault_address_env`
- `dex`

Keep legacy Hyperliquid configs working unchanged.

**Step 4: Run tests to verify they pass**

Run: `pytest tests/unit_tests/examples/test_live_venue_registry.py -q`
Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/live/venues.py tests/unit_tests/examples/test_live_venue_registry.py examples/live/hyperliquid/hyperliquid_exec_tester.py examples/live/hyperliquid/trade_xyz_exec_tester.py

git commit -m "feat: wire trade xyz venue settings through flux runners"
```

### Task 5: Create a dedicated `equities` runner namespace by mirroring TokenMM

**Files:**
- Create: `systems/flux/flux/runners/equities/run_api.py`
- Create: `systems/flux/flux/runners/equities/run_node.py`
- Create: `systems/flux/flux/runners/equities/run_bridge.py`
- Create: `systems/flux/flux/runners/equities/run_portfolio.py`
- Create: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Create: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Create: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Step 1: Write the failing tests**

Create tests that encode the new contracts:
- profile strategy maps are built from `api.equities_strategy_ids`,
- portfolio aggregation uses `portfolio_id = "equities"`,
- node startup locks are strategy-specific and independent from TokenMM,
- the API host exposes `/equities` filtering through the same endpoints as TokenMM.

**Step 2: Run tests to verify they fail**

Run: `pytest tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -q`
Expected: FAIL because the `equities` runner namespace does not exist yet.

**Step 3: Write the minimal implementation**

Mirror TokenMM behavior but rename and scope the stack cleanly:

```text
flux.runners.equities.run_node
flux.runners.equities.run_portfolio
flux.runners.equities.run_bridge
flux.runners.equities.run_api
```

Use dedicated config keys such as `equities_strategy_ids`, `equities_required_strategy_ids`, and `portfolio_id = "equities"`.

**Step 4: Run tests to verify they pass**

Run the same pytest command.
Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/equities/run_api.py systems/flux/flux/runners/equities/run_node.py systems/flux/flux/runners/equities/run_bridge.py systems/flux/flux/runners/equities/run_portfolio.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py

git commit -m "feat: add dedicated equities runner stack"
```

### Task 6: Add equities deployment configs, templates, and systemd assets

**Files:**
- Create: `deploy/equities/README.md`
- Create: `deploy/equities/equities.live.toml`
- Create: `deploy/equities/equities_stack.env.example`
- Create: `deploy/equities/strategies/README.md`
- Create: `deploy/equities/strategies/equities.strategy.template.toml`
- Create: `deploy/equities/systemd/flux-equities.target`
- Create: `ops/scripts/deploy/equities_stack.sh`
- Create: `ops/scripts/deploy/install_equities_systemd.sh`
- Create: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Create contract tests modeled after the TokenMM stack tests. Encode:
- safe paper-mode defaults,
- no fallback to TokenMM or MakerV3 legacy surfaces,
- dedicated `portfolio_id = "equities"`,
- explicit strategy allowlists,
- strategy config naming conventions for one-stock-per-strategy,
- systemd units named under `equities` rather than `tokenmm`.

**Step 2: Run tests to verify they fail**

Run: `pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py -q`
Expected: FAIL because the deploy surface does not exist yet.

**Step 3: Write the minimal implementation**

Seed the first config set for trade[XYZ] with placeholders for the stock allowlist. The shared top-level config owns:
- API strategy allowlist,
- portfolio ID,
- contract catalog,
- shared redis and API settings.

The per-strategy template owns:
- `identity.strategy_id`,
- execution and reference venue symbols,
- MakerV3 parameters,
- strategy group metadata for `equities`.

**Step 4: Run tests to verify they pass**

Run: `pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py -q`
Expected: PASS.

**Step 5: Commit**

```bash
git add deploy/equities/README.md deploy/equities/equities.live.toml deploy/equities/equities_stack.env.example deploy/equities/strategies/README.md deploy/equities/strategies/equities.strategy.template.toml deploy/equities/systemd/flux-equities.target ops/scripts/deploy/equities_stack.sh ops/scripts/deploy/install_equities_systemd.sh tests/unit_tests/examples/strategies/test_equities_stack_contract.py

git commit -m "feat: add equities deployment stack"
```

### Task 7: Wire Flux API and Socket.IO profile and portfolio filtering for equities

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/socketio.py`
- Modify: `systems/flux/flux/api/payloads.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Create: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`

**Step 1: Write the failing tests**

Add API and socket contract tests that prove:
- `GET /api/v1/signals?profile=equities` returns only equities strategies,
- `GET /api/v1/params?profile=equities` scopes to the equities allowlist,
- `GET /api/v1/balances?profile=equities` uses `portfolio_id = "equities"`,
- `GET /api/v1/trades?profile=equities` returns only equities strategies,
- socket rooms normalize and emit `profile:equities` traffic separately from TokenMM.

**Step 2: Run tests to verify they fail**

Run: `pytest tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/examples/strategies/test_equities_run_api.py -q`
Expected: FAIL because equities profile maps and portfolio scoping are incomplete.

**Step 3: Write the minimal implementation**

Use the new `equities_strategy_ids` and `equities_required_strategy_ids` lists as the source of truth for API filtering. Keep `strategy=` overrides higher priority than `profile=equities`, mirroring existing TokenMM behavior.

**Step 4: Run tests to verify they pass**

Run the same pytest command.
Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/api/app.py systems/flux/flux/api/socketio.py systems/flux/flux/api/payloads.py systems/flux/flux/runners/equities/run_api.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/examples/strategies/test_equities_run_api.py

git commit -m "feat: scope flux api to equities profile and portfolio"
```

### Task 8: Finish Fluxboard equities contract and guardrail tests

**Files:**
- Modify: `fluxboard/__tests__/api.flux.test.ts`
- Modify: `fluxboard/__tests__/panels/signal.test.tsx`
- Modify: `fluxboard/main.routes.test.tsx`
- Modify: `fluxboard/config/uiProfiles.test.ts`
- Modify: `fluxboard/sockets.test.ts`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/sockets.ts`
- Modify: `fluxboard/api.ts`

**Step 1: Write the failing tests**

Add or tighten tests that verify:
- `/equities` exposes the same panel set as TokenMM,
- API requests append `profile=equities`,
- socket handshakes join `profile:equities`,
- signal filtering prefers `strategy_groups=equities` and falls back to `meta.chain == "equities"`,
- non-equities strategies do not appear on the equities surface.

**Step 2: Run tests to verify they fail**

Run: `cd fluxboard && pnpm vitest run __tests__/api.flux.test.ts main.routes.test.tsx config/uiProfiles.test.ts sockets.test.ts __tests__/panels/signal.test.tsx`
Expected: FAIL if the backend contract assumptions are not reflected cleanly in the UI.

**Step 3: Write the minimal implementation**

Only change Fluxboard code where tests prove it is necessary. The expected end state is still the current narrow surface:
- `dashboard`
- `signal`
- `params`
- `balances`
- `trades`
- `alerts`

No `order-view`. No broader trader surface.

**Step 4: Run tests to verify they pass**

Run the same Vitest command.
Expected: PASS.

**Step 5: Commit**

```bash
git add fluxboard/__tests__/api.flux.test.ts fluxboard/__tests__/panels/signal.test.tsx fluxboard/main.routes.test.tsx fluxboard/config/uiProfiles.test.ts fluxboard/sockets.test.ts fluxboard/components/domain/signal/SignalTable.tsx fluxboard/sockets.ts fluxboard/api.ts

git commit -m "feat: finalize equities fluxboard contract"
```

### Task 9: Seed first-pass equities strategy templates for one-stock-per-strategy

**Files:**
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Create: `deploy/equities/strategies/<stock>_tradexyz_makerv3.toml`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Add tests that enforce:
- each seeded stock strategy has a unique descriptive Flux strategy ID,
- all seeded strategy files are listed in `api.equities_strategy_ids`,
- all configs set `strategy_groups = "equities"`,
- the execution venue uses Hyperliquid with `dex = "xyz"`,
- the reference venue and symbol are explicit per stock.

**Step 2: Run tests to verify they fail**

Run: `pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py -q`
Expected: FAIL until the initial stock configs and allowlist are in sync.

**Step 3: Write the minimal implementation**

Use a naming convention like:

```text
<stock>_tradexyz_makerv3
```

and keep the shared config allowlist as the source of truth. Seed only the approved v1 stock set.

**Step 4: Run tests to verify they pass**

Run the same pytest command.
Expected: PASS.

**Step 5: Commit**

```bash
git add deploy/equities/equities.live.toml deploy/equities/strategies/equities.strategy.template.toml deploy/equities/strategies/*.toml tests/unit_tests/examples/strategies/test_equities_stack_contract.py

git commit -m "feat: seed equities strategy configs"
```

### Task 10: Document operator workflow and venue caveats

**Files:**
- Modify: `docs/integrations/hyperliquid.md`
- Create: `fluxboard/docs/equities_runbook.md`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`

**Step 1: Write the failing tests**

Extend the stack contract tests to assert docs mention:
- `profile=equities`,
- `portfolio=equities`,
- `flux.runners.equities.*` entry points,
- trade[XYZ] DEX and account-address env vars,
- local smoke vs production systemd flows.

**Step 2: Run tests to verify they fail**

Run: `pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py -q`
Expected: FAIL until docs and scripts agree.

**Step 3: Write the minimal implementation**

Document:
- required environment variables,
- how agent-wallet auth maps into config,
- how to start paper-mode local smoke,
- how to install systemd units,
- which endpoints and UI routes are the acceptance checks.

**Step 4: Run tests to verify they pass**

Run the same pytest command.
Expected: PASS.

**Step 5: Commit**

```bash
git add docs/integrations/hyperliquid.md fluxboard/docs/equities_runbook.md deploy/equities/README.md deploy/equities/strategies/README.md tests/unit_tests/examples/strategies/test_equities_stack_contract.py

git commit -m "docs: add equities trade xyz runbooks"
```

### Task 11: End-to-end verification and smoke acceptance

**Files:**
- Modify: `docs/plans/2026-03-06-equities-tradexyz-port.md`

**Step 1: Run the targeted backend tests**

Run:

```bash
pytest tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py \
  tests/integration_tests/adapters/hyperliquid/test_factories.py \
  tests/integration_tests/adapters/hyperliquid/test_execution.py \
  tests/integration_tests/adapters/hyperliquid/test_providers.py \
  tests/integration_tests/adapters/hyperliquid/test_data.py \
  tests/unit_tests/examples/test_live_venue_registry.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -q
```

Expected: PASS.

**Step 2: Run the Fluxboard tests**

Run:

```bash
cd fluxboard && pnpm vitest run __tests__/api.flux.test.ts main.routes.test.tsx config/uiProfiles.test.ts sockets.test.ts __tests__/panels/signal.test.tsx
```

Expected: PASS.

**Step 3: Run script syntax checks and local smoke**

Run:

```bash
bash -n ops/scripts/deploy/equities_stack.sh
EQUITIES_MODE=paper EQUITIES_ENABLE_EXECUTION=0 bash ops/scripts/deploy/equities_stack.sh
curl -fsS 'http://127.0.0.1:5022/api/v1/params?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/trades?profile=equities'
```

Expected: syntax check succeeds, local stack boots in paper mode, and all three endpoints return equities-scoped payloads.

**Step 4: Record acceptance evidence**

Append a short verification log to this plan file with the exact commands run and observed outputs.

**Step 5: Commit**

```bash
git add docs/plans/2026-03-06-equities-tradexyz-port.md

git commit -m "chore: record equities verification evidence"
```

## Acceptance checklist

- `trade[XYZ]` is reachable through the Hyperliquid adapter with explicit DEX selection.
- Agent-wallet style auth is represented explicitly and does not overload vault trading semantics.
- `flux.runners.equities.*` exists and is independent from `flux.runners.tokenmm.*`.
- `deploy/equities/` contains a complete, paper-safe stack.
- `/equities` in Fluxboard shows the same panel family as TokenMM.
- API and socket filtering honor `profile=equities` and `portfolio=equities`.
- One stock equals one strategy config and one allowlist entry.
- Docs and scripts show a coherent operator workflow.

## Risks to watch during execution

- Hyperliquid API-wallet semantics may force a field name or precedence change once PR `#3668` details are confirmed.
- trade[XYZ] symbol and asset-ID mapping may require a provider-level normalization layer not visible in current TokenMM flows.
- Funding, holiday closures, and roll behavior may require strategy parameter defaults or risk guardrails beyond pure venue plumbing.
- If the seeded stock list is large, systemd and env generation should be templated early to avoid hand-maintained drift.

Plan complete and saved to `docs/plans/2026-03-06-equities-tradexyz-port.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
