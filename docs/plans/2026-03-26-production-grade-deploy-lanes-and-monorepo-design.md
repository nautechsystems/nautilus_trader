# Production-Grade Deploy Lanes And Monorepo Design

**Date:** 2026-03-26

## Goal

Define a production-grade operating model for this trading monorepo on a shared host so that:

- development stays fast and agent-friendly
- live validation can happen in a formal `pilot` lane
- trusted trading stays in a stable `prod` lane
- monorepo ownership and naming become clearer over time without destabilizing live deploys

## Current Problems

1. Live services are still pinned to mutable roots.
   - `equities` currently points at a git worktree via `/etc/flux/equities-*.env`.
   - `tokenmm` currently points at the mutable home checkout via `/etc/flux/tokenmm-*.env`.
2. The deploy contract is inconsistent by stack.
   - TokenMM already rejects worktree deploy roots.
   - Equities, LP, and TG bots still render env files from the current checkout.
3. There is no first-class lane model.
   - The repo has stack-specific deploy docs, but no authoritative `dev` vs `pilot` vs `prod` contract.
4. Repo and worktree sprawl already affects operations.
   - Multiple top-level clones exist in `~`.
   - Worktrees are split across repo-local `.worktrees`, `~/.config/superpowers/worktrees/...`, and `~/.worktrees/...`.
5. The monorepo structure is directionally good but still transitional.
   - `systems/`, `apps/`, `ops/`, and `tooling/` are the intended ownership boundaries.
   - Compatibility paths and mirrored app/doc surfaces still exist.

## Chosen Design

### 1. Three Lanes, Two Live Lanes

The system will use three lanes:

- `dev`: one mutable canonical repo plus one approved worktree location
- `pilot`: immutable release lane for the stack under test
- `prod`: immutable release lane for approved live trading

`dev` is where code changes happen.

`pilot` and `prod` are where live services run.

The hard rule is:

- no live service may point at the canonical dev checkout
- no live service may point at `.worktrees/*`
- no one hot-edits an active pilot or prod release root

### 2. Immutable Release Roots

Every live lane must resolve to a pinned release root rather than a mutable repo path.

Recommended host layout:

```text
~/nautilus_trader                         # canonical mutable dev repo for now
~/nautilus_trader/.worktrees             # canonical worktree location

~/releases/pilot/equities/releases/<timestamp>-<sha>
~/releases/pilot/equities/current
~/releases/prod/equities/releases/<timestamp>-<sha>
~/releases/prod/equities/current

~/releases/pilot/tokenmm/releases/<timestamp>-<sha>
~/releases/pilot/tokenmm/current
~/releases/prod/tokenmm/releases/<timestamp>-<sha>
~/releases/prod/tokenmm/current
```

The parent path is not the important part. The important part is that:

- release roots are separate from the mutable dev repo
- `current` is the only path env files should point at
- rollback is a symlink repoint plus service restart

Each release should carry explicit metadata, for example:

- source commit SHA
- source branch/worktree path
- build timestamp
- stack name
- intended lane

### 3. Namespaced Services Per Lane

The existing `flux@.service` model is good enough and should remain the base unit contract.

What changes is service naming.

Example for equities:

- prod:
  - `equities-api`
  - `equities-portfolio`
  - `equities-bridge`
  - `equities-node-*`
- pilot:
  - `equities-pilot-api`
  - `equities-pilot-portfolio`
  - `equities-pilot-bridge`
  - `equities-pilot-node-*`

Each lane must have:

- its own service IDs
- its own systemd target
- its own ports where applicable
- its own state/data directories where collision is possible
- its own Pulse group key and label

This keeps one Pulse surface while making pilot and prod operationally distinct.

### 4. One Pulse Surface, Multiple Groups

Pulse should stay as one control plane served at `/pulse`.

The supported operating model is:

- one Pulse instance
- multiple groups such as `tokenmm`, `equities`, `equities-pilot`, `lp`

Pulse should not need a second app or second base path to support pilot.

The contract becomes:

- “deploy equities to pilot” means create a new pinned pilot release, rewrite only the `equities-pilot-*` envs, and restart only the pilot target/services
- “bounce equities pilot” means restart only the `equities-pilot-*` services
- “promote equities pilot to prod” means deploy the exact tested pilot release into the prod release root and repoint only prod

### 5. Shared-Host Operator Model

This design assumes one shared host for now.

Production-grade on one shared host means:

- journald remains the host log source of truth
- live control remains `systemd` plus Pulse
- repo/worktree cleanup is part of production hygiene, not a separate convenience task
- pilot and prod are isolated by path, service namespace, state path, and config, not by hand-maintained convention

Live debugging workflow:

1. observe issue in `pilot`
2. inspect pilot logs/state
3. fix in `dev` worktree
4. cut a new pinned pilot release
5. bounce pilot only
6. repeat until validated
7. promote the exact release to prod

This preserves the fast iteration loop without turning pilot or prod into mutable dev environments.

### 6. Monorepo Ownership Model

The repo-level ownership model documented in `README.md` and `docs/repo/*` is the correct direction:

- `engine/` owns reusable engine/runtime capabilities
- `systems/` owns deployable proprietary systems
- `apps/` owns operator-facing UIs
- `ops/` owns deploy/runtime operations assets
- `tooling/` owns dev/CI/release automation
- `research/` owns non-production experimentation

Production-grade means this becomes the real source of truth rather than an aspirational migration note.

Near-term recommendations:

- keep `nautilus_trader` as the engine/runtime namespace
- keep `systems/flux` as a transitional system boundary
- do not do a flag-day import rename
- keep compatibility shims only where callers still need them

### 7. Naming Direction

Recommended naming model:

- product / repo / deploy surface: `flux`
- engine runtime identity: `nautilus_trader`
- systems, apps, ops, and tooling stay domain-based

This means:

- the repo can later be renamed to `flux`
- the engine can still be referred to canonically as Nautilus Trader within the monorepo
- import namespaces do not need to churn immediately

Important constraint:

- do not rename the repo/product/deploy surface before live deploy lanes are hardened

If the repo is later renamed to `flux`, the internal `systems/flux` path should be treated as transitional and reviewed later for a more role-based name if it remains confusing.

## Release Materialization Contract

Pilot and prod releases should be materialized from dev/worktree state through one common workflow:

1. resolve the source commit/worktree
2. create a clean release directory under the lane/stack release root
3. populate source files from the chosen commit/worktree snapshot
4. build lane-required assets in the release root
5. write release metadata
6. repoint `current`
7. regenerate lane env files from `current`
8. restart lane services

This should become a shared deploy helper instead of each stack improvising its own root resolution.

## Error Handling And Safety Rules

Installer and release helpers should fail closed when:

- the resolved deploy root is a git worktree
- the release metadata is missing or mismatched
- a lane namespace is incomplete
- required env files are missing after generation
- a lane tries to reuse prod ports or state paths
- the target checkout/release does not contain the expected scripts, configs, or virtualenv

Rollbacks should be simple:

- repoint `current` to the previous release
- restart only the affected lane

## Security And Exposure

Production-grade also means:

- Pulse and privileged API routes remain internal-only by default
- `0.0.0.0` bindings are used only when there is an explicit secure edge in front
- live state and telemetry paths are namespaced per lane where collision is possible

This design does not add new auth/authz features yet, but it does tighten the boundary around what path and what service is allowed to be live.

## Tradeoffs

### Recommended Approach: immutable pilot/prod lanes on one host

Pros:

- lowest infrastructure overhead
- preserves your fast live-debug loop
- clean rollback model
- clear agent/operator contract

Cons:

- requires up-front deploy helper work
- requires service naming and env generation cleanup
- still shares machine resources between dev and live lanes

### Rejected Alternative: keep live bound to mutable repos

Pros:

- fastest short-term

Cons:

- no trustworthy rollback contract
- easy for agents to deploy unintended code
- live drift remains normal
- impossible to call the result production-grade

### Deferred Alternative: full namespace rename first

Pros:

- branding looks cleaner sooner

Cons:

- distracts from the actual production boundary
- increases churn before the deploy model is stable

## Success Criteria

- every live service points at a pinned release root, never at the canonical dev repo or a worktree
- `equities-pilot` exists as a first-class lane with distinct service IDs and Pulse grouping
- all installer scripts share the same stable-root policy
- one canonical repo and one canonical worktree location are documented and enforced
- agent instructions define exact meanings for `deploy to pilot`, `bounce pilot`, and `promote to prod`
- repo/product naming cleanup is explicitly sequenced after deploy-lane hardening
